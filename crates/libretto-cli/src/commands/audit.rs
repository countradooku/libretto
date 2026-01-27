//! Audit command implementation.

use anyhow::Result;
use clap::Args;
use console::style;
use libretto_audit::Auditor;
use libretto_core::PackageId;
use semver::Version;
use serde::Deserialize;
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
}

#[derive(Debug, Deserialize)]
struct LockFile {
    #[serde(default)]
    packages: Vec<LockPackage>,
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

    for pkg in &lock.packages {
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
    } else {
        println!(
            "{} Found {} vulnerabilities in {} packages",
            style("Warning:").yellow().bold(),
            report.vulnerability_count(),
            report.vulnerable_package_count()
        );
        println!();

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
            }
        }

        if report.has_critical() && !args.no_fail {
            println!();
            println!(
                "{} Critical vulnerabilities found",
                style("Error:").red().bold()
            );
        }
    }

    Ok(())
}
