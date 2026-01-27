//! Browse command - open package URLs in browser.

use anyhow::{Context, Result};
use clap::Args;
use sonic_rs::JsonValueTrait;

/// Arguments for the browse command
#[derive(Args, Debug, Clone)]
pub struct BrowseArgs {
    /// Package to browse (vendor/name format)
    #[arg(value_name = "PACKAGE")]
    pub package: Option<String>,

    /// Open the homepage instead of the repository URL
    #[arg(short = 'H', long)]
    pub homepage: bool,

    /// Only show the URL, don't open browser
    #[arg(short = 's', long)]
    pub show: bool,
}

/// Run the browse command
pub async fn run(args: BrowseArgs) -> Result<()> {
    use crate::output::{header, info, success};
    use libretto_repository::Repository;

    header("Opening package URL");

    // Get package name
    let package_name = if let Some(name) = &args.package {
        name.clone()
    } else {
        // Read from composer.json
        let composer_path = std::env::current_dir()?.join("composer.json");
        if !composer_path.exists() {
            anyhow::bail!("No package specified and no composer.json found");
        }
        let content = std::fs::read_to_string(&composer_path)?;
        let json: sonic_rs::Value = sonic_rs::from_str(&content)?;
        json.get("name")
            .and_then(|v| v.as_str())
            .map(String::from)
            .context("No package name found in composer.json")?
    };

    info(&format!("Looking up {}", package_name));

    // Fetch package info
    let repo = Repository::packagist()?;
    repo.init_packagist().await?;
    let package_id = libretto_core::PackageId::parse(&package_name)
        .ok_or_else(|| anyhow::anyhow!("Invalid package name: {}", package_name))?;
    let package = repo.get_package(&package_id).await?;

    // Get the first version to extract URLs
    let versions: Vec<_> = package.into_iter().collect();
    let first_version = versions.first().context("No versions found for package")?;

    // Helper to extract URL from PackageSource
    fn get_url_from_source(source: &libretto_core::PackageSource) -> String {
        match source {
            libretto_core::PackageSource::Git { url, .. } => url.to_string(),
            libretto_core::PackageSource::Dist { url, .. } => url.to_string(),
        }
    }

    // Determine URL to open - prefer source (git) for repository, dist for homepage-like
    let url = if args.homepage {
        // Try dist first, fall back to source
        first_version
            .dist
            .as_ref()
            .map(get_url_from_source)
            .or_else(|| first_version.source.as_ref().map(get_url_from_source))
            .context("No dist or source URL found")?
    } else {
        // Try source (git) first, fall back to dist
        first_version
            .source
            .as_ref()
            .map(get_url_from_source)
            .or_else(|| first_version.dist.as_ref().map(get_url_from_source))
            .context("No source or dist URL found")?
    };

    if args.show {
        println!("{url}");
    } else {
        success(&format!("Opening: {}", url));
        open_url(&url)?;
    }

    Ok(())
}

/// Open a URL in the default browser
fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("Failed to open URL with xdg-open")?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("Failed to open URL with open")?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .context("Failed to open URL with start")?;
    }

    Ok(())
}
