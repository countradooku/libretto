//! Search command implementation.

use anyhow::Result;
use clap::Args;
use console::style;
use libretto_repository::Repository;
use tracing::info;

/// Arguments for the search command.
#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Search query
    pub query: String,

    /// Only show package names
    #[arg(short = 'N', long)]
    pub only_name: bool,

    /// Search type (library, project, etc.)
    #[arg(short = 't', long = "type")]
    pub package_type: Option<String>,
}

/// Run the search command.
pub async fn run(args: SearchArgs) -> Result<()> {
    info!(query = %args.query, "running search command");

    println!(
        "{} Searching for '{}'...",
        style("Libretto").cyan().bold(),
        style(&args.query).yellow()
    );

    let repo = Repository::packagist()?;

    match repo.search(&args.query).await {
        Ok(results) => {
            if results.is_empty() {
                println!("{}", style("No packages found").dim());
            } else {
                println!();
                for result in results.iter().take(15) {
                    if args.only_name {
                        println!("{}", result.name);
                    } else {
                        println!(
                            "{} {}",
                            style(&result.name).green().bold(),
                            style(format!("({} downloads)", result.downloads)).dim()
                        );
                        if !result.description.is_empty() {
                            println!("  {}", result.description);
                        }
                    }
                }
                if results.len() > 15 {
                    println!(
                        "\n{} {} more results...",
                        style("...and").dim(),
                        results.len() - 15
                    );
                }
            }
        }
        Err(e) => {
            println!("{} {}", style("Search failed:").red(), e);
        }
    }

    Ok(())
}
