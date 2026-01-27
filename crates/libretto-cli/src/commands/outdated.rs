//! Outdated command - show packages with available updates.

use anyhow::Result;
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};

/// Arguments for the outdated command
#[derive(Args, Debug, Clone)]
pub struct OutdatedArgs {
    /// Packages to check (all if omitted)
    #[arg(value_name = "PACKAGE")]
    pub packages: Vec<String>,

    /// Show all packages, not just outdated ones
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Only show direct dependencies
    #[arg(short = 'D', long)]
    pub direct: bool,

    /// Only show minor and patch updates
    #[arg(short = 'm', long)]
    pub minor_only: bool,

    /// Output format (text, json)
    #[arg(short = 'f', long, default_value = "text")]
    pub format: String,

    /// Exit with non-zero if any package is outdated
    #[arg(long)]
    pub strict: bool,
}

/// Package update information
#[derive(Debug, Clone)]
struct PackageUpdate {
    name: String,
    current: String,
    latest: String,
    constraint: String,
    is_direct: bool,
    is_dev: bool,
    update_type: UpdateType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum UpdateType {
    Major,
    Minor,
    Patch,
    UpToDate,
}

/// Run the outdated command
pub async fn run(args: OutdatedArgs) -> Result<()> {
    use crate::output::progress::Spinner;
    use crate::output::{header, info, warning};
    use libretto_repository::Repository;

    header("Checking for updates");

    let lock_path = std::env::current_dir()?.join("composer.lock");
    let composer_path = std::env::current_dir()?.join("composer.json");

    if !lock_path.exists() {
        anyhow::bail!("composer.lock not found - run 'libretto install' first");
    }

    let lock_content = std::fs::read_to_string(&lock_path)?;
    let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

    // Get direct dependencies from composer.json
    let mut direct_deps: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut constraints: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    if composer_path.exists() {
        let composer_content = std::fs::read_to_string(&composer_path)?;
        let composer: sonic_rs::Value = sonic_rs::from_str(&composer_content)?;

        if let Some(require) = composer.get("require").and_then(|v| v.as_object()) {
            for (name, constraint) in require {
                direct_deps.insert(name.to_string());
                if let Some(c) = constraint.as_str() {
                    constraints.insert(name.to_string(), c.to_string());
                }
            }
        }

        if let Some(require_dev) = composer.get("require-dev").and_then(|v| v.as_object()) {
            for (name, constraint) in require_dev {
                direct_deps.insert(name.to_string());
                if let Some(c) = constraint.as_str() {
                    constraints.insert(name.to_string(), c.to_string());
                }
            }
        }
    }

    // Collect installed packages
    let mut installed: Vec<(String, String, bool)> = Vec::new();

    for (key, is_dev) in [("packages", false), ("packages-dev", true)] {
        if let Some(packages) = lock.get(key).and_then(|v| v.as_array()) {
            for pkg in packages {
                let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");

                if !name.is_empty() && !version.is_empty() {
                    // Filter by package names if specified
                    if args.packages.is_empty() || args.packages.iter().any(|p| name.contains(p)) {
                        // Filter by direct only if requested
                        if !args.direct || direct_deps.contains(name) {
                            installed.push((name.to_string(), version.to_string(), is_dev));
                        }
                    }
                }
            }
        }
    }

    if installed.is_empty() {
        info("No packages to check");
        return Ok(());
    }

    // Check for updates
    let spinner = Spinner::new(format!("Checking {} packages...", installed.len()));
    let repo = Repository::packagist()?;
    repo.init_packagist().await?;

    let mut updates: Vec<PackageUpdate> = Vec::new();

    for (name, current, is_dev) in &installed {
        let package_id = match libretto_core::PackageId::parse(name) {
            Some(id) => id,
            None => continue,
        };

        // Fetch latest version
        let latest = match repo.get_package(&package_id).await {
            Ok(versions) => {
                let versions: Vec<_> = versions.into_iter().collect();
                versions
                    .iter()
                    .filter(|v| {
                        let ver_str = v.version.to_string();
                        !ver_str.contains("dev")
                            && !ver_str.contains("alpha")
                            && !ver_str.contains("beta")
                    })
                    .map(|v| v.version.to_string())
                    .next()
                    .or_else(|| versions.first().map(|v| v.version.to_string()))
                    .unwrap_or_else(|| current.clone())
            }
            Err(_) => current.clone(),
        };

        let update_type = compare_versions(current, &latest);

        // Filter by update type
        if args.minor_only && update_type == UpdateType::Major {
            continue;
        }

        // Filter out up-to-date packages unless --all
        if !args.all && update_type == UpdateType::UpToDate {
            continue;
        }

        let constraint = constraints
            .get(name)
            .cloned()
            .unwrap_or_else(|| "*".to_string());

        updates.push(PackageUpdate {
            name: name.clone(),
            current: current.clone(),
            latest,
            constraint,
            is_direct: direct_deps.contains(name),
            is_dev: *is_dev,
            update_type,
        });
    }

    spinner.finish_and_clear();

    // Sort by name
    updates.sort_by(|a, b| a.name.cmp(&b.name));

    if updates.is_empty() {
        info("All packages are up to date!");
        return Ok(());
    }

    // Output results
    if args.format == "json" {
        return output_json(&updates);
    }

    output_text(&updates, args.all)?;

    // Summary
    let outdated_count = updates
        .iter()
        .filter(|u| u.update_type != UpdateType::UpToDate)
        .count();
    let major_count = updates
        .iter()
        .filter(|u| u.update_type == UpdateType::Major)
        .count();
    let minor_count = updates
        .iter()
        .filter(|u| u.update_type == UpdateType::Minor)
        .count();
    let patch_count = updates
        .iter()
        .filter(|u| u.update_type == UpdateType::Patch)
        .count();

    println!();
    if outdated_count > 0 {
        warning(&format!(
            "{} outdated package(s): {} major, {} minor, {} patch",
            outdated_count, major_count, minor_count, patch_count
        ));

        if args.strict {
            std::process::exit(1);
        }
    } else {
        info("All packages are up to date!");
    }

    Ok(())
}

fn compare_versions(current: &str, latest: &str) -> UpdateType {
    let current_clean = current.trim_start_matches('v');
    let latest_clean = latest.trim_start_matches('v');

    if current_clean == latest_clean {
        return UpdateType::UpToDate;
    }

    let current_parts: Vec<u64> = current_clean
        .split('.')
        .filter_map(|s| s.split('-').next()?.parse().ok())
        .collect();
    let latest_parts: Vec<u64> = latest_clean
        .split('.')
        .filter_map(|s| s.split('-').next()?.parse().ok())
        .collect();

    if current_parts.is_empty() || latest_parts.is_empty() {
        return UpdateType::Patch;
    }

    let current_major = current_parts.first().copied().unwrap_or(0);
    let latest_major = latest_parts.first().copied().unwrap_or(0);

    if latest_major > current_major {
        return UpdateType::Major;
    }

    let current_minor = current_parts.get(1).copied().unwrap_or(0);
    let latest_minor = latest_parts.get(1).copied().unwrap_or(0);

    if latest_minor > current_minor {
        return UpdateType::Minor;
    }

    UpdateType::Patch
}

fn output_text(updates: &[PackageUpdate], show_all: bool) -> Result<()> {
    use crate::output::table::Table;

    // Separate outdated from up-to-date
    let outdated: Vec<_> = updates
        .iter()
        .filter(|u| u.update_type != UpdateType::UpToDate)
        .collect();
    let up_to_date: Vec<_> = updates
        .iter()
        .filter(|u| u.update_type == UpdateType::UpToDate)
        .collect();

    if !outdated.is_empty() {
        println!("Outdated packages:");
        println!();

        let mut table = Table::new();
        table.headers(["Package", "Current", "Latest", "Type"]);

        for pkg in &outdated {
            let type_str = match pkg.update_type {
                UpdateType::Major => "major",
                UpdateType::Minor => "minor",
                UpdateType::Patch => "patch",
                UpdateType::UpToDate => "up-to-date",
            };

            let type_cell = match pkg.update_type {
                UpdateType::Major => table.error_cell(type_str),
                UpdateType::Minor => table.warning_cell(type_str),
                UpdateType::Patch => table.success_cell(type_str),
                UpdateType::UpToDate => comfy_table::Cell::new(type_str),
            };

            table.styled_row(vec![
                comfy_table::Cell::new(&pkg.name),
                comfy_table::Cell::new(&pkg.current),
                comfy_table::Cell::new(&pkg.latest),
                type_cell,
            ]);
        }

        table.print();
    }

    if show_all && !up_to_date.is_empty() {
        println!();
        println!("Up-to-date packages:");
        println!();

        let mut table = Table::new();
        table.headers(["Package", "Version"]);

        for pkg in &up_to_date {
            table.row([pkg.name.as_str(), pkg.current.as_str()]);
        }

        table.print();
    }

    Ok(())
}

fn output_json(updates: &[PackageUpdate]) -> Result<()> {
    let output: Vec<_> = updates
        .iter()
        .map(|pkg| {
            sonic_rs::json!({
                "name": pkg.name,
                "current": pkg.current,
                "latest": pkg.latest,
                "constraint": pkg.constraint,
                "direct": pkg.is_direct,
                "dev": pkg.is_dev,
                "update_type": match pkg.update_type {
                    UpdateType::Major => "major",
                    UpdateType::Minor => "minor",
                    UpdateType::Patch => "patch",
                    UpdateType::UpToDate => "up-to-date",
                }
            })
        })
        .collect();

    println!("{}", sonic_rs::to_string_pretty(&output)?);
    Ok(())
}
