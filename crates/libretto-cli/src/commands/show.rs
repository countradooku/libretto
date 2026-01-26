//! Show command implementation.

use anyhow::Result;
use clap::Args;
use console::style;
use libretto_core::{PackageId, VersionConstraint};
use libretto_repository::Repository;
use tracing::info;

/// Arguments for the show command.
#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Package name (vendor/name)
    pub package: Option<String>,

    /// Show installed packages
    #[arg(short, long)]
    pub installed: bool,

    /// Show available versions
    #[arg(long)]
    pub available: bool,

    /// Show all info
    #[arg(short, long)]
    pub all: bool,

    /// Output as tree
    #[arg(short, long)]
    pub tree: bool,
}

/// Run the show command.
pub async fn run(args: ShowArgs) -> Result<()> {
    info!("running show command");

    if args.installed {
        println!(
            "{} {}",
            style("Libretto").cyan().bold(),
            style("Installed packages:").dim()
        );
        println!("{}", style("Show installed not yet implemented").yellow());
        return Ok(());
    }

    let Some(package_name) = args.package else {
        println!("{}", style("No package specified").yellow());
        return Ok(());
    };

    println!(
        "{} Looking up {}...",
        style("Libretto").cyan().bold(),
        style(&package_name).yellow()
    );

    let Some(package_id) = PackageId::parse(&package_name) else {
        println!(
            "{} Invalid package name: {}",
            style("Error:").red(),
            package_name
        );
        return Ok(());
    };

    let repo = Repository::packagist()?;

    match repo
        .find_version(&package_id, &VersionConstraint::any())
        .await
    {
        Ok(package) => {
            println!();
            println!("{}", style(package.id.full_name()).green().bold());
            println!();
            println!("  {} {}", style("Version:").dim(), package.version);
            if !package.description.is_empty() {
                println!("  {} {}", style("Description:").dim(), package.description);
            }
            if !package.license.is_empty() {
                println!(
                    "  {} {}",
                    style("License:").dim(),
                    package.license.join(", ")
                );
            }
            if !package.require.is_empty() {
                println!();
                println!("  {}", style("Dependencies:").dim());
                for dep in &package.require {
                    println!("    {} {}", dep.package, dep.constraint);
                }
            }
        }
        Err(e) => {
            println!("{} {}", style("Failed to fetch package:").red(), e);
        }
    }

    Ok(())
}
