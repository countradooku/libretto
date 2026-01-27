//! Depends command - show what depends on a package.

use anyhow::Result;
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::collections::HashMap;

/// Arguments for the depends command
#[derive(Args, Debug, Clone)]
pub struct DependsArgs {
    /// Package to check
    #[arg(required = true, value_name = "PACKAGE")]
    pub package: String,

    /// Recursively resolve up to the root packages
    #[arg(short = 'r', long)]
    pub recursive: bool,

    /// Show tree view
    #[arg(short = 't', long)]
    pub tree: bool,
}

/// Run the depends command
pub async fn run(args: DependsArgs) -> Result<()> {
    use crate::output::table::Table;
    use crate::output::{header, info, warning};

    header("Dependency analysis");

    let lock_path = std::env::current_dir()?.join("composer.lock");
    if !lock_path.exists() {
        anyhow::bail!("composer.lock not found - run 'libretto install' first");
    }

    let lock_content = std::fs::read_to_string(&lock_path)?;
    let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

    // Build dependency graph
    let mut dependents: HashMap<String, Vec<(String, String, bool)>> = HashMap::new();

    // Process packages
    for (packages_key, is_dev) in [("packages", false), ("packages-dev", true)] {
        if let Some(packages) = lock.get(packages_key).and_then(|v| v.as_array()) {
            for pkg in packages {
                let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");

                // Check require
                if let Some(require) = pkg.get("require").and_then(|v| v.as_object()) {
                    for (dep_name, constraint) in require {
                        let constraint_str = constraint.as_str().unwrap_or("*");
                        dependents.entry(dep_name.to_string()).or_default().push((
                            name.to_string(),
                            constraint_str.to_string(),
                            is_dev,
                        ));
                    }
                }

                // Check require-dev
                if let Some(require_dev) = pkg.get("require-dev").and_then(|v| v.as_object()) {
                    for (dep_name, constraint) in require_dev {
                        let constraint_str = constraint.as_str().unwrap_or("*");
                        dependents.entry(dep_name.to_string()).or_default().push((
                            name.to_string(),
                            constraint_str.to_string(),
                            true,
                        ));
                    }
                }
            }
        }
    }

    // Also check root composer.json
    let composer_path = std::env::current_dir()?.join("composer.json");
    if composer_path.exists() {
        let composer_content = std::fs::read_to_string(&composer_path)?;
        let composer: sonic_rs::Value = sonic_rs::from_str(&composer_content)?;
        let root_name = composer
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("__root__");

        if let Some(require) = composer.get("require").and_then(|v| v.as_object()) {
            for (dep_name, constraint) in require {
                let constraint_str = constraint.as_str().unwrap_or("*");
                dependents.entry(dep_name.to_string()).or_default().push((
                    root_name.to_string(),
                    constraint_str.to_string(),
                    false,
                ));
            }
        }

        if let Some(require_dev) = composer.get("require-dev").and_then(|v| v.as_object()) {
            for (dep_name, constraint) in require_dev {
                let constraint_str = constraint.as_str().unwrap_or("*");
                dependents.entry(dep_name.to_string()).or_default().push((
                    root_name.to_string(),
                    constraint_str.to_string(),
                    true,
                ));
            }
        }
    }

    // Find packages that depend on the target
    let target = args.package.to_lowercase();
    let direct_dependents = dependents.get(&target);

    if direct_dependents.is_none() || direct_dependents.map(|d| d.is_empty()).unwrap_or(true) {
        warning(&format!("No packages depend on '{}'", args.package));
        return Ok(());
    }

    let direct = direct_dependents.unwrap();
    info(&format!(
        "Found {} package(s) that depend on '{}':",
        direct.len(),
        args.package
    ));
    println!();

    if args.tree {
        // Tree view
        print_dependency_tree(&args.package, &dependents, 0, args.recursive, &mut vec![]);
    } else {
        // Table view
        let mut table = Table::new();
        table.headers(["Package", "Constraint", "Type"]);

        for (name, constraint, is_dev) in direct {
            let pkg_type = if *is_dev { "dev" } else { "prod" };
            table.row([name.as_str(), constraint.as_str(), pkg_type]);
        }

        table.print();

        if args.recursive {
            // Show recursive dependents
            println!();
            info("Recursive dependents:");
            let mut visited = vec![args.package.clone()];
            print_recursive_dependents(&direct, &dependents, 1, &mut visited);
        }
    }

    Ok(())
}

fn print_dependency_tree(
    package: &str,
    dependents: &HashMap<String, Vec<(String, String, bool)>>,
    depth: usize,
    recursive: bool,
    visited: &mut Vec<String>,
) {
    use owo_colors::OwoColorize;

    let colors = crate::output::colors_enabled();
    let unicode = crate::output::unicode_enabled();

    if let Some(deps) = dependents.get(package) {
        for (i, (name, constraint, is_dev)) in deps.iter().enumerate() {
            let is_last = i == deps.len() - 1;
            let prefix = if depth == 0 {
                String::new()
            } else {
                let connector = if unicode {
                    if is_last {
                        "\u{2514}\u{2500}\u{2500}"
                    } else {
                        "\u{251C}\u{2500}\u{2500}"
                    }
                } else {
                    if is_last {
                        "`--"
                    } else {
                        "|--"
                    }
                };
                format!("{}{} ", "  ".repeat(depth - 1), connector)
            };

            let dev_marker = if *is_dev { " (dev)" } else { "" };

            if colors {
                println!(
                    "{}{} {} {}",
                    prefix,
                    name.green(),
                    constraint.yellow(),
                    dev_marker.dimmed()
                );
            } else {
                println!("{}{} {} {}", prefix, name, constraint, dev_marker);
            }

            if recursive && !visited.contains(name) {
                visited.push(name.clone());
                print_dependency_tree(name, dependents, depth + 1, recursive, visited);
            }
        }
    }
}

fn print_recursive_dependents(
    packages: &[(String, String, bool)],
    dependents: &HashMap<String, Vec<(String, String, bool)>>,
    depth: usize,
    visited: &mut Vec<String>,
) {
    use owo_colors::OwoColorize;

    let colors = crate::output::colors_enabled();
    let indent = "  ".repeat(depth);

    for (name, _, _) in packages {
        if visited.contains(name) {
            continue;
        }
        visited.push(name.clone());

        if let Some(deps) = dependents.get(name) {
            if !deps.is_empty() {
                if colors {
                    println!("{}{} is required by:", indent, name.cyan());
                } else {
                    println!("{}{} is required by:", indent, name);
                }
                for (dep_name, constraint, is_dev) in deps {
                    let dev = if *is_dev { " (dev)" } else { "" };
                    println!("{}  - {} {}{}", indent, dep_name, constraint, dev);
                }
                print_recursive_dependents(deps, dependents, depth + 1, visited);
            }
        }
    }
}
