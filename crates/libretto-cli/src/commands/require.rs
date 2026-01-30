//! Require command implementation.

use anyhow::Result;
use clap::Args;
use sonic_rs::JsonValueMutTrait;

/// Arguments for the require command.
#[derive(Args, Debug, Clone)]
pub struct RequireArgs {
    /// Packages to require (name or name:version)
    #[arg(required = true, value_name = "PACKAGE")]
    pub packages: Vec<String>,

    /// Add as dev dependency
    #[arg(short = 'D', long)]
    pub dev: bool,

    /// Don't update dependencies after adding
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
    use crate::output::progress::Spinner;
    use crate::output::{error, header, info, success, warning};
    use libretto_repository::Repository;
    use owo_colors::OwoColorize;

    header("Adding packages");

    let cwd = std::env::current_dir()?;
    let composer_path = cwd.join("composer.json");

    if !composer_path.exists() {
        anyhow::bail!("composer.json not found. Run 'libretto init' to create one.");
    }

    // Read current composer.json
    let composer_content = std::fs::read_to_string(&composer_path)?;
    let mut composer: sonic_rs::Value = sonic_rs::from_str(&composer_content)?;

    if args.dry_run {
        warning("Dry run mode - no changes will be made");
    }

    let dep_type = if args.dev { "require-dev" } else { "require" };
    let dep_label = if args.dev {
        "dev dependency"
    } else {
        "dependency"
    };

    // Parse packages and resolve versions
    let repo = Repository::packagist()?;
    repo.init_packagist().await?;
    let mut resolved: Vec<(String, String)> = Vec::new();

    for package_spec in &args.packages {
        // Parse name:constraint format
        let (name, constraint) = if let Some(idx) = package_spec.find(':') {
            let (n, c) = package_spec.split_at(idx);
            (n.to_string(), c[1..].to_string())
        } else {
            (package_spec.clone(), String::new())
        };

        let spinner = Spinner::new(format!("Looking up {name}..."));

        // Validate package exists
        let package_id = libretto_core::PackageId::parse(&name)
            .ok_or_else(|| anyhow::anyhow!("Invalid package name: {name}"))?;

        // Find best version
        let version_constraint = if constraint.is_empty() {
            libretto_core::VersionConstraint::any()
        } else {
            libretto_core::VersionConstraint::new(&constraint)
        };

        match repo.find_version(&package_id, &version_constraint).await {
            Ok(pkg) => {
                spinner.finish_and_clear();

                // Determine constraint to save
                let save_constraint = if constraint.is_empty() {
                    // Auto-generate caret constraint from resolved version
                    let version_str = pkg.version.to_string();
                    let parts: Vec<&str> = version_str.split('.').collect();
                    if parts.len() >= 2 {
                        format!("^{}.{}", parts[0], parts[1])
                    } else {
                        format!("^{version_str}")
                    }
                } else {
                    constraint.clone()
                };

                let colors = crate::output::colors_enabled();
                if colors {
                    println!(
                        "  {} {} {} as {}",
                        "+".green(),
                        name.green(),
                        save_constraint.yellow(),
                        dep_label
                    );
                } else {
                    println!("  + {name} {save_constraint} as {dep_label}");
                }

                resolved.push((name, save_constraint));
            }
            Err(e) => {
                spinner.finish_and_clear();
                error(&format!("Package '{name}' not found: {e}"));
                if !args.dry_run {
                    anyhow::bail!("Failed to resolve package: {name}");
                }
            }
        }
    }

    if resolved.is_empty() {
        warning("No packages to add");
        return Ok(());
    }

    if args.dry_run {
        println!();
        warning("Dry run - no changes made");
        return Ok(());
    }

    // Update composer.json
    let deps = composer.get_mut(dep_type).and_then(|v| v.as_object_mut());

    if deps.is_none()
        && let Some(obj) = composer.as_object_mut()
    {
        obj.insert(dep_type, sonic_rs::json!({}));
    }

    if let Some(deps) = composer.get_mut(dep_type).and_then(|v| v.as_object_mut()) {
        for (name, constraint) in &resolved {
            deps.insert(name.as_str(), sonic_rs::json!(constraint));
        }

        // Sort if requested
        if args.sort_packages {
            let mut sorted: Vec<(String, sonic_rs::Value)> = deps
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));

            deps.clear();
            for (k, v) in sorted {
                deps.insert(k.as_str(), v);
            }
        }
    }

    // Write composer.json
    let output = sonic_rs::to_string_pretty(&composer)?;
    std::fs::write(&composer_path, format!("{output}\n"))?;

    success(&format!(
        "Added {} package(s) to {}",
        resolved.len(),
        dep_type
    ));

    // Run update unless --no-update
    if !args.no_update {
        println!();
        info("Running update to install new packages...");

        let update_args = crate::commands::update::UpdateArgs {
            packages: resolved.iter().map(|(n, _)| n.clone()).collect(),
            no_dev: false,
            prefer_lowest: false,
            prefer_stable: args.prefer_stable,
            dry_run: false,
            root_reqs: false,
            lock: false,
            audit: false,
            fail_on_audit: false,
        };

        crate::commands::update::run(update_args).await?;
    }

    Ok(())
}
