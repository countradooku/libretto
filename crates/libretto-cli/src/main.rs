//! Libretto CLI - A high-performance Composer-compatible package manager.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

mod commands;

use clap::Parser;
use commands::{Cli, Commands};
use console::style;
use std::process::ExitCode;
use tracing::Level;
use tracing_subscriber::EnvFilter;

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = EnvFilter::builder()
        .with_default_directive(
            if cli.verbose {
                Level::DEBUG
            } else {
                Level::INFO
            }
            .into(),
        )
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .init();

    // Run the command
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create runtime");

    match runtime.block_on(run_command(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e}", style("error:").red().bold());
            ExitCode::FAILURE
        }
    }
}

async fn run_command(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Install(args) => commands::install::run(args).await,
        Commands::Update(args) => commands::update::run(args).await,
        Commands::Require(args) => commands::require::run(args).await,
        Commands::Remove(args) => commands::remove::run(args).await,
        Commands::Search(args) => commands::search::run(args).await,
        Commands::Show(args) => commands::show::run(args).await,
        Commands::Init(args) => commands::init::run(args).await,
        Commands::Validate(args) => commands::validate::run(args).await,
        Commands::DumpAutoload(args) => commands::dump_autoload::run(args).await,
        Commands::Audit(args) => commands::audit::run(args).await,
        Commands::CacheClear(args) => commands::cache_clear::run(args).await,
    }
}
