//! CLI commands for Libretto.

pub mod audit;
pub mod cache_clear;
pub mod dump_autoload;
pub mod init;
pub mod install;
pub mod remove;
pub mod require;
pub mod search;
pub mod show;
pub mod update;
pub mod validate;

use clap::{Parser, Subcommand};

/// Libretto - A high-performance Composer-compatible package manager
#[derive(Parser, Debug)]
#[command(name = "libretto")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Working directory
    #[arg(short = 'd', long, global = true)]
    pub working_dir: Option<std::path::PathBuf>,

    /// Disable ANSI colors
    #[arg(long, global = true)]
    pub no_ansi: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// Available commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Install dependencies from composer.json/composer.lock
    Install(install::InstallArgs),

    /// Update dependencies to latest versions
    Update(update::UpdateArgs),

    /// Add a package to dependencies
    Require(require::RequireArgs),

    /// Remove a package from dependencies
    Remove(remove::RemoveArgs),

    /// Search for packages
    Search(search::SearchArgs),

    /// Show package information
    Show(show::ShowArgs),

    /// Initialize a new composer.json
    Init(init::InitArgs),

    /// Validate composer.json
    Validate(validate::ValidateArgs),

    /// Regenerate autoloader
    #[command(name = "dump-autoload", alias = "dumpautoload")]
    DumpAutoload(dump_autoload::DumpAutoloadArgs),

    /// Check for security vulnerabilities
    Audit(audit::AuditArgs),

    /// Clear the package cache
    #[command(name = "cache:clear", alias = "clearcache")]
    CacheClear(cache_clear::CacheClearArgs),
}
