//! Show command implementation.

use anyhow::Result;
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};

/// Arguments for the show command.
#[derive(Args, Debug, Clone)]
pub struct ShowArgs {
    /// Package name (vendor/name)
    #[arg(value_name = "PACKAGE")]
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

    /// Output as dependency tree
    #[arg(short, long)]
    pub tree: bool,
}

/// Run the show command.
pub async fn run(args: ShowArgs) -> Result<()> {
    use crate::output::table::{kv_table, Table};
    use crate::output::{header, info, warning};
    use libretto_core::PackageId;
    use libretto_repository::Repository;
    use owo_colors::OwoColorize;

    // If no package specified, show installed packages
    if args.package.is_none() || args.installed {
        return show_installed(&args).await;
    }

    let package_name = args.package.as_ref().unwrap();

    header(&format!("Package: {}", package_name));

    let package_id = PackageId::parse(package_name)
        .ok_or_else(|| anyhow::anyhow!("Invalid package name: {}", package_name))?;

    let repo = Repository::packagist()?;
    repo.init_packagist().await?;
    let colors = crate::output::colors_enabled();

    // Fetch package info
    let spinner = crate::output::progress::Spinner::new("Fetching package info...");

    match repo.get_package(&package_id).await {
        Ok(versions) => {
            spinner.finish_and_clear();

            let versions: Vec<_> = versions.into_iter().collect();
            if versions.is_empty() {
                warning("No versions found for this package");
                return Ok(());
            }

            // Show latest version info
            let latest = &versions[0];

            println!();
            if colors {
                println!("{}", package_name.green().bold());
            } else {
                println!("{}", package_name);
            }
            println!();

            // Basic info
            let mut info_table = kv_table([
                ("name", package_name.as_str()),
                ("version", &latest.version.to_string()),
            ]);

            if !latest.description.is_empty() {
                info_table.row(["description", &latest.description]);
            }

            if !latest.license.is_empty() {
                info_table.row(["license", &latest.license.join(", ")]);
            }

            // Show source/dist URLs if available
            if let Some(source) = &latest.source {
                match source {
                    libretto_core::PackageSource::Git { url, reference } => {
                        info_table.row(["source", &format!("{} ({})", url, reference)]);
                    }
                    libretto_core::PackageSource::Dist { url, .. } => {
                        info_table.row(["source", url.as_str()]);
                    }
                }
            }

            if let Some(dist) = &latest.dist {
                match dist {
                    libretto_core::PackageSource::Dist { url, .. } => {
                        info_table.row(["dist", url.as_str()]);
                    }
                    libretto_core::PackageSource::Git { url, .. } => {
                        info_table.row(["dist", url.as_str()]);
                    }
                }
            }

            info_table.print();

            // Show authors
            if !latest.authors.is_empty() {
                println!();
                info("Authors:");
                for author in &latest.authors {
                    let email = author.email.as_deref().unwrap_or("");
                    if colors {
                        println!(
                            "  {} {}",
                            author.name.cyan(),
                            format!("<{}>", email).dimmed()
                        );
                    } else {
                        println!("  {} <{}>", author.name, email);
                    }
                }
            }

            // Show dependencies
            if !latest.require.is_empty() {
                println!();
                info("Dependencies:");
                let mut dep_table = Table::new();
                dep_table.headers(["Package", "Constraint"]);

                for dep in &latest.require {
                    dep_table.row([dep.package.full_name(), dep.constraint.to_string()]);
                }

                dep_table.print();
            }

            // Show dev dependencies
            if !latest.require_dev.is_empty() && args.all {
                println!();
                info("Dev Dependencies:");
                let mut dep_table = Table::new();
                dep_table.headers(["Package", "Constraint"]);

                for dep in &latest.require_dev {
                    dep_table.row([dep.package.full_name(), dep.constraint.to_string()]);
                }

                dep_table.print();
            }

            // Show available versions
            if args.available || args.all {
                println!();
                info(&format!("Available versions ({}):", versions.len()));

                let display_count = if args.all {
                    versions.len()
                } else {
                    10.min(versions.len())
                };

                for version in versions.iter().take(display_count) {
                    let stability = if version.version.to_string().contains("dev") {
                        " (dev)"
                    } else if version.version.to_string().contains("alpha") {
                        " (alpha)"
                    } else if version.version.to_string().contains("beta") {
                        " (beta)"
                    } else if version.version.to_string().contains("RC") {
                        " (RC)"
                    } else {
                        ""
                    };

                    if colors {
                        println!(
                            "  {} {}",
                            version.version.to_string().yellow(),
                            stability.dimmed()
                        );
                    } else {
                        println!("  {} {}", version.version, stability);
                    }
                }

                if versions.len() > display_count {
                    println!("  ... and {} more", versions.len() - display_count);
                }
            }
        }
        Err(e) => {
            spinner.finish_and_clear();
            anyhow::bail!("Failed to fetch package info: {}", e);
        }
    }

    Ok(())
}

async fn show_installed(args: &ShowArgs) -> Result<()> {
    use crate::output::table::Table;
    use crate::output::{header, info, warning};

    header("Installed packages");

    let lock_path = std::env::current_dir()?.join("composer.lock");

    if !lock_path.exists() {
        warning("No composer.lock found. Run 'libretto install' first.");
        return Ok(());
    }

    let lock_content = std::fs::read_to_string(&lock_path)?;
    let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

    if args.tree {
        return show_tree(&lock).await;
    }

    // Collect packages
    let mut packages: Vec<(String, String, bool)> = Vec::new();

    if let Some(pkgs) = lock.get("packages").and_then(|v| v.as_array()) {
        for pkg in pkgs {
            let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");
            packages.push((name.to_string(), version.to_string(), false));
        }
    }

    if let Some(pkgs) = lock.get("packages-dev").and_then(|v| v.as_array()) {
        for pkg in pkgs {
            let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");
            packages.push((name.to_string(), version.to_string(), true));
        }
    }

    if packages.is_empty() {
        info("No packages installed");
        return Ok(());
    }

    // Sort by name
    packages.sort_by(|a, b| a.0.cmp(&b.0));

    // Filter by package name if specified
    if let Some(filter) = &args.package {
        packages.retain(|(name, _, _)| name.contains(filter));
    }

    // Display
    let mut table = Table::new();
    table.headers(["Package", "Version", "Type"]);

    for (name, version, is_dev) in &packages {
        let pkg_type = if *is_dev { "dev" } else { "prod" };

        let type_cell = if *is_dev {
            table.dim_cell(pkg_type)
        } else {
            comfy_table::Cell::new(pkg_type)
        };

        table.styled_row(vec![
            comfy_table::Cell::new(name),
            comfy_table::Cell::new(version),
            type_cell,
        ]);
    }

    table.print();

    println!();
    info(&format!("{} package(s) installed", packages.len()));

    Ok(())
}

async fn show_tree(lock: &sonic_rs::Value) -> Result<()> {
    use owo_colors::OwoColorize;
    use std::collections::HashMap;

    let colors = crate::output::colors_enabled();
    let unicode = crate::output::unicode_enabled();

    // Build dependency map
    let mut deps: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut versions: HashMap<String, String> = HashMap::new();

    for key in ["packages", "packages-dev"] {
        if let Some(packages) = lock.get(key).and_then(|v| v.as_array()) {
            for pkg in packages {
                let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");

                versions.insert(name.to_string(), version.to_string());

                if let Some(require) = pkg.get("require").and_then(|v| v.as_object()) {
                    let pkg_deps: Vec<(String, String)> = require
                        .iter()
                        .filter(|(n, _)| !n.starts_with("php") && !n.starts_with("ext-"))
                        .map(|(n, c)| (n.to_string(), c.as_str().unwrap_or("*").to_string()))
                        .collect();
                    deps.insert(name.to_string(), pkg_deps);
                }
            }
        }
    }

    // Get root dependencies
    let composer_path = std::env::current_dir()?.join("composer.json");
    let mut root_deps: Vec<String> = Vec::new();

    if composer_path.exists() {
        let composer_content = std::fs::read_to_string(&composer_path)?;
        let composer: sonic_rs::Value = sonic_rs::from_str(&composer_content)?;

        if let Some(require) = composer.get("require").and_then(|v| v.as_object()) {
            for (name, _) in require {
                if !name.starts_with("php") && !name.starts_with("ext-") {
                    root_deps.push(name.to_string());
                }
            }
        }

        if let Some(require_dev) = composer.get("require-dev").and_then(|v| v.as_object()) {
            for (name, _) in require_dev {
                if !name.starts_with("php") && !name.starts_with("ext-") {
                    root_deps.push(name.to_string());
                }
            }
        }
    }

    root_deps.sort();

    // Print tree
    fn print_tree_node(
        name: &str,
        versions: &HashMap<String, String>,
        deps: &HashMap<String, Vec<(String, String)>>,
        depth: usize,
        is_last: bool,
        prefix: &str,
        colors: bool,
        unicode: bool,
        visited: &mut Vec<String>,
    ) {
        let connector = if depth == 0 {
            ""
        } else if unicode {
            if is_last {
                "\u{2514}\u{2500}\u{2500} "
            } else {
                "\u{251C}\u{2500}\u{2500} "
            }
        } else if is_last {
            "`-- "
        } else {
            "|-- "
        };

        let version = versions
            .get(name)
            .cloned()
            .unwrap_or_else(|| "?".to_string());

        if colors {
            println!(
                "{}{}{}{}",
                prefix,
                connector,
                name.green(),
                format!(" {}", version).yellow()
            );
        } else {
            println!("{}{}{} {}", prefix, connector, name, version);
        }

        // Avoid cycles
        if visited.contains(&name.to_string()) {
            return;
        }
        visited.push(name.to_string());

        if let Some(pkg_deps) = deps.get(name) {
            let new_prefix = if depth == 0 {
                String::new()
            } else {
                format!(
                    "{}{}",
                    prefix,
                    if unicode {
                        if is_last {
                            "    "
                        } else {
                            "\u{2502}   "
                        }
                    } else if is_last {
                        "    "
                    } else {
                        "|   "
                    }
                )
            };

            for (i, (dep_name, _)) in pkg_deps.iter().enumerate() {
                let dep_is_last = i == pkg_deps.len() - 1;
                print_tree_node(
                    dep_name,
                    versions,
                    deps,
                    depth + 1,
                    dep_is_last,
                    &new_prefix,
                    colors,
                    unicode,
                    visited,
                );
            }
        }

        visited.pop();
    }

    for (i, root) in root_deps.iter().enumerate() {
        let is_last = i == root_deps.len() - 1;
        let mut visited = Vec::new();
        print_tree_node(
            root,
            &versions,
            &deps,
            0,
            is_last,
            "",
            colors,
            unicode,
            &mut visited,
        );
    }

    Ok(())
}
