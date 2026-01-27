//! Search command implementation.

use anyhow::Result;
use clap::Args;

/// Arguments for the search command.
#[derive(Args, Debug, Clone)]
pub struct SearchArgs {
    /// Search query
    #[arg(required = true, value_name = "QUERY")]
    pub query: String,

    /// Only show package names
    #[arg(short = 'N', long)]
    pub only_name: bool,

    /// Filter by package type (library, project, etc.)
    #[arg(short = 't', long = "type")]
    pub package_type: Option<String>,

    /// Output format (text, json)
    #[arg(short = 'f', long, default_value = "text")]
    pub format: String,
}

/// Run the search command.
pub async fn run(args: SearchArgs) -> Result<()> {
    use crate::output::progress::Spinner;
    use crate::output::{header, info, warning};
    use libretto_repository::Repository;

    header(&format!("Searching for '{}'", args.query));

    let spinner = Spinner::new("Searching Packagist...");

    let repo = Repository::packagist()?;
    repo.init_packagist().await?;

    match repo.search(&args.query).await {
        Ok(results) => {
            spinner.finish_and_clear();

            if results.is_empty() {
                warning(&format!("No packages found matching '{}'", args.query));
                return Ok(());
            }

            // Filter by type if specified (not supported yet, so just pass through)
            let filtered: Vec<_> = results;

            if filtered.is_empty() {
                warning(&format!("No packages found matching '{}'", args.query));
                return Ok(());
            }

            // Output format
            if args.format == "json" {
                return output_json(&filtered);
            }

            if args.only_name {
                return output_names_only(&filtered);
            }

            output_full(&filtered)?;

            println!();
            info(&format!("Found {} package(s)", filtered.len()));
        }
        Err(e) => {
            spinner.finish_and_clear();
            anyhow::bail!("Search failed: {}", e);
        }
    }

    Ok(())
}

fn output_full(results: &[libretto_repository::PackageSearchResult]) -> Result<()> {
    use crate::output::table::Table;
    use owo_colors::OwoColorize;

    let colors = crate::output::colors_enabled();

    let mut table = Table::new();
    table.headers(["Package", "Description", "Downloads"]);

    // Limit to 25 results for display
    let display_count = 25.min(results.len());

    for result in results.iter().take(display_count) {
        let downloads = format_downloads(result.downloads);
        let description = truncate(&result.description, 60);

        table.row([result.name.as_str(), &description, &downloads]);
    }

    table.print();

    if results.len() > display_count {
        println!();
        if colors {
            println!(
                "  {} {} more results...",
                "...".dimmed(),
                results.len() - display_count
            );
        } else {
            println!("  ... {} more results...", results.len() - display_count);
        }
    }

    Ok(())
}

fn output_names_only(results: &[libretto_repository::PackageSearchResult]) -> Result<()> {
    for result in results {
        println!("{}", result.name);
    }
    Ok(())
}

fn output_json(results: &[libretto_repository::PackageSearchResult]) -> Result<()> {
    let output: Vec<_> = results
        .iter()
        .map(|r| {
            sonic_rs::json!({
                "name": r.name,
                "description": r.description,
                "downloads": r.downloads
            })
        })
        .collect();

    println!("{}", sonic_rs::to_string_pretty(&output)?);
    Ok(())
}

fn format_downloads(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
