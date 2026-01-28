//! CLI context for shared state across commands.
//!
//! This module provides context utilities (Timer, profiling, etc.) that are
//! designed for use across all CLI commands. Some utilities may not be used
//! yet but are kept for completeness and future use.

#![allow(dead_code)]

use crate::output::{OutputMode, Theme};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// Global CLI context shared across all commands
#[derive(Debug, Clone)]
pub struct Context {
    /// Working directory
    pub working_dir: PathBuf,
    /// Output mode (normal, quiet, verbose, debug, json)
    pub output_mode: OutputMode,
    /// Whether colors are enabled
    pub colors_enabled: bool,
    /// Whether Unicode is enabled
    pub unicode_enabled: bool,
    /// Whether plugins are enabled
    pub plugins_enabled: bool,
    /// Whether scripts are enabled
    pub scripts_enabled: bool,
    /// Whether cache is enabled
    pub cache_enabled: bool,
    /// Whether to run in non-interactive mode
    pub non_interactive: bool,
    /// Whether to show profiling information
    pub profile: bool,
    /// Start time for profiling
    pub start_time: Instant,
    /// Theme for output styling
    pub theme: Theme,
}

impl Context {
    /// Create a new context from CLI arguments
    pub fn new(args: &ContextArgs) -> anyhow::Result<Self> {
        let working_dir = if let Some(dir) = &args.working_dir {
            std::fs::canonicalize(dir)?
        } else {
            std::env::current_dir()?
        };

        // Change to working directory if specified
        if args.working_dir.is_some() {
            std::env::set_current_dir(&working_dir)?;
        }

        // Determine output mode from verbosity
        let output_mode = match args.verbosity {
            0 if args.quiet => OutputMode::Quiet,
            0 => OutputMode::Normal,
            1 => OutputMode::Verbose,
            _ => OutputMode::Debug,
        };

        // Initialize output settings
        let force_ansi = match (args.ansi, args.no_ansi) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        };
        crate::output::init(force_ansi, args.quiet);

        let colors_enabled = crate::output::colors_enabled();
        let unicode_enabled = crate::output::unicode_enabled();

        Ok(Self {
            working_dir,
            output_mode,
            colors_enabled,
            unicode_enabled,
            plugins_enabled: !args.no_plugins,
            scripts_enabled: !args.no_scripts,
            cache_enabled: !args.no_cache,
            non_interactive: args.no_interaction,
            profile: args.profile,
            start_time: Instant::now(),
            theme: Theme::new(colors_enabled, unicode_enabled),
        })
    }

    /// Get the path to composer.json
    pub fn composer_json_path(&self) -> PathBuf {
        self.working_dir.join("composer.json")
    }

    /// Get the path to composer.lock
    pub fn composer_lock_path(&self) -> PathBuf {
        self.working_dir.join("composer.lock")
    }

    /// Get the vendor directory path
    pub fn vendor_dir(&self) -> PathBuf {
        self.working_dir.join("vendor")
    }

    /// Check if composer.json exists
    pub fn has_composer_json(&self) -> bool {
        self.composer_json_path().exists()
    }

    /// Check if composer.lock exists
    pub fn has_composer_lock(&self) -> bool {
        self.composer_lock_path().exists()
    }

    /// Check if verbose output is enabled
    pub fn is_verbose(&self) -> bool {
        self.output_mode.is_verbose()
    }

    /// Check if debug output is enabled
    pub fn is_debug(&self) -> bool {
        self.output_mode.is_debug()
    }

    /// Check if quiet mode is enabled
    pub fn is_quiet(&self) -> bool {
        self.output_mode.is_quiet()
    }

    /// Check if JSON output is requested
    pub fn is_json(&self) -> bool {
        self.output_mode.is_json()
    }

    /// Print profiling information if enabled
    pub fn print_profile(&self, label: &str) {
        if self.profile {
            let elapsed = self.start_time.elapsed();
            let memory = get_memory_usage();
            eprintln!(
                "[profile] {}: {} | Memory: {}",
                label,
                crate::output::format_duration(elapsed),
                crate::output::format_bytes(memory)
            );
        }
    }

    /// Create a timer for profiling
    pub fn timer(&self, label: &'static str) -> Timer {
        Timer::new(label, self.profile)
    }
}

/// Arguments that affect context creation
#[derive(Debug, Clone, Default)]
pub struct ContextArgs {
    /// Working directory
    pub working_dir: Option<PathBuf>,
    /// Verbosity level (0 = normal, 1 = verbose, 2+ = debug)
    pub verbosity: u8,
    /// Quiet mode
    pub quiet: bool,
    /// Force ANSI colors
    pub ansi: bool,
    /// Disable ANSI colors
    pub no_ansi: bool,
    /// Disable plugins
    pub no_plugins: bool,
    /// Disable scripts
    pub no_scripts: bool,
    /// Disable cache
    pub no_cache: bool,
    /// Non-interactive mode
    pub no_interaction: bool,
    /// Show profiling information
    pub profile: bool,
}

/// Timer for profiling operations
pub struct Timer {
    label: &'static str,
    start: Instant,
    enabled: bool,
}

impl Timer {
    /// Create a new timer
    pub fn new(label: &'static str, enabled: bool) -> Self {
        Self {
            label,
            start: Instant::now(),
            enabled,
        }
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> std::time::Duration {
        self.start.elapsed()
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if self.enabled {
            eprintln!(
                "[profile] {}: {}",
                self.label,
                crate::output::format_duration(self.elapsed())
            );
        }
    }
}

/// Get current memory usage (platform-specific)
fn get_memory_usage() -> u64 {
    #[cfg(target_os = "linux")]
    {
        // Read from /proc/self/statm
        std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|s| s.split_whitespace().next().map(String::from))
            .and_then(|s| s.parse::<u64>().ok())
            .map(|pages| pages * 4096) // Page size
            .unwrap_or(0)
    }

    #[cfg(target_os = "macos")]
    {
        // Use mach task_info (simplified)
        0
    }

    #[cfg(target_os = "windows")]
    {
        // Use GetProcessMemoryInfo (simplified)
        0
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        0
    }
}

/// Shared context wrapped in Arc for async operations
pub type SharedContext = Arc<Context>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let args = ContextArgs::default();
        let ctx = Context::new(&args).unwrap();
        assert!(ctx.working_dir.exists());
    }

    #[test]
    fn test_context_paths() {
        let args = ContextArgs::default();
        let ctx = Context::new(&args).unwrap();
        assert!(ctx.composer_json_path().ends_with("composer.json"));
        assert!(ctx.composer_lock_path().ends_with("composer.lock"));
        assert!(ctx.vendor_dir().ends_with("vendor"));
    }
}
