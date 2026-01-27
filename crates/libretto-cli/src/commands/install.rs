//! Install command implementation.

use anyhow::{Context, Result};
use clap::Args;
use sonic_rs::{JsonContainerTrait, JsonValueTrait};
use std::path::PathBuf;

/// Arguments for the install command.
#[derive(Args, Debug, Clone)]
pub struct InstallArgs {
    /// Skip dev dependencies
    #[arg(long)]
    pub no_dev: bool,

    /// Prefer dist packages (archives)
    #[arg(long)]
    pub prefer_dist: bool,

    /// Prefer source packages (VCS)
    #[arg(long)]
    pub prefer_source: bool,

    /// Dry run (don't install anything)
    #[arg(long)]
    pub dry_run: bool,

    /// Ignore platform requirements
    #[arg(long)]
    pub ignore_platform_reqs: bool,

    /// Optimize autoloader
    #[arg(short = 'o', long)]
    pub optimize_autoloader: bool,
}

/// Run the install command.
pub async fn run(args: InstallArgs) -> Result<()> {
    use crate::output::{header, info, success, warning};

    header("Installing dependencies");

    let cwd = std::env::current_dir()?;
    let composer_json_path = cwd.join("composer.json");
    let composer_lock_path = cwd.join("composer.lock");
    let vendor_dir = cwd.join("vendor");

    // Check for composer.json
    if !composer_json_path.exists() {
        anyhow::bail!(
            "No composer.json found in current directory.\nRun 'libretto init' to create one."
        );
    }

    // Read composer.json
    let composer_content =
        std::fs::read_to_string(&composer_json_path).context("Failed to read composer.json")?;
    let composer: sonic_rs::Value =
        sonic_rs::from_str(&composer_content).context("Failed to parse composer.json")?;

    if args.dry_run {
        warning("Dry run mode - no changes will be made");
    }

    if args.no_dev {
        info("Skipping dev dependencies");
    }

    // Check for lock file
    let has_lock = composer_lock_path.exists();

    if has_lock {
        info("Installing from composer.lock");
        install_from_lock(&composer_lock_path, &vendor_dir, &args).await?;
    } else {
        info("No lock file found, resolving dependencies...");
        resolve_and_install(&composer, &composer_lock_path, &vendor_dir, &args).await?;
    }

    // Generate autoloader
    if !args.dry_run {
        info("Generating autoloader...");
        generate_autoloader(&vendor_dir, args.optimize_autoloader)?;
    }

    success("Installation complete");

    Ok(())
}

async fn install_from_lock(
    lock_path: &PathBuf,
    vendor_dir: &PathBuf,
    args: &InstallArgs,
) -> Result<()> {
    use crate::output::progress::MultiProgress;
    use crate::output::{format_bytes, info};

    let lock_content = std::fs::read_to_string(lock_path)?;
    let lock: sonic_rs::Value = sonic_rs::from_str(&lock_content)?;

    // Collect packages to install
    let mut packages: Vec<(String, String, Option<String>, bool)> = Vec::new();

    if let Some(pkgs) = lock.get("packages").and_then(|v| v.as_array()) {
        for pkg in pkgs {
            let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");
            let dist_url = pkg
                .get("dist")
                .and_then(|d| d.get("url"))
                .and_then(|u| u.as_str())
                .map(String::from);

            packages.push((name.to_string(), version.to_string(), dist_url, false));
        }
    }

    if !args.no_dev {
        if let Some(pkgs) = lock.get("packages-dev").and_then(|v| v.as_array()) {
            for pkg in pkgs {
                let name = pkg.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("");
                let dist_url = pkg
                    .get("dist")
                    .and_then(|d| d.get("url"))
                    .and_then(|u| u.as_str())
                    .map(String::from);

                packages.push((name.to_string(), version.to_string(), dist_url, true));
            }
        }
    }

    if packages.is_empty() {
        info("No packages to install");
        return Ok(());
    }

    info(&format!("Installing {} package(s)...", packages.len()));

    if args.dry_run {
        // Just show what would be installed
        use crate::output::table::Table;

        let mut table = Table::new();
        table.headers(["Package", "Version", "Type"]);

        for (name, version, _, is_dev) in &packages {
            let pkg_type = if *is_dev { "dev" } else { "prod" };
            table.row([name.as_str(), version.as_str(), pkg_type]);
        }

        table.print();
        return Ok(());
    }

    // Create vendor directory
    std::fs::create_dir_all(vendor_dir)?;

    // Install packages with progress
    let mp = MultiProgress::new();
    let mut total_bytes: u64 = 0;

    for (name, version, dist_url, _) in &packages {
        let spinner = mp.add_spinner(format!("Installing {}...", name));

        if let Some(url) = dist_url {
            match download_and_extract(url, vendor_dir, name).await {
                Ok(bytes) => {
                    total_bytes += bytes;
                    spinner.finish_with_message(format!("{} {} installed", name, version));
                }
                Err(e) => {
                    spinner.finish_with_message(format!("{} failed: {}", name, e));
                    crate::output::warning(&format!("Failed to install {}: {}", name, e));
                }
            }
        } else {
            spinner.finish_with_message(format!("{} {} (no dist)", name, version));
        }
    }

    info(&format!("Downloaded: {}", format_bytes(total_bytes)));

    Ok(())
}

async fn resolve_and_install(
    composer: &sonic_rs::Value,
    lock_path: &PathBuf,
    vendor_dir: &PathBuf,
    args: &InstallArgs,
) -> Result<()> {
    use crate::output::progress::Spinner;
    use crate::output::{info, warning};
    use libretto_repository::Repository;

    let spinner = Spinner::new("Resolving dependencies...");

    // Collect requirements
    let mut requirements: Vec<(String, String)> = Vec::new();

    if let Some(require) = composer.get("require").and_then(|v| v.as_object()) {
        for (name, constraint) in require {
            if !name.starts_with("php") && !name.starts_with("ext-") {
                let c = constraint.as_str().unwrap_or("*");
                requirements.push((name.to_string(), c.to_string()));
            }
        }
    }

    let mut dev_requirements: Vec<(String, String)> = Vec::new();
    if !args.no_dev {
        if let Some(require_dev) = composer.get("require-dev").and_then(|v| v.as_object()) {
            for (name, constraint) in require_dev {
                if !name.starts_with("php") && !name.starts_with("ext-") {
                    let c = constraint.as_str().unwrap_or("*");
                    dev_requirements.push((name.to_string(), c.to_string()));
                }
            }
        }
    }

    spinner.finish_and_clear();

    if requirements.is_empty() && dev_requirements.is_empty() {
        info("No dependencies to install");
        return Ok(());
    }

    info(&format!(
        "Found {} production and {} dev dependencies",
        requirements.len(),
        dev_requirements.len()
    ));

    // Fetch package information
    let repo = Repository::packagist()?;
    repo.init_packagist().await?;
    let mut resolved: Vec<(String, String, Option<String>, bool)> = Vec::new();

    for (name, constraint) in &requirements {
        let package_id = match libretto_core::PackageId::parse(name) {
            Some(id) => id,
            None => {
                warning(&format!("Invalid package name: {}", name));
                continue;
            }
        };

        let version_constraint = libretto_core::VersionConstraint::new(constraint);

        match repo.find_version(&package_id, &version_constraint).await {
            Ok(pkg) => {
                let dist_url = pkg.dist.as_ref().and_then(|d| match d {
                    libretto_core::PackageSource::Dist { url, .. } => Some(url.to_string()),
                    libretto_core::PackageSource::Git { url, .. } => Some(url.to_string()),
                });
                resolved.push((name.clone(), pkg.version.to_string(), dist_url, false));
            }
            Err(e) => {
                warning(&format!("Could not resolve {}: {}", name, e));
            }
        }
    }

    for (name, constraint) in &dev_requirements {
        let package_id = match libretto_core::PackageId::parse(name) {
            Some(id) => id,
            None => continue,
        };

        let version_constraint = libretto_core::VersionConstraint::new(constraint);

        match repo.find_version(&package_id, &version_constraint).await {
            Ok(pkg) => {
                let dist_url = pkg.dist.as_ref().and_then(|d| match d {
                    libretto_core::PackageSource::Dist { url, .. } => Some(url.to_string()),
                    libretto_core::PackageSource::Git { url, .. } => Some(url.to_string()),
                });
                resolved.push((name.clone(), pkg.version.to_string(), dist_url, true));
            }
            Err(_) => {}
        }
    }

    // Generate lock file
    if !args.dry_run {
        generate_lock_file(lock_path, &resolved, composer)?;
    }

    // Install packages
    if args.dry_run {
        use crate::output::table::Table;

        let mut table = Table::new();
        table.headers(["Package", "Version", "Type"]);

        for (name, version, _, is_dev) in &resolved {
            let pkg_type = if *is_dev { "dev" } else { "prod" };
            table.row([name.as_str(), version.as_str(), pkg_type]);
        }

        table.print();
    } else {
        std::fs::create_dir_all(vendor_dir)?;

        for (name, version, dist_url, _) in &resolved {
            if let Some(url) = dist_url {
                info(&format!("Installing {} {}...", name, version));
                download_and_extract(url, vendor_dir, name).await?;
            }
        }
    }

    Ok(())
}

async fn download_and_extract(url: &str, vendor_dir: &PathBuf, name: &str) -> Result<u64> {
    use crate::output::warning;

    // Create client with redirect following enabled
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("Libretto/0.1.0 (https://github.com/libretto-pm/libretto)")
        .build()?;

    let mut request = client.get(url);

    // Add GitHub token if available for higher rate limits
    if url.contains("api.github.com") {
        if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
            request = request.header("Authorization", format!("token {}", token));
        }
    }

    let response = request.send().await?;
    let status = response.status();

    if !status.is_success() {
        if status.as_u16() == 403 && url.contains("github.com") {
            warning("GitHub API rate limit may be exceeded.");
            warning("Set GITHUB_TOKEN environment variable for higher rate limits.");
        }
        anyhow::bail!("Download failed: HTTP {} from {}", status, response.url());
    }

    let bytes = response.bytes().await?;
    let size = bytes.len() as u64;

    // Create package directory
    let pkg_dir = vendor_dir.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
    std::fs::create_dir_all(&pkg_dir)?;

    // Extract zip
    let cursor = std::io::Cursor::new(&bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    // Find common prefix
    let prefix = archive
        .file_names()
        .next()
        .and_then(|name| name.split('/').next())
        .map(String::from);

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        // Strip prefix
        let relative_path = if let Some(ref prefix) = prefix {
            name.strip_prefix(prefix)
                .and_then(|s| s.strip_prefix('/'))
                .unwrap_or(&name)
        } else {
            &name
        };

        if relative_path.is_empty() {
            continue;
        }

        let target_path = pkg_dir.join(relative_path);

        if file.is_dir() {
            std::fs::create_dir_all(&target_path)?;
        } else {
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&target_path)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(size)
}

fn generate_lock_file(
    lock_path: &PathBuf,
    resolved: &[(String, String, Option<String>, bool)],
    composer: &sonic_rs::Value,
) -> Result<()> {
    let mut packages: Vec<sonic_rs::Value> = Vec::new();
    let mut packages_dev: Vec<sonic_rs::Value> = Vec::new();

    for (name, version, dist_url, is_dev) in resolved {
        let pkg = sonic_rs::json!({
            "name": name,
            "version": version,
            "dist": {
                "type": "zip",
                "url": dist_url
            }
        });

        if *is_dev {
            packages_dev.push(pkg);
        } else {
            packages.push(pkg);
        }
    }

    // Calculate content hash
    let content_hash =
        libretto_core::ContentHash::from_bytes(sonic_rs::to_string(composer)?.as_bytes());

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
        "prefer-stable": true,
        "prefer-lowest": false
    });

    let output = sonic_rs::to_string_pretty(&lock)?;
    std::fs::write(lock_path, format!("{output}\n"))?;

    Ok(())
}

fn generate_autoloader(vendor_dir: &PathBuf, optimize: bool) -> Result<()> {
    use libretto_autoloader::{AutoloaderGenerator, OptimizationLevel};

    let level = if optimize {
        OptimizationLevel::Optimized
    } else {
        OptimizationLevel::None
    };

    let _generator = AutoloaderGenerator::with_optimization(vendor_dir.clone(), level);

    // Scan packages and generate autoloader
    // For now, create a basic autoload.php
    let autoload_path = vendor_dir.join("autoload.php");
    let autoload_content = r#"<?php

// Libretto autoloader
// Generated by libretto

require_once __DIR__ . '/composer/autoload_real.php';

return ComposerAutoloaderInit::getLoader();
"#;

    std::fs::write(&autoload_path, autoload_content)?;

    // Create composer directory
    let composer_dir = vendor_dir.join("composer");
    std::fs::create_dir_all(&composer_dir)?;

    // Create minimal autoload files
    let autoload_real = r#"<?php

class ComposerAutoloaderInit
{
    private static $loader;

    public static function getLoader()
    {
        if (null !== self::$loader) {
            return self::$loader;
        }

        spl_autoload_register(array('ComposerAutoloaderInit', 'autoload'), true, true);
        self::$loader = new \Composer\Autoload\ClassLoader();

        return self::$loader;
    }

    public static function autoload($class)
    {
        // Basic PSR-4 autoloading
        $prefix = '';
        $baseDir = __DIR__ . '/../';

        $file = $baseDir . str_replace('\\', '/', $class) . '.php';
        if (file_exists($file)) {
            require $file;
        }
    }
}
"#;

    std::fs::write(composer_dir.join("autoload_real.php"), autoload_real)?;

    Ok(())
}
