//! Remove command implementation.

use anyhow::Result;
use clap::Args;
use console::style;
use tracing::info;

/// Arguments for the remove command.
#[derive(Args, Debug)]
pub struct RemoveArgs {
    /// Packages to remove
    #[arg(required = true)]
    pub packages: Vec<String>,

    /// Remove from dev dependencies
    #[arg(long)]
    pub dev: bool,

    /// Don't update dependencies
    #[arg(long)]
    pub no_update: bool,

    /// Don't remove unused dependencies
    #[arg(long)]
    pub no_update_with_dependencies: bool,
}

/// Run the remove command.
pub async fn run(args: RemoveArgs) -> Result<()> {
    info!("running remove command");

    println!(
        "{} {}",
        style("Libretto").cyan().bold(),
        style("Removing packages...").dim()
    );

    for package in &args.packages {
        println!("{} {}", style("Removing").red(), style(package).bold());
    }

    println!(
        "{}",
        style("Remove command not yet fully implemented").yellow()
    );

    Ok(())
}
