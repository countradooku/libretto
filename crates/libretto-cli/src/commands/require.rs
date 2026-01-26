//! Require command implementation.

use anyhow::Result;
use clap::Args;
use console::style;
use tracing::info;

/// Arguments for the require command.
#[derive(Args, Debug)]
pub struct RequireArgs {
    /// Packages to require (name or name:version)
    #[arg(required = true)]
    pub packages: Vec<String>,

    /// Add as dev dependency
    #[arg(long)]
    pub dev: bool,

    /// Don't update dependencies
    #[arg(long)]
    pub no_update: bool,

    /// Dry run
    #[arg(long)]
    pub dry_run: bool,

    /// Prefer stable versions
    #[arg(long)]
    pub prefer_stable: bool,

    /// Sort packages alphabetically
    #[arg(long)]
    pub sort_packages: bool,
}

/// Run the require command.
pub async fn run(args: RequireArgs) -> Result<()> {
    info!("running require command");

    println!(
        "{} {}",
        style("Libretto").cyan().bold(),
        style("Adding packages...").dim()
    );

    for package in &args.packages {
        let dep_type = if args.dev {
            "dev dependency"
        } else {
            "dependency"
        };
        println!(
            "{} {} as {}",
            style("Adding").green(),
            style(package).bold(),
            dep_type
        );
    }

    if args.dry_run {
        println!(
            "{}",
            style("Dry run mode - no changes will be made").yellow()
        );
    }

    println!(
        "{}",
        style("Require command not yet fully implemented").yellow()
    );

    Ok(())
}
