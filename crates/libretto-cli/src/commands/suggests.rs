//! Suggests command - show package suggestions.

use anyhow::Result;
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::collections::HashMap;

/// Arguments for the suggests command
#[derive(Args, Debug, Clone)]
pub struct SuggestsArgs {
    /// Only show suggestions for specified packages
    #[arg(value_name = "PACKAGE")]
    pub packages: Vec<String>,

    /// Only show suggestions that are already installed
    #[arg(long)]
    pub installed: bool,

    /// Only show suggestions that are NOT installed
    #[arg(long)]
    pub uninstalled: bool,

    /// Show flat list without grouping
    #[arg(long)]
    pub flat: bool,

    /// Output format (text, json)
    #[arg(short = 'f', long, default_value = "text")]
    pub format: String,
}

/// Suggestion information
#[derive(Debug, Clone)]
struct Suggestion {
    package: String,
    suggested: String,
    reason: String,
    installed: bool,
}

/// Run the suggests command
pub async fn run(args: SuggestsArgs) -> Result<()> {
    use crate::output::{header, info};

    header("Package suggestions");

    let lock_path = std::env::current_dir()?.join("composer.lock");
    if !lock_path.exists() {
        anyhow::bail!("composer.lock not found - run 'libretto install' first");
    }

    let lock_content = std::fs::read_to_string(&lock_path)?;
    let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

    // Build set of installed packages
    let mut installed_packages: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for key in ["packages", "packages-dev"] {
        if let Some(packages) = lock.get(key).and_then(|v| v.as_array()) {
            for pkg in packages {
                if let Some(name) = pkg.get("name").and_then(|v| v.as_str()) {
                    installed_packages.insert(name.to_string());
                }
            }
        }
    }

    // Collect suggestions
    let mut suggestions: Vec<Suggestion> = Vec::new();

    for key in ["packages", "packages-dev"] {
        if let Some(packages) = lock.get(key).and_then(|v| v.as_array()) {
            for pkg in packages {
                let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");

                // Filter by specified packages
                if !args.packages.is_empty() && !args.packages.iter().any(|p| name.contains(p)) {
                    continue;
                }

                // Get suggestions
                if let Some(suggest) = pkg.get("suggest").and_then(|v| v.as_object()) {
                    for (suggested, reason) in suggest {
                        let reason_str = reason.as_str().unwrap_or("");
                        let suggested_str = suggested.to_string();
                        let is_installed = installed_packages.contains(&suggested_str);

                        // Filter by installed status
                        if args.installed && !is_installed {
                            continue;
                        }
                        if args.uninstalled && is_installed {
                            continue;
                        }

                        suggestions.push(Suggestion {
                            package: name.to_string(),
                            suggested: suggested_str,
                            reason: reason_str.to_string(),
                            installed: is_installed,
                        });
                    }
                }
            }
        }
    }

    if suggestions.is_empty() {
        info("No package suggestions found");
        return Ok(());
    }

    // Remove duplicates (same suggested package from different sources)
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    if args.flat {
        suggestions.retain(|s| seen.insert(s.suggested.clone()));
    }

    // Output
    if args.format == "json" {
        return output_json(&suggestions);
    }

    if args.flat {
        output_flat(&suggestions)?;
    } else {
        output_grouped(&suggestions)?;
    }

    // Summary
    println!();
    let installed_count = suggestions.iter().filter(|s| s.installed).count();
    let uninstalled_count = suggestions.len() - installed_count;
    info(&format!(
        "Total: {} suggestion(s) - {} installed, {} not installed",
        suggestions.len(),
        installed_count,
        uninstalled_count
    ));

    Ok(())
}

fn output_grouped(suggestions: &[Suggestion]) -> Result<()> {
    use owo_colors::OwoColorize;

    let colors = crate::output::colors_enabled();

    // Group by source package
    let mut grouped: HashMap<String, Vec<&Suggestion>> = HashMap::new();
    for suggestion in suggestions {
        grouped
            .entry(suggestion.package.clone())
            .or_default()
            .push(suggestion);
    }

    // Sort packages
    let mut packages: Vec<_> = grouped.keys().collect();
    packages.sort();

    for package in packages {
        let suggestions = &grouped[package];

        if colors {
            println!("{}", package.cyan().bold());
        } else {
            println!("{package}");
        }

        for suggestion in suggestions {
            let status = if suggestion.installed {
                if colors {
                    "[installed]".green().to_string()
                } else {
                    "[installed]".to_string()
                }
            } else if colors {
                "[not installed]".yellow().to_string()
            } else {
                "[not installed]".to_string()
            };

            let suggested = if colors {
                suggestion.suggested.green().to_string()
            } else {
                suggestion.suggested.clone()
            };

            if suggestion.reason.is_empty() {
                println!("  {suggested} {status}");
            } else {
                let reason = if colors {
                    suggestion.reason.dimmed().to_string()
                } else {
                    format!("({})", suggestion.reason)
                };
                println!("  {suggested} {status} - {reason}");
            }
        }
        println!();
    }

    Ok(())
}

fn output_flat(suggestions: &[Suggestion]) -> Result<()> {
    use crate::output::table::Table;

    let mut table = Table::new();
    table.headers(["Package", "Reason", "Status"]);

    for suggestion in suggestions {
        let status = if suggestion.installed {
            "installed"
        } else {
            "not installed"
        };

        let status_cell = if suggestion.installed {
            table.success_cell(status)
        } else {
            table.warning_cell(status)
        };

        table.styled_row(vec![
            comfy_table::Cell::new(&suggestion.suggested),
            comfy_table::Cell::new(&suggestion.reason),
            status_cell,
        ]);
    }

    table.print();
    Ok(())
}

fn output_json(suggestions: &[Suggestion]) -> Result<()> {
    let output: Vec<_> = suggestions
        .iter()
        .map(|s| {
            sonic_rs::json!({
                "source": s.package,
                "package": s.suggested,
                "reason": s.reason,
                "installed": s.installed
            })
        })
        .collect();

    println!("{}", sonic_rs::to_string_pretty(&output)?);
    Ok(())
}
