//! Licenses command - show dependency licenses.

use anyhow::Result;
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::collections::HashMap;

/// Arguments for the licenses command
#[derive(Args, Debug, Clone)]
pub struct LicensesArgs {
    /// Output format (text, json, summary)
    #[arg(short = 'f', long, default_value = "text")]
    pub format: String,

    /// Only show dev dependencies
    #[arg(long)]
    pub dev: bool,

    /// Only show production dependencies
    #[arg(long)]
    pub no_dev: bool,
}

/// License information for a package
#[derive(Debug, Clone)]
struct PackageLicense {
    name: String,
    version: String,
    license: Vec<String>,
    is_dev: bool,
}

/// Run the licenses command
pub async fn run(args: LicensesArgs) -> Result<()> {
    use crate::output::{header, info};

    header("Dependency licenses");

    let lock_path = std::env::current_dir()?.join("composer.lock");
    if !lock_path.exists() {
        anyhow::bail!("composer.lock not found - run 'libretto install' first");
    }

    let lock_content = std::fs::read_to_string(&lock_path)?;
    let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

    let mut licenses: Vec<PackageLicense> = Vec::new();

    // Collect license info from packages
    if !args.dev
        && let Some(packages) = lock.get("packages").and_then(|v| v.as_array())
    {
        for pkg in packages {
            licenses.push(extract_license(pkg, false));
        }
    }

    if !args.no_dev
        && let Some(packages) = lock.get("packages-dev").and_then(|v| v.as_array())
    {
        for pkg in packages {
            licenses.push(extract_license(pkg, true));
        }
    }

    if licenses.is_empty() {
        info("No packages found");
        return Ok(());
    }

    // Sort by name
    licenses.sort_by(|a, b| a.name.cmp(&b.name));

    match args.format.as_str() {
        "json" => output_json(&licenses)?,
        "summary" => output_summary(&licenses)?,
        _ => output_text(&licenses)?,
    }

    Ok(())
}

fn extract_license(pkg: &sonic_rs::Value, is_dev: bool) -> PackageLicense {
    let name = pkg
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let version = pkg
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let license = if let Some(lic) = pkg.get("license") {
        if let Some(arr) = lic.as_array() {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else if let Some(s) = lic.as_str() {
            vec![s.to_string()]
        } else {
            vec!["Unknown".to_string()]
        }
    } else {
        vec!["Unknown".to_string()]
    };

    PackageLicense {
        name,
        version,
        license,
        is_dev,
    }
}

fn output_text(licenses: &[PackageLicense]) -> Result<()> {
    use crate::output::table::{Table, TableStyle};
    use owo_colors::OwoColorize;

    let colors = crate::output::colors_enabled();

    let mut table = Table::with_style(TableStyle::Minimal);
    table.headers(["Package", "Version", "License", "Type"]);

    for pkg in licenses {
        let license_str = pkg.license.join(", ");
        let pkg_type = if pkg.is_dev { "dev" } else { "prod" };

        let license_cell = if is_permissive(&license_str) {
            table.success_cell(&license_str)
        } else if is_copyleft(&license_str) {
            table.warning_cell(&license_str)
        } else {
            comfy_table::Cell::new(&license_str)
        };

        table.styled_row(vec![
            comfy_table::Cell::new(&pkg.name),
            comfy_table::Cell::new(&pkg.version),
            license_cell,
            comfy_table::Cell::new(pkg_type),
        ]);
    }

    table.print();

    // Legend
    println!();
    if colors {
        println!("License types:");
        println!("  {} - Permissive (MIT, BSD, Apache)", "Green".green());
        println!("  {} - Copyleft (GPL, LGPL, AGPL)", "Yellow".yellow());
        println!("  Normal - Other/Unknown");
    }

    Ok(())
}

fn output_json(licenses: &[PackageLicense]) -> Result<()> {
    let output: Vec<_> = licenses
        .iter()
        .map(|pkg| {
            sonic_rs::json!({
                "name": pkg.name,
                "version": pkg.version,
                "license": pkg.license,
                "dev": pkg.is_dev
            })
        })
        .collect();

    println!("{}", sonic_rs::to_string_pretty(&output)?);
    Ok(())
}

fn output_summary(licenses: &[PackageLicense]) -> Result<()> {
    use crate::output::table::Table;

    // Count licenses
    let mut counts: HashMap<String, (usize, usize)> = HashMap::new(); // (prod, dev)

    for pkg in licenses {
        for license in &pkg.license {
            let entry = counts.entry(license.clone()).or_default();
            if pkg.is_dev {
                entry.1 += 1;
            } else {
                entry.0 += 1;
            }
        }
    }

    // Sort by total count
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| (b.1.0 + b.1.1).cmp(&(a.1.0 + a.1.1)));

    let mut table = Table::new();
    table.headers(["License", "Production", "Dev", "Total"]);

    let mut total_prod = 0;
    let mut total_dev = 0;

    for (license, (prod, dev)) in &sorted {
        total_prod += prod;
        total_dev += dev;

        let license_cell = if is_permissive(license) {
            table.success_cell(license)
        } else if is_copyleft(license) {
            table.warning_cell(license)
        } else {
            comfy_table::Cell::new(license)
        };

        table.styled_row(vec![
            license_cell,
            comfy_table::Cell::new(prod),
            comfy_table::Cell::new(dev),
            comfy_table::Cell::new(prod + dev),
        ]);
    }

    // Total row
    table.row([
        "TOTAL",
        &total_prod.to_string(),
        &total_dev.to_string(),
        &(total_prod + total_dev).to_string(),
    ]);

    table.print();

    // Summary stats
    println!();
    let permissive = sorted
        .iter()
        .filter(|(l, _)| is_permissive(l))
        .map(|(_, c)| c.0 + c.1)
        .sum::<usize>();
    let copyleft = sorted
        .iter()
        .filter(|(l, _)| is_copyleft(l))
        .map(|(_, c)| c.0 + c.1)
        .sum::<usize>();
    let other = total_prod + total_dev - permissive - copyleft;

    println!("Summary: {permissive} permissive, {copyleft} copyleft, {other} other");

    Ok(())
}

fn is_permissive(license: &str) -> bool {
    let lower = license.to_lowercase();
    lower.contains("mit")
        || lower.contains("bsd")
        || lower.contains("apache")
        || lower.contains("isc")
        || lower.contains("unlicense")
        || lower.contains("wtfpl")
        || lower.contains("cc0")
        || lower.contains("public domain")
}

fn is_copyleft(license: &str) -> bool {
    let lower = license.to_lowercase();
    (lower.contains("gpl") || lower.contains("agpl") || lower.contains("lgpl"))
        && !lower.contains("exception")
}
