//! Dump-autoload command implementation.

use anyhow::Result;
use clap::Args;
use console::style;
use libretto_autoloader::AutoloaderGenerator;
use std::path::PathBuf;
use tracing::info;

/// Arguments for the dump-autoload command.
#[derive(Args, Debug)]
pub struct DumpAutoloadArgs {
    /// Optimize autoloader for production
    #[arg(short, long)]
    pub optimize: bool,

    /// Convert PSR-0/PSR-4 to classmap
    #[arg(short, long)]
    pub classmap_authoritative: bool,

    /// APCu caching
    #[arg(long)]
    pub apcu: bool,

    /// Don't scan for classes
    #[arg(long)]
    pub no_scripts: bool,
}

/// Run the dump-autoload command.
pub async fn run(args: DumpAutoloadArgs) -> Result<()> {
    info!("running dump-autoload command");

    println!(
        "{} {}",
        style("Libretto").cyan().bold(),
        style("Generating autoloader...").dim()
    );

    let vendor_dir = PathBuf::from("vendor");
    if !vendor_dir.exists() {
        std::fs::create_dir_all(&vendor_dir)?;
    }

    let generator = AutoloaderGenerator::new(vendor_dir);

    if args.optimize || args.classmap_authoritative {
        println!("{}", style("Generating optimized autoloader").dim());
    }

    match generator.generate() {
        Ok(()) => {
            println!(
                "{} Autoloader generated successfully",
                style("Success:").green().bold()
            );
        }
        Err(e) => {
            println!(
                "{} Failed to generate autoloader: {}",
                style("Error:").red().bold(),
                e
            );
        }
    }

    Ok(())
}
