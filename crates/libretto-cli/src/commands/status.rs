//! Status command - show locally modified packages.

use anyhow::Result;
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};

/// Arguments for the status command
#[derive(Args, Debug, Clone)]
pub struct StatusArgs {
    /// Show verbose diff output
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Run the status command
pub fn run(args: StatusArgs) -> Result<()> {
    use crate::output::table::Table;
    use crate::output::{header, info, warning};
    use owo_colors::OwoColorize;

    header("Checking local modifications");

    let vendor_dir = std::env::current_dir()?.join("vendor");
    let lock_path = std::env::current_dir()?.join("composer.lock");

    if !vendor_dir.exists() {
        anyhow::bail!("vendor directory not found - run 'libretto install' first");
    }

    if !lock_path.exists() {
        anyhow::bail!("composer.lock not found - run 'libretto install' first");
    }

    let lock_content = std::fs::read_to_string(&lock_path)?;
    let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

    let mut modified_packages: Vec<(String, String, Vec<String>)> = Vec::new();
    let colors = crate::output::colors_enabled();

    // Check each package
    for key in ["packages", "packages-dev"] {
        if let Some(packages) = lock.get(key).and_then(|v| v.as_array()) {
            for pkg in packages {
                let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");

                let pkg_dir = vendor_dir.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
                if !pkg_dir.exists() {
                    continue;
                }

                // Check if it's a git repo (VCS install)
                let git_dir = pkg_dir.join(".git");
                if git_dir.exists() {
                    let modifications = check_git_modifications(&pkg_dir)?;
                    if !modifications.is_empty() {
                        modified_packages.push((
                            name.to_string(),
                            version.to_string(),
                            modifications,
                        ));
                    }
                }
            }
        }
    }

    if modified_packages.is_empty() {
        info("No local modifications detected");
        return Ok(());
    }

    warning(&format!(
        "Found {} package(s) with local modifications:",
        modified_packages.len()
    ));
    println!();

    for (name, version, modifications) in &modified_packages {
        if colors {
            println!("{} ({})", name.yellow().bold(), version);
        } else {
            println!("{name} ({version})");
        }

        if args.verbose {
            for modification in modifications {
                println!("  {modification}");
            }
        } else {
            println!("  {} modified file(s)", modifications.len());
        }
        println!();
    }

    // Summary table
    let mut table = Table::new();
    table.headers(["Package", "Version", "Modified Files"]);

    for (name, version, modifications) in &modified_packages {
        table.row([
            name.as_str(),
            version.as_str(),
            &modifications.len().to_string(),
        ]);
    }

    table.print();

    println!();
    warning("Local modifications may be lost when updating packages!");
    info("Use 'libretto update --prefer-source' to use VCS for packages");

    Ok(())
}

fn check_git_modifications(dir: &std::path::PathBuf) -> Result<Vec<String>> {
    use std::process::Command;

    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let modifications: Vec<String> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let status = &l[0..2];
            let file = l[3..].trim();
            format!("{status} {file}")
        })
        .collect();

    Ok(modifications)
}
