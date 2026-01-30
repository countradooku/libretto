//! Fund command - show funding information for dependencies.

use anyhow::Result;
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::collections::HashMap;

/// Arguments for the fund command
#[derive(Args, Debug, Clone)]
pub struct FundArgs {
    /// Output format (text, json)
    #[arg(short = 'f', long, default_value = "text")]
    pub format: String,
}

/// Funding information
#[derive(Debug, Clone)]
struct FundingInfo {
    package: String,
    funding_type: String,
    url: String,
}

/// Run the fund command
pub fn run(args: FundArgs) -> Result<()> {
    use crate::output::{header, info};
    use owo_colors::OwoColorize;

    header("Funding information");

    let lock_path = std::env::current_dir()?.join("composer.lock");
    if !lock_path.exists() {
        anyhow::bail!("composer.lock not found - run 'libretto install' first");
    }

    let lock_content = std::fs::read_to_string(&lock_path)?;
    let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

    let mut funding_info: Vec<FundingInfo> = Vec::new();

    // Collect funding info from all packages
    for packages_key in ["packages", "packages-dev"] {
        if let Some(packages) = lock.get(packages_key).and_then(|v| v.as_array()) {
            for pkg in packages {
                let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");

                if let Some(funding) = pkg.get("funding")
                    && let Some(funding_array) = funding.as_array()
                {
                    for fund in funding_array {
                        let fund_type =
                            fund.get("type").and_then(|v| v.as_str()).unwrap_or("other");
                        let url = fund.get("url").and_then(|v| v.as_str()).unwrap_or("");

                        if !url.is_empty() {
                            funding_info.push(FundingInfo {
                                package: name.to_string(),
                                funding_type: fund_type.to_string(),
                                url: url.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    if funding_info.is_empty() {
        info("No funding information found in your dependencies");
        return Ok(());
    }

    // Sort by package name
    funding_info.sort_by(|a, b| a.package.cmp(&b.package));

    if args.format == "json" {
        let output: Vec<_> = funding_info
            .iter()
            .map(|f| {
                sonic_rs::json!({
                    "package": f.package,
                    "type": f.funding_type,
                    "url": f.url
                })
            })
            .collect();
        println!("{}", sonic_rs::to_string_pretty(&output)?);
        return Ok(());
    }

    // Group by package
    let mut grouped: HashMap<String, Vec<&FundingInfo>> = HashMap::new();
    for fund in &funding_info {
        grouped.entry(fund.package.clone()).or_default().push(fund);
    }

    let colors = crate::output::colors_enabled();

    println!(
        "Found {} package(s) with funding information:",
        grouped.len()
    );
    println!();

    for (package, funds) in &grouped {
        if colors {
            println!("  {}", package.green().bold());
        } else {
            println!("  {package}");
        }

        for fund in funds {
            let type_display = format_funding_type(&fund.funding_type);
            if colors {
                println!(
                    "    {} {}",
                    type_display.cyan(),
                    fund.url.blue().underline()
                );
            } else {
                println!("    {} {}", type_display, fund.url);
            }
        }
        println!();
    }

    // Summary
    let type_counts = count_funding_types(&funding_info);
    info("Funding platforms used:");
    for (platform, count) in &type_counts {
        println!(
            "  - {}: {} package(s)",
            format_funding_type(platform),
            count
        );
    }

    Ok(())
}

fn format_funding_type(t: &str) -> &str {
    match t.to_lowercase().as_str() {
        "github" => "GitHub Sponsors",
        "patreon" => "Patreon",
        "tidelift" => "Tidelift",
        "opencollective" | "open_collective" => "Open Collective",
        "ko-fi" | "ko_fi" => "Ko-fi",
        "liberapay" => "Liberapay",
        "issuehunt" => "IssueHunt",
        "community_bridge" => "Community Bridge",
        "polar" => "Polar",
        "custom" | "other" => "Custom",
        _ => t,
    }
}

fn count_funding_types(funds: &[FundingInfo]) -> Vec<(String, usize)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for fund in funds {
        *counts.entry(fund.funding_type.clone()).or_default() += 1;
    }

    let mut result: Vec<_> = counts.into_iter().collect();
    result.sort_by(|a, b| b.1.cmp(&a.1));
    result
}
