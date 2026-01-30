//! Init command implementation.

use anyhow::Result;
use clap::Args;
use console::style;
use std::path::Path;
use tracing::info;

/// Arguments for the init command.
#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// Project name (vendor/name)
    #[arg(long)]
    pub name: Option<String>,

    /// Project description
    #[arg(long)]
    pub description: Option<String>,

    /// Author (name `<email>`)
    #[arg(long)]
    pub author: Option<String>,

    /// Package type
    #[arg(long, default_value = "library")]
    pub package_type: String,

    /// License
    #[arg(short, long, default_value = "MIT")]
    pub license: String,

    /// Minimum stability
    #[arg(long, default_value = "stable")]
    pub stability: String,
}

/// Run the init command.
pub async fn run(args: InitArgs) -> Result<()> {
    info!("running init command");

    let composer_json = Path::new("composer.json");
    if composer_json.exists() {
        println!(
            "{} composer.json already exists",
            style("Error:").red().bold()
        );
        return Ok(());
    }

    println!(
        "{} {}",
        style("Libretto").cyan().bold(),
        style("Initializing new project...").dim()
    );

    let name = args.name.unwrap_or_else(|| {
        let dir_name = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "my-project".to_string());
        format!("vendor/{dir_name}")
    });

    let content = format!(
        r#"{{
    "name": "{}",
    "description": "{}",
    "type": "{}",
    "license": "{}",
    "minimum-stability": "{}",
    "require": {{
        "php": ">=8.1"
    }},
    "autoload": {{
        "psr-4": {{
            "App\\": "src/"
        }}
    }},
    "autoload-dev": {{
        "psr-4": {{
            "App\\Tests\\": "tests/"
        }}
    }}
}}
"#,
        name,
        args.description.unwrap_or_default(),
        args.package_type,
        args.license,
        args.stability
    );

    std::fs::write(composer_json, content)?;

    println!(
        "{} Created {}",
        style("Success:").green().bold(),
        style("composer.json").cyan()
    );

    // Create src and tests directories
    std::fs::create_dir_all("src")?;
    std::fs::create_dir_all("tests")?;

    println!(
        "{} Created src/ directory",
        style("Success:").green().bold()
    );
    println!(
        "{} Created tests/ directory",
        style("Success:").green().bold()
    );

    Ok(())
}
