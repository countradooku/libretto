//! Bump command - update version constraints to match installed versions.

use anyhow::Result;
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueMutTrait, JsonValueTrait};
use std::collections::HashMap;

/// Arguments for the bump command
#[derive(Args, Debug, Clone)]
pub struct BumpArgs {
    /// Only bump packages matching the given patterns
    #[arg(value_name = "PACKAGE")]
    pub packages: Vec<String>,

    /// Only bump dev dependencies
    #[arg(short = 'D', long)]
    pub dev_only: bool,

    /// Only bump non-dev dependencies
    #[arg(short = 'R', long)]
    pub no_dev_only: bool,

    /// Only show what would be changed, don't modify composer.json
    #[arg(long)]
    pub dry_run: bool,
}

/// Run the bump command
pub async fn run(args: BumpArgs) -> Result<()> {
    use crate::output::{header, info, success, warning};

    header("Bumping version constraints");

    let composer_path = std::env::current_dir()?.join("composer.json");
    let lock_path = std::env::current_dir()?.join("composer.lock");

    if !composer_path.exists() {
        anyhow::bail!("composer.json not found in current directory");
    }

    if !lock_path.exists() {
        anyhow::bail!("composer.lock not found - run 'libretto install' first");
    }

    // Read composer.json
    let composer_content = std::fs::read_to_string(&composer_path)?;
    let mut composer: sonic_rs::Value = sonic_rs::from_str(&composer_content)?;

    // Read composer.lock
    let lock_content = std::fs::read_to_string(&lock_path)?;
    let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

    // Build map of installed versions
    let mut installed: HashMap<String, String> = HashMap::new();

    if let Some(packages) = lock.get("packages").and_then(|v| v.as_array()) {
        for pkg in packages {
            if let (Some(name), Some(version)) = (
                pkg.get("name").and_then(|v| v.as_str()),
                pkg.get("version").and_then(|v| v.as_str()),
            ) {
                installed.insert(name.to_string(), version.to_string());
            }
        }
    }

    if let Some(packages) = lock.get("packages-dev").and_then(|v| v.as_array()) {
        for pkg in packages {
            if let (Some(name), Some(version)) = (
                pkg.get("name").and_then(|v| v.as_str()),
                pkg.get("version").and_then(|v| v.as_str()),
            ) {
                installed.insert(name.to_string(), version.to_string());
            }
        }
    }

    let mut changes: Vec<(String, String, String)> = Vec::new();

    // Process require section
    if !args.dev_only {
        if let Some(require) = composer.get_mut("require").and_then(|v| v.as_object_mut()) {
            for (name, constraint) in require.iter_mut() {
                if should_process(name, &args.packages) {
                    if let Some(version) = installed.get(name) {
                        let new_constraint =
                            bump_constraint(constraint.as_str().unwrap_or("*"), version);
                        let old = constraint.as_str().unwrap_or("*").to_string();
                        if old != new_constraint {
                            changes.push((name.to_string(), old, new_constraint.clone()));
                            *constraint = sonic_rs::json!(new_constraint);
                        }
                    }
                }
            }
        }
    }

    // Process require-dev section
    if !args.no_dev_only {
        if let Some(require_dev) = composer
            .get_mut("require-dev")
            .and_then(|v| v.as_object_mut())
        {
            for (name, constraint) in require_dev.iter_mut() {
                if should_process(name, &args.packages) {
                    if let Some(version) = installed.get(name) {
                        let new_constraint =
                            bump_constraint(constraint.as_str().unwrap_or("*"), version);
                        let old = constraint.as_str().unwrap_or("*").to_string();
                        if old != new_constraint {
                            changes.push((name.to_string(), old, new_constraint.clone()));
                            *constraint = sonic_rs::json!(new_constraint);
                        }
                    }
                }
            }
        }
    }

    if changes.is_empty() {
        info("No version constraints need to be bumped");
        return Ok(());
    }

    // Display changes
    info(&format!("Found {} constraint(s) to bump:", changes.len()));
    println!();

    for (name, old, new) in &changes {
        use owo_colors::OwoColorize;
        if crate::output::colors_enabled() {
            println!(
                "  {} {} {} {}",
                name.green(),
                old.red(),
                "->".dimmed(),
                new.green()
            );
        } else {
            println!("  {name} {old} -> {new}");
        }
    }
    println!();

    if args.dry_run {
        warning("Dry run - no changes written");
        return Ok(());
    }

    // Write updated composer.json
    let output = sonic_rs::to_string_pretty(&composer)?;
    std::fs::write(&composer_path, output)?;

    success(&format!(
        "Updated {} constraint(s) in composer.json",
        changes.len()
    ));

    Ok(())
}

fn should_process(name: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    filters.iter().any(|f| {
        if f.contains('*') {
            let pattern = f.replace('*', ".*");
            regex::Regex::new(&pattern)
                .map(|r| r.is_match(name))
                .unwrap_or(false)
        } else {
            name == f
        }
    })
}

fn bump_constraint(old: &str, version: &str) -> String {
    // Strip dev- prefix and v prefix
    let clean_version = version
        .strip_prefix("dev-")
        .or_else(|| version.strip_prefix('v'))
        .unwrap_or(version);

    // Parse version
    let parts: Vec<&str> = clean_version.split('.').collect();

    // Determine constraint style from old constraint
    if old.starts_with('^') || old.starts_with('~') || old.starts_with(">=") {
        // Use caret constraint with major.minor
        if parts.len() >= 2 {
            format!("^{}.{}", parts[0], parts[1])
        } else {
            format!("^{clean_version}")
        }
    } else if old.contains("||") || old.contains(" || ") {
        // Keep OR constraints, just update the last one
        let parts: Vec<&str> = old.split("||").collect();
        if let Some(_last) = parts.last() {
            let new_last = format!("^{clean_version}");
            let mut result: Vec<String> = parts[..parts.len() - 1]
                .iter()
                .map(|s| s.trim().to_string())
                .collect();
            result.push(new_last);
            result.join(" || ")
        } else {
            format!("^{clean_version}")
        }
    } else {
        // Default to caret
        format!("^{clean_version}")
    }
}
