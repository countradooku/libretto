//! Update command implementation.

use anyhow::Result;
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::collections::HashMap;

/// Arguments for the update command.
#[derive(Args, Debug, Clone)]
pub struct UpdateArgs {
    /// Packages to update (all if empty)
    #[arg(value_name = "PACKAGE")]
    pub packages: Vec<String>,

    /// Skip dev dependencies
    #[arg(long)]
    pub no_dev: bool,

    /// Prefer lowest versions
    #[arg(long)]
    pub prefer_lowest: bool,

    /// Prefer stable versions
    #[arg(long)]
    pub prefer_stable: bool,

    /// Dry run (don't update anything)
    #[arg(long)]
    pub dry_run: bool,

    /// Only update root dependencies
    #[arg(long)]
    pub root_reqs: bool,

    /// Lock file only (don't install)
    #[arg(long)]
    pub lock: bool,
}

/// Run the update command.
pub async fn run(args: UpdateArgs) -> Result<()> {
    use crate::output::progress::Spinner;
    use crate::output::table::Table;
    use crate::output::{header, info, success, warning};
    use libretto_repository::Repository;

    header("Updating dependencies");

    let cwd = std::env::current_dir()?;
    let composer_path = cwd.join("composer.json");
    let lock_path = cwd.join("composer.lock");

    if !composer_path.exists() {
        anyhow::bail!("composer.json not found in current directory");
    }

    let composer_content = std::fs::read_to_string(&composer_path)?;
    let composer: sonic_rs::Value = sonic_rs::from_str(&composer_content)?;

    if args.dry_run {
        warning("Dry run mode - no changes will be made");
    }

    // Collect current locked versions
    let mut current_versions: HashMap<String, String> = HashMap::new();
    if lock_path.exists() {
        let lock_content = std::fs::read_to_string(&lock_path)?;
        let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

        for key in ["packages", "packages-dev"] {
            if let Some(packages) = lock.get(key).and_then(|v| v.as_array()) {
                for pkg in packages {
                    let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");
                    current_versions.insert(name.to_string(), version.to_string());
                }
            }
        }
    }

    // Collect requirements
    let mut requirements: Vec<(String, String, bool)> = Vec::new();

    if let Some(require) = composer.get("require").and_then(|v| v.as_object()) {
        for (name, constraint) in require {
            if name.starts_with("php") || name.starts_with("ext-") {
                continue;
            }

            // Filter by specified packages
            if !args.packages.is_empty() && !args.packages.iter().any(|p| name.contains(p.as_str()))
            {
                continue;
            }

            let c = constraint.as_str().unwrap_or("*");
            requirements.push((name.to_string(), c.to_string(), false));
        }
    }

    if !args.no_dev {
        if let Some(require_dev) = composer.get("require-dev").and_then(|v| v.as_object()) {
            for (name, constraint) in require_dev {
                if name.starts_with("php") || name.starts_with("ext-") {
                    continue;
                }

                if !args.packages.is_empty()
                    && !args.packages.iter().any(|p| name.contains(p.as_str()))
                {
                    continue;
                }

                let c = constraint.as_str().unwrap_or("*");
                requirements.push((name.to_string(), c.to_string(), true));
            }
        }
    }

    if requirements.is_empty() {
        info("No packages to update");
        return Ok(());
    }

    if args.packages.is_empty() {
        info(&format!("Updating {} package(s)...", requirements.len()));
    } else {
        info(&format!(
            "Updating {} of {} package(s)...",
            args.packages.len(),
            requirements.len()
        ));
    }

    // Resolve new versions
    let spinner = Spinner::new("Resolving dependencies...");
    let repo = Repository::packagist()?;
    repo.init_packagist().await?;

    let mut updates: Vec<(String, String, String, bool, bool)> = Vec::new(); // name, old, new, is_dev, changed

    for (name, constraint, is_dev) in &requirements {
        let package_id = match libretto_core::PackageId::parse(name) {
            Some(id) => id,
            None => continue,
        };

        let version_constraint = libretto_core::VersionConstraint::new(constraint);

        match repo.find_version(&package_id, &version_constraint).await {
            Ok(pkg) => {
                let old_version = current_versions
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "(new)".to_string());
                let new_version = pkg.version.to_string();
                let changed = old_version != new_version;

                updates.push((name.clone(), old_version, new_version, *is_dev, changed));
            }
            Err(e) => {
                warning(&format!("Could not resolve {}: {}", name, e));
            }
        }
    }

    spinner.finish_and_clear();

    // Display updates
    let changed_count = updates.iter().filter(|(_, _, _, _, c)| *c).count();

    if changed_count == 0 {
        success("All packages are already at their latest versions");
        return Ok(());
    }

    info(&format!("{} package(s) will be updated:", changed_count));
    println!();

    let mut table = Table::new();
    table.headers(["Package", "Current", "New", "Type"]);

    for (name, old, new, is_dev, changed) in &updates {
        if !*changed {
            continue;
        }

        let pkg_type = if *is_dev { "dev" } else { "prod" };

        let old_cell = if old == "(new)" {
            table.success_cell(old)
        } else {
            table.warning_cell(old)
        };

        table.styled_row(vec![
            comfy_table::Cell::new(name),
            old_cell,
            table.success_cell(new),
            comfy_table::Cell::new(pkg_type),
        ]);
    }

    table.print();

    if args.dry_run {
        println!();
        warning("Dry run - no changes made");
        return Ok(());
    }

    // Update lock file
    if !args.lock {
        info("Updating composer.lock...");
    }

    // Generate new lock file
    let mut packages: Vec<sonic_rs::Value> = Vec::new();
    let mut packages_dev: Vec<sonic_rs::Value> = Vec::new();

    for (name, _, version, is_dev, _) in &updates {
        let pkg = sonic_rs::json!({
            "name": name,
            "version": version
        });

        if *is_dev {
            packages_dev.push(pkg);
        } else {
            packages.push(pkg);
        }
    }

    let content_hash =
        libretto_core::ContentHash::from_bytes(sonic_rs::to_string(&composer)?.as_bytes());

    let lock = sonic_rs::json!({
        "_readme": [
            "This file locks the dependencies of your project to a known state",
            "Read more about it at https://getcomposer.org/doc/01-basic-usage.md#installing-dependencies"
        ],
        "content-hash": content_hash.to_hex(),
        "packages": packages,
        "packages-dev": packages_dev,
        "aliases": [],
        "minimum-stability": "stable",
        "prefer-stable": args.prefer_stable,
        "prefer-lowest": args.prefer_lowest
    });

    let output = sonic_rs::to_string_pretty(&lock)?;
    std::fs::write(&lock_path, format!("{output}\n"))?;

    // Install updated packages
    if !args.lock {
        info("Installing updated packages...");

        let install_args = crate::commands::install::InstallArgs {
            no_dev: args.no_dev,
            prefer_dist: true,
            prefer_source: false,
            dry_run: false,
            ignore_platform_reqs: false,
            ignore_platform_req: vec![],
            optimize_autoloader: false,
            classmap_authoritative: false,
            apcu_autoloader: false,
            no_scripts: false,
            prefer_lowest: false,
            prefer_stable: true,
            minimum_stability: None,
            no_progress: false,
            concurrency: 64,
        };

        crate::commands::install::run(install_args).await?;
    }

    success(&format!("Updated {} package(s)", changed_count));

    Ok(())
}
