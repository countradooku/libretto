//! Install command implementation.
//!
//! High-performance package installation using parallel resolution and downloads.

use crate::cas_cache;
use crate::fetcher::Fetcher;
use crate::output::live::LiveProgress;
use crate::output::table::Table;
use crate::output::{error, format_bytes, header, info, success, warning};
use crate::platform::PlatformValidator;
use crate::scripts::{
    ScriptConfig, run_post_autoload_scripts, run_post_install_scripts, run_pre_autoload_scripts,
    run_pre_install_scripts,
};
use anyhow::{Context, Result, bail};
use clap::Args;
use futures::stream::{FuturesUnordered, StreamExt};
use libretto_resolver::Stability;
use libretto_resolver::turbo::{TurboConfig, TurboResolver};
use libretto_resolver::{ComposerConstraint, Dependency, PackageName, ResolutionMode};
use sonic_rs::{JsonContainerTrait, JsonValueTrait, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tracing::debug;

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

    /// Ignore specific platform requirements (e.g., php, ext-*)
    #[arg(long, value_name = "REQ")]
    pub ignore_platform_req: Vec<String>,

    /// Optimize autoloader
    #[arg(short = 'o', long)]
    pub optimize_autoloader: bool,

    /// Generate classmap for PSR-0/4 autoloading
    #[arg(short = 'a', long)]
    pub classmap_authoritative: bool,

    /// APCu autoloader caching
    #[arg(long)]
    pub apcu_autoloader: bool,

    /// Skip scripts execution
    #[arg(long)]
    pub no_scripts: bool,

    /// Prefer lowest versions (for testing)
    #[arg(long)]
    pub prefer_lowest: bool,

    /// Prefer stable versions
    #[arg(long)]
    pub prefer_stable: bool,

    /// Minimum stability (dev, alpha, beta, RC, stable)
    #[arg(long, value_name = "STABILITY")]
    pub minimum_stability: Option<String>,

    /// Disable progress bar
    #[arg(long)]
    pub no_progress: bool,

    /// Maximum concurrent HTTP requests
    #[arg(long, default_value = "64")]
    pub concurrency: usize,
}

/// Run the install command.
pub async fn run(args: InstallArgs) -> Result<()> {
    let start = Instant::now();
    header("Installing dependencies");

    let cwd = std::env::current_dir()?;
    let composer_json_path = cwd.join("composer.json");
    let composer_lock_path = cwd.join("composer.lock");
    let vendor_dir = cwd.join("vendor");

    // Check for composer.json
    if !composer_json_path.exists() {
        bail!("No composer.json found in current directory.\nRun 'libretto init' to create one.");
    }

    // Read composer.json
    let composer_content =
        std::fs::read_to_string(&composer_json_path).context("Failed to read composer.json")?;
    let composer: Value =
        sonic_rs::from_str(&composer_content).context("Failed to parse composer.json")?;

    if args.dry_run {
        warning("Dry run mode - no changes will be made");
    }

    // Script config for lifecycle hooks
    let script_config = ScriptConfig {
        working_dir: cwd.clone(),
        dev_mode: !args.no_dev,
        ..Default::default()
    };

    // Run pre-install scripts
    if !args.no_scripts && !args.dry_run {
        if let Some(result) = run_pre_install_scripts(&composer, &script_config, false)? {
            if result.success {
                debug!(
                    "Pre-install script: {} commands in {}ms",
                    result.commands_executed,
                    result.duration.as_millis()
                );
            } else if let Some(ref err) = result.error {
                warning(&format!("Pre-install script warning: {}", err));
            }
        }
    }

    // Create live progress display
    let progress = if !args.no_progress && !args.dry_run {
        Some(LiveProgress::new())
    } else {
        None
    };

    // Check for lock file
    let has_lock = composer_lock_path.exists();

    let result = if has_lock && !args.prefer_lowest {
        install_from_lock(&composer_lock_path, &vendor_dir, &args, progress.as_ref()).await
    } else {
        resolve_and_install(
            &composer,
            &composer_lock_path,
            &vendor_dir,
            &args,
            progress.as_ref(),
        )
        .await
    };

    // Handle result and finish progress
    match result {
        Ok(()) => {
            let elapsed = start.elapsed();
            if let Some(p) = &progress {
                p.finish_success(&format!(
                    "Installed in {}",
                    crate::output::format_duration(elapsed)
                ));
            } else {
                success(&format!(
                    "Installation complete ({})",
                    crate::output::format_duration(elapsed)
                ));
            }
        }
        Err(e) => {
            if let Some(p) = &progress {
                p.finish_error(&e.to_string());
            }
            return Err(e);
        }
    }

    // Generate autoloader
    if !args.dry_run {
        // Pre-autoload-dump scripts
        if !args.no_scripts {
            if let Some(result) = run_pre_autoload_scripts(&composer, &script_config)? {
                if !result.success {
                    if let Some(ref err) = result.error {
                        warning(&format!("Pre-autoload script warning: {}", err));
                    }
                }
            }
        }

        generate_autoloader(&vendor_dir, &args)?;

        // Post-autoload-dump scripts
        if !args.no_scripts {
            if let Some(result) = run_post_autoload_scripts(&composer, &script_config)? {
                if !result.success {
                    if let Some(ref err) = result.error {
                        warning(&format!("Post-autoload script warning: {}", err));
                    }
                }
            }
        }
    }

    // Run post-install scripts
    if !args.no_scripts && !args.dry_run {
        if let Some(result) = run_post_install_scripts(&composer, &script_config, false)? {
            if result.success {
                debug!(
                    "Post-install script: {} commands in {}ms",
                    result.commands_executed,
                    result.duration.as_millis()
                );
            } else if let Some(ref err) = result.error {
                warning(&format!("Post-install script warning: {}", err));
            }
        }
    }

    Ok(())
}

/// Install from an existing lock file.
async fn install_from_lock(
    lock_path: &PathBuf,
    vendor_dir: &PathBuf,
    args: &InstallArgs,
    progress: Option<&LiveProgress>,
) -> Result<()> {
    let lock_content = std::fs::read_to_string(lock_path)?;
    let lock: Value = sonic_rs::from_str(&lock_content)?;

    // Collect packages to install
    let mut packages: Vec<PackageInfo> = Vec::new();

    if let Some(pkgs) = lock.get("packages").and_then(|v| v.as_array()) {
        for pkg in pkgs {
            if let Some(info) = parse_lock_package(pkg, false) {
                packages.push(info);
            }
        }
    }

    if !args.no_dev {
        if let Some(pkgs) = lock.get("packages-dev").and_then(|v| v.as_array()) {
            for pkg in pkgs {
                if let Some(info) = parse_lock_package(pkg, true) {
                    packages.push(info);
                }
            }
        }
    }

    if packages.is_empty() {
        info("No packages to install");
        return Ok(());
    }

    // Validate platform requirements
    if !args.ignore_platform_reqs {
        validate_platform_from_lock(&lock, args)?;
    }

    if args.dry_run {
        info(&format!("Would install {} package(s)", packages.len()));
        show_packages_table(&packages);
        return Ok(());
    }

    // Create vendor directory
    std::fs::create_dir_all(vendor_dir)?;

    // Install packages
    install_packages(&packages, vendor_dir, args, progress).await?;

    Ok(())
}

/// Resolve dependencies and install.
async fn resolve_and_install(
    composer: &Value,
    lock_path: &PathBuf,
    vendor_dir: &PathBuf,
    args: &InstallArgs,
    progress: Option<&LiveProgress>,
) -> Result<()> {
    // Collect requirements from composer.json
    let mut require: HashMap<String, String> = HashMap::new();
    let mut require_dev: HashMap<String, String> = HashMap::new();

    if let Some(req) = composer.get("require").and_then(|v| v.as_object()) {
        for (name, constraint) in req {
            if let Some(c) = constraint.as_str() {
                require.insert(name.to_string(), c.to_string());
            }
        }
    }

    if let Some(req) = composer.get("require-dev").and_then(|v| v.as_object()) {
        for (name, constraint) in req {
            if let Some(c) = constraint.as_str() {
                require_dev.insert(name.to_string(), c.to_string());
            }
        }
    }

    if require.is_empty() && require_dev.is_empty() {
        info("No dependencies to install");
        return Ok(());
    }

    info(&format!(
        "Found {} production and {} dev dependencies",
        require.len(),
        require_dev.len()
    ));

    // Parse minimum stability
    let min_stability = args
        .minimum_stability
        .as_deref()
        .and_then(parse_stability)
        .or_else(|| {
            composer
                .get("minimum-stability")
                .and_then(|v| v.as_str())
                .and_then(parse_stability)
        })
        .unwrap_or(Stability::Stable);

    // Create fetcher
    let fetcher =
        Arc::new(Fetcher::new().map_err(|e| anyhow::anyhow!("Failed to create fetcher: {}", e))?);

    // Configure resolver
    let config = TurboConfig {
        max_concurrent: args.concurrency.max(32),
        request_timeout: std::time::Duration::from_secs(10),
        mode: if args.prefer_lowest {
            ResolutionMode::PreferLowest
        } else {
            ResolutionMode::PreferHighest
        },
        min_stability,
        include_dev: !args.no_dev,
    };

    // Parse dependencies
    let mut root_deps = Vec::new();
    let mut dev_deps = Vec::new();

    for (name, constraint) in &require {
        if is_platform_package(name) {
            continue;
        }
        if let (Some(n), Some(c)) = (
            PackageName::parse(name),
            ComposerConstraint::parse(constraint),
        ) {
            root_deps.push(Dependency::new(n, c));
        }
    }

    for (name, constraint) in &require_dev {
        if is_platform_package(name) {
            continue;
        }
        if let (Some(n), Some(c)) = (
            PackageName::parse(name),
            ComposerConstraint::parse(constraint),
        ) {
            dev_deps.push(Dependency::new(n, c));
        }
    }

    // Resolve dependencies
    if let Some(p) = progress {
        p.set_resolving();
    }

    let resolver = TurboResolver::new(fetcher.clone(), config);
    let resolution = resolver
        .resolve(&root_deps, &dev_deps)
        .await
        .map_err(|e| anyhow::anyhow!("Resolution failed: {}", e))?;

    // Convert to package info
    let packages: Vec<PackageInfo> = resolution
        .packages
        .iter()
        .map(|p| PackageInfo {
            name: p.name.as_str().to_string(),
            version: p.version.to_string(),
            is_dev: p.is_dev,
            dist_url: p.dist_url.clone(),
            dist_shasum: p.dist_shasum.clone(),
        })
        .collect();

    if args.dry_run {
        info(&format!("Would install {} package(s)", packages.len()));
        show_packages_table(&packages);
        return Ok(());
    }

    // Create vendor directory
    std::fs::create_dir_all(vendor_dir)?;

    // Install packages
    install_packages(&packages, vendor_dir, args, progress).await?;

    // Generate lock file
    generate_lock_file(lock_path, &resolution, composer)?;

    Ok(())
}

fn parse_stability(s: &str) -> Option<Stability> {
    match s.to_lowercase().as_str() {
        "dev" => Some(Stability::Dev),
        "alpha" => Some(Stability::Alpha),
        "beta" => Some(Stability::Beta),
        "rc" => Some(Stability::RC),
        "stable" => Some(Stability::Stable),
        _ => None,
    }
}

fn is_platform_package(name: &str) -> bool {
    name == "php"
        || name.starts_with("php-")
        || name.starts_with("ext-")
        || name.starts_with("lib-")
        || name == "composer"
        || name == "composer-plugin-api"
        || name == "composer-runtime-api"
}

/// Package information for installation.
#[derive(Debug, Clone)]
struct PackageInfo {
    name: String,
    version: String,
    is_dev: bool,
    dist_url: Option<String>,
    dist_shasum: Option<String>,
}

fn parse_lock_package(pkg: &Value, is_dev: bool) -> Option<PackageInfo> {
    let name = pkg.get("name").and_then(|v| v.as_str())?;
    let version = pkg.get("version").and_then(|v| v.as_str())?;
    let dist_url = pkg
        .get("dist")
        .and_then(|d| d.get("url"))
        .and_then(|u| u.as_str())
        .map(String::from);
    let dist_shasum = pkg
        .get("dist")
        .and_then(|d| d.get("shasum"))
        .and_then(|u| u.as_str())
        .map(String::from);

    Some(PackageInfo {
        name: name.to_string(),
        version: version.to_string(),
        is_dev,
        dist_url,
        dist_shasum,
    })
}

fn validate_platform_from_lock(lock: &Value, args: &InstallArgs) -> Result<()> {
    let mut requirements: Vec<(&str, &str, Vec<String>)> = Vec::new();

    if let Some(platform) = lock.get("platform").and_then(|v| v.as_object()) {
        for (name, constraint) in platform {
            if let Some(c) = constraint.as_str() {
                if args
                    .ignore_platform_req
                    .iter()
                    .any(|r| r == name || r == "*")
                {
                    continue;
                }
                requirements.push((name, c, vec!["lock file".to_string()]));
            }
        }
    }

    if requirements.is_empty() {
        return Ok(());
    }

    let mut validator = PlatformValidator::new();
    validator.detect()?;

    let result = validator.validate(&requirements)?;

    if !result.is_satisfied() {
        warning("Platform requirements not satisfied:");
        for err in &result.errors {
            error(&format!(
                "  {} {} required, {} installed",
                err.name,
                err.constraint,
                err.installed.as_deref().unwrap_or("not found")
            ));
        }
        if !args.ignore_platform_reqs {
            bail!("Platform requirements check failed. Use --ignore-platform-reqs to skip.");
        }
    }

    Ok(())
}

fn show_packages_table(packages: &[PackageInfo]) {
    let mut table = Table::new();
    table.headers(["Package", "Version", "Type"]);

    for pkg in packages {
        let pkg_type = if pkg.is_dev { "dev" } else { "prod" };
        table.row([pkg.name.as_str(), pkg.version.as_str(), pkg_type]);
    }

    table.print();
}

/// Install packages with parallel downloads and CAS cache.
async fn install_packages(
    packages: &[PackageInfo],
    vendor_dir: &PathBuf,
    args: &InstallArgs,
    progress: Option<&LiveProgress>,
) -> Result<()> {
    let start = Instant::now();

    // Build HTTP client with optimized settings
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(100)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .http2_adaptive_window(true)
        .http2_initial_stream_window_size(Some(4 * 1024 * 1024))
        .http2_initial_connection_window_size(Some(8 * 1024 * 1024))
        .http2_keep_alive_interval(Some(std::time::Duration::from_secs(15)))
        .http2_keep_alive_timeout(std::time::Duration::from_secs(20))
        .http2_keep_alive_while_idle(true)
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .tcp_nodelay(true)
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .gzip(true)
        .brotli(true)
        .deflate(true)
        .build()
        .context("Failed to create HTTP client")?;

    // Separate cached vs need-download
    let mut to_download: Vec<(String, String, String, PathBuf)> = Vec::new();
    let mut from_cache: Vec<(String, PathBuf, PathBuf)> = Vec::new();
    let mut skipped = 0;

    for pkg in packages {
        let dest = vendor_dir.join(pkg.name.replace('/', std::path::MAIN_SEPARATOR_STR));

        if let Some(ref url_str) = pkg.dist_url {
            let url = convert_github_api_url(url_str);

            if let Some(cache_path) = cas_cache::get_cached_path(&url) {
                from_cache.push((pkg.name.clone(), cache_path, dest));
            } else {
                to_download.push((pkg.name.clone(), pkg.version.clone(), url, dest));
            }
        } else {
            skipped += 1;
        }
    }

    let cached_count = from_cache.len();
    let download_count = to_download.len();
    let total = cached_count + download_count;

    if total == 0 {
        if skipped > 0 {
            warning(&format!("No download URLs for {} packages", skipped));
        }
        return Ok(());
    }

    // Set up progress for all packages (downloads + cache links)
    if let Some(p) = progress {
        if download_count > 0 {
            p.set_downloading(total, cached_count);
        } else {
            p.set_linking(total);
        }
    }

    // Link cached packages first (instant)
    for (name, cache_path, dest) in &from_cache {
        if let Some(p) = progress {
            p.set_current(name);
        }
        if let Err(e) = cas_cache::link_from_cache(cache_path, dest) {
            warning(&format!("Cache link failed for {}: {}", name, e));
        }
        if let Some(p) = progress {
            p.inc_completed();
        }
    }

    if to_download.is_empty() {
        return Ok(());
    }

    // Adaptive concurrency based on CPU cores
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let max_concurrent = if args.dry_run {
        1
    } else {
        (cpu_cores * 8).clamp(32, 128)
    };

    let completed = Arc::new(AtomicU64::new(0));
    let failed_count = Arc::new(AtomicU64::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));

    let mut pending: Vec<_> = to_download.into_iter().collect();
    let mut in_flight = FuturesUnordered::new();
    let mut errors: Vec<String> = Vec::new();

    while !pending.is_empty() || !in_flight.is_empty() {
        while in_flight.len() < max_concurrent && !pending.is_empty() {
            let (name, version, url, dest) = pending.pop().unwrap();
            let client = client.clone();
            let total_bytes = Arc::clone(&total_bytes);

            // Update progress with current package
            if let Some(p) = progress {
                p.set_current(&name);
            }

            in_flight.push(async move {
                let result =
                    download_and_extract(&client, &name, &version, &url, &dest, &total_bytes).await;
                (name, url, result)
            });
        }

        if let Some((name, url, result)) = in_flight.next().await {
            match result {
                Ok(dest_path) => {
                    completed.fetch_add(1, Ordering::Relaxed);
                    if let Some(p) = progress {
                        p.inc_completed();
                        p.add_bytes(total_bytes.load(Ordering::Relaxed));
                    }
                    let _ = cas_cache::store_in_cache(&url, &dest_path);
                }
                Err(e) => {
                    failed_count.fetch_add(1, Ordering::Relaxed);
                    errors.push(format!("{}: {}", name, e));
                }
            }
        }
    }

    let elapsed = start.elapsed();
    let installed = completed.load(Ordering::Relaxed) + cached_count as u64;
    let failed = failed_count.load(Ordering::Relaxed);
    let bytes = total_bytes.load(Ordering::Relaxed);

    for err in &errors {
        warning(&format!("Failed: {}", err));
    }

    if failed > 0 {
        bail!(
            "Failed to install {} of {} packages. See warnings above.",
            failed,
            total
        );
    }

    Ok(())
}

async fn download_and_extract(
    client: &reqwest::Client,
    name: &str,
    _version: &str,
    url: &str,
    dest: &std::path::Path,
    total_bytes: &AtomicU64,
) -> Result<PathBuf> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch {}", name))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP {}", response.status());
    }

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read response for {}", name))?;

    total_bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);

    // Extract in blocking task to not block async runtime
    let dest = dest.to_path_buf();
    let name = name.to_string();
    tokio::task::spawn_blocking(move || {
        use std::io::Write;

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let temp_path = dest.with_extension("download.zip");
        {
            let mut file = std::fs::File::create(&temp_path)?;
            file.write_all(&bytes)?;
        }

        extract_zip(&temp_path, &dest).with_context(|| format!("Failed to extract {}", name))?;
        let _ = std::fs::remove_file(&temp_path);

        Ok(dest)
    })
    .await
    .context("Extraction task failed")?
}

fn extract_zip(zip_path: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    use std::io::Read;

    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // Find common prefix (GitHub zips have vendor-repo-hash/ prefix)
    let mut common_prefix: Option<String> = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let path = entry.name();
        if let Some(first_component) = path.split('/').next() {
            if !first_component.is_empty() {
                match &common_prefix {
                    None => common_prefix = Some(format!("{}/", first_component)),
                    Some(p) if !path.starts_with(p) => {
                        common_prefix = None;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let prefix_len = common_prefix.as_ref().map(|p| p.len()).unwrap_or(0);

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let entry_path = entry.name();

        if entry_path.len() <= prefix_len {
            continue;
        }

        let relative_path = &entry_path[prefix_len..];
        if relative_path.is_empty() {
            continue;
        }

        let out_path = dest.join(relative_path);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&out_path)?;
            let mut buffer = Vec::new();
            entry.read_to_end(&mut buffer)?;
            std::io::Write::write_all(&mut outfile, &buffer)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = entry.unix_mode() {
                    let _ =
                        std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
                }
            }
        }
    }

    Ok(())
}

fn generate_lock_file(
    lock_path: &PathBuf,
    resolution: &libretto_resolver::Resolution,
    composer: &Value,
) -> Result<()> {
    let mut packages: Vec<Value> = Vec::new();
    let mut packages_dev: Vec<Value> = Vec::new();

    for pkg in &resolution.packages {
        let mut entry = sonic_rs::json!({
            "name": pkg.name.as_str(),
            "version": pkg.version.to_string()
        });

        if let Some(ref url) = pkg.dist_url {
            entry["dist"] = sonic_rs::json!({
                "type": pkg.dist_type.as_deref().unwrap_or("zip"),
                "url": url,
                "shasum": pkg.dist_shasum.as_deref().unwrap_or("")
            });
        }

        if let Some(ref url) = pkg.source_url {
            entry["source"] = sonic_rs::json!({
                "type": pkg.source_type.as_deref().unwrap_or("git"),
                "url": url,
                "reference": pkg.source_reference.as_deref().unwrap_or("")
            });
        }

        if pkg.is_dev {
            packages_dev.push(entry);
        } else {
            packages.push(entry);
        }
    }

    let content_hash =
        libretto_core::ContentHash::from_bytes(sonic_rs::to_string(composer)?.as_bytes());

    let lock = sonic_rs::json!({
        "_readme": [
            "This file locks the dependencies of your project to a known state",
            "Read more about it at https://getcomposer.org/doc/01-basic-usage.md#installing-dependencies",
            "",
            "Generated by Libretto - https://github.com/libretto-pm/libretto"
        ],
        "content-hash": content_hash.to_hex(),
        "packages": packages,
        "packages-dev": packages_dev,
        "aliases": [],
        "minimum-stability": "stable",
        "stability-flags": {},
        "prefer-stable": true,
        "prefer-lowest": false,
        "platform": {},
        "platform-dev": {}
    });

    let output = sonic_rs::to_string_pretty(&lock)?;
    std::fs::write(lock_path, format!("{output}\n"))?;

    Ok(())
}

fn generate_autoloader(vendor_dir: &PathBuf, args: &InstallArgs) -> Result<()> {
    use libretto_autoloader::{AutoloaderGenerator, OptimizationLevel};

    let level = if args.classmap_authoritative {
        OptimizationLevel::Authoritative
    } else if args.optimize_autoloader {
        OptimizationLevel::Optimized
    } else {
        OptimizationLevel::None
    };

    let _generator = AutoloaderGenerator::with_optimization(vendor_dir.clone(), level);

    let autoload_path = vendor_dir.join("autoload.php");
    let autoload_content = r#"<?php

// Libretto autoloader
// Generated by libretto - https://github.com/libretto-pm/libretto

require_once __DIR__ . '/composer/autoload_real.php';

return ComposerAutoloaderInit::getLoader();
"#;

    std::fs::write(&autoload_path, autoload_content)?;

    let composer_dir = vendor_dir.join("composer");
    std::fs::create_dir_all(&composer_dir)?;

    let autoload_real = r#"<?php

class ComposerAutoloaderInit
{
    private static $loader;

    public static function getLoader()
    {
        if (null !== self::$loader) {
            return self::$loader;
        }

        require __DIR__ . '/ClassLoader.php';
        spl_autoload_register(array('ComposerAutoloaderInit', 'autoload'), true, true);
        self::$loader = new \Composer\Autoload\ClassLoader();

        $map = require __DIR__ . '/autoload_namespaces.php';
        foreach ($map as $namespace => $path) {
            self::$loader->set($namespace, $path);
        }

        $map = require __DIR__ . '/autoload_psr4.php';
        foreach ($map as $namespace => $path) {
            self::$loader->setPsr4($namespace, $path);
        }

        $classMap = require __DIR__ . '/autoload_classmap.php';
        if ($classMap) {
            self::$loader->addClassMap($classMap);
        }

        self::$loader->register(true);

        return self::$loader;
    }

    public static function autoload($class)
    {
        $file = __DIR__ . '/../' . str_replace('\\', '/', $class) . '.php';
        if (file_exists($file)) {
            require $file;
        }
    }
}
"#;

    std::fs::write(composer_dir.join("autoload_real.php"), autoload_real)?;
    std::fs::write(
        composer_dir.join("autoload_namespaces.php"),
        "<?php\n\nreturn array();\n",
    )?;
    std::fs::write(
        composer_dir.join("autoload_psr4.php"),
        "<?php\n\nreturn array();\n",
    )?;
    std::fs::write(
        composer_dir.join("autoload_classmap.php"),
        "<?php\n\nreturn array();\n",
    )?;

    let classloader = include_str!("../../resources/ClassLoader.php.template");
    std::fs::write(
        composer_dir.join("ClassLoader.php"),
        classloader.replace("{{VERSION}}", env!("CARGO_PKG_VERSION")),
    )?;

    Ok(())
}

/// Convert GitHub API URLs to codeload URLs to avoid rate limits.
fn convert_github_api_url(url: &str) -> String {
    if !url.starts_with("https://api.github.com/repos/") {
        return url.to_string();
    }

    let path = url.trim_start_matches("https://api.github.com/repos/");
    let parts: Vec<&str> = path.split('/').collect();

    if parts.len() >= 4 {
        let owner = parts[0];
        let repo = parts[1];
        let archive_type = parts[2];
        let reference = parts[3..].join("/");

        let ext = if archive_type == "tarball" {
            "legacy.tar.gz"
        } else {
            "legacy.zip"
        };

        format!(
            "https://codeload.github.com/{}/{}/{}/{}",
            owner, repo, ext, reference
        )
    } else {
        url.to_string()
    }
}
