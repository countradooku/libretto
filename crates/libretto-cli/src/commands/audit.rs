//! Audit command implementation.

use anyhow::Result;
use clap::Args;
use console::style;
use libretto_audit::Auditor;
use libretto_core::PackageId;
use libretto_repository::Repository;
use semver::Version;
use serde::Deserialize;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::collections::HashMap;
use tracing::info;

/// Arguments for the audit command.
#[derive(Args, Debug, Clone)]
pub struct AuditArgs {
    /// Output format (table, json)
    #[arg(short, long, default_value = "table")]
    pub format: String,

    /// Don't fail on vulnerabilities
    #[arg(long)]
    pub no_fail: bool,

    /// Only show abandoned packages
    #[arg(long)]
    pub abandoned: bool,

    /// Only audit packages from composer.lock (don't resolve from composer.json)
    #[arg(long)]
    pub locked: bool,

    /// Suggest safe versions for vulnerable packages
    #[arg(long)]
    pub suggest_versions: bool,
}

#[derive(Debug, Deserialize)]
struct LockFile {
    #[serde(default)]
    packages: Vec<LockPackage>,
    #[serde(default, rename = "packages-dev")]
    packages_dev: Vec<LockPackage>,
}

#[derive(Debug, Deserialize)]
struct LockPackage {
    name: String,
    version: String,
}

/// Run the audit command.
pub async fn run(args: AuditArgs) -> Result<()> {
    info!("running audit command");

    println!(
        "{} {}",
        style("Libretto").cyan().bold(),
        style("Checking for security vulnerabilities...").dim()
    );

    let lock_path = std::path::Path::new("composer.lock");
    if !lock_path.exists() {
        println!(
            "{} composer.lock not found - run install first",
            style("Error:").red().bold()
        );
        return Ok(());
    }

    // Parse lock file to get packages
    let content = std::fs::read_to_string(lock_path)?;
    let lock: LockFile = sonic_rs::from_str(&content)?;

    let mut packages_to_audit: Vec<(PackageId, Version)> = Vec::new();
    let mut package_constraints: HashMap<String, String> = HashMap::new();

    // Load constraints from composer.json if not in locked mode
    if !args.locked {
        let composer_path = std::path::Path::new("composer.json");
        if composer_path.exists() {
            let composer_content = std::fs::read_to_string(composer_path)?;
            let composer: sonic_rs::Value = sonic_rs::from_str(&composer_content)?;

            if let Some(require) = composer.get("require").and_then(|v| v.as_object()) {
                for (name, constraint) in require {
                    if let Some(c) = constraint.as_str() {
                        package_constraints.insert(name.to_string(), c.to_string());
                    }
                }
            }

            if let Some(require_dev) = composer.get("require-dev").and_then(|v| v.as_object()) {
                for (name, constraint) in require_dev {
                    if let Some(c) = constraint.as_str() {
                        package_constraints.insert(name.to_string(), c.to_string());
                    }
                }
            }
        }
    }

    // Collect packages from lock file
    for pkg in &lock.packages {
        if let Some(id) = PackageId::parse(&pkg.name) {
            let version_str = pkg.version.trim_start_matches('v');
            if let Ok(ver) = Version::parse(version_str) {
                packages_to_audit.push((id, ver));
            }
        }
    }

    // Include dev packages
    for pkg in &lock.packages_dev {
        if let Some(id) = PackageId::parse(&pkg.name) {
            let version_str = pkg.version.trim_start_matches('v');
            if let Ok(ver) = Version::parse(version_str) {
                packages_to_audit.push((id, ver));
            }
        }
    }

    println!(
        "{}",
        style(format!("Auditing {} packages...", packages_to_audit.len())).dim()
    );

    let auditor = Auditor::new()?;
    let report = auditor.audit(&packages_to_audit).await?;

    println!();

    if report.vulnerability_count() == 0 {
        println!(
            "{} No security vulnerabilities found",
            style("Success:").green().bold()
        );
        return Ok(());
    }

    println!(
        "{} Found {} vulnerabilities in {} packages",
        style("Warning:").yellow().bold(),
        report.vulnerability_count(),
        report.vulnerable_package_count()
    );
    println!();

    // Initialize repository for version suggestions if needed
    let repo = if args.suggest_versions {
        match Repository::packagist() {
            Ok(r) => {
                let _ = r.init_packagist().await;
                Some(r)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    for (severity, vulns) in report.by_severity() {
        let color = severity.color();
        let reset = "\x1b[0m";

        for vuln in vulns {
            println!(
                "  {color}[{}]{reset} {} ({})",
                severity, vuln.advisory_id, vuln.package
            );
            println!("    {}", vuln.title);

            if let Some(ref fixed) = vuln.fixed_version {
                println!("    Fixed in: {fixed}");
            }

            // Suggest safe version if requested
            if args.suggest_versions
                && let Some(ref r) = repo
                && let Some(constraint) = package_constraints.get(&vuln.package.to_string())
            {
                let version_constraint = libretto_core::VersionConstraint::new(constraint);
                if let Ok(pkg) = r.find_version(&vuln.package, &version_constraint).await {
                    let suggested = pkg.version.to_string();
                    // Only suggest if different from fixed version
                    if vuln
                        .fixed_version
                        .as_ref()
                        .map(std::string::ToString::to_string)
                        != Some(suggested.clone())
                    {
                        println!("    {}", style(format!("Suggested: {suggested}")).green());
                    }
                }
            }
        }
    }

    if report.has_critical() && !args.no_fail {
        println!();
        println!(
            "{} Critical vulnerabilities found",
            style("Error:").red().bold()
        );
        std::process::exit(1);
    }

    if !report.passes() && !args.no_fail {
        std::process::exit(1);
    }

    Ok(())
}
