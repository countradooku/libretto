//! Cache commands - manage the package cache.

use crate::cas_cache;
use crate::output::format_bytes;
use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

/// Arguments for the cache:clear command
#[derive(Args, Debug, Clone)]
pub struct CacheClearArgs {
    /// Only clear the packages cache
    #[arg(long)]
    pub packages: bool,

    /// Only clear the repository cache
    #[arg(long)]
    pub repo: bool,

    /// Only clear the VCS cache
    #[arg(long)]
    pub vcs: bool,

    /// Only clear the content-addressable storage (CAS) cache
    #[arg(long)]
    pub cas: bool,

    /// Garbage collect expired cache entries
    #[arg(long)]
    pub gc: bool,

    /// Maximum age for cache entries when using --gc (e.g., "30d", "12h")
    #[arg(long, default_value = "30d")]
    pub max_age: String,
}

/// Arguments for the cache:list command
#[derive(Args, Debug, Clone)]
pub struct CacheListArgs {
    /// Only list packages cache
    #[arg(long)]
    pub packages: bool,

    /// Only list repository cache
    #[arg(long)]
    pub repo: bool,

    /// Only list VCS cache
    #[arg(long)]
    pub vcs: bool,
}

/// Run the cache:clear command
pub async fn run_clear(args: CacheClearArgs) -> Result<()> {
    use crate::output::{format_bytes, header, info, success};
    use libretto_cache::Cache;

    header("Clearing cache");

    let cache = Cache::new()?;
    let clear_all = !args.packages && !args.repo && !args.vcs && !args.cas && !args.gc;

    let mut total_cleared: u64 = 0;

    if args.gc {
        info("Garbage collecting expired cache entries...");
        let max_age = parse_duration(&args.max_age)?;
        let cleared = cache.prune(max_age.as_secs() as i64 / 86400)?;
        total_cleared += cleared as u64;
        info(&format!(
            "Garbage collected: {}",
            format_bytes(cleared as u64)
        ));
    }

    if clear_all || args.packages {
        info("Clearing packages cache...");
        let packages_dir = get_cache_dir()?.join("packages");
        let cleared = clear_directory(&packages_dir)?;
        total_cleared += cleared;
        info(&format!(
            "Packages cache cleared: {}",
            format_bytes(cleared)
        ));
    }

    if clear_all || args.repo {
        info("Clearing repository cache...");
        let repo_dir = get_cache_dir()?.join("repo");
        let cleared = clear_directory(&repo_dir)?;
        total_cleared += cleared;
        info(&format!(
            "Repository cache cleared: {}",
            format_bytes(cleared)
        ));
    }

    if clear_all || args.vcs {
        info("Clearing VCS cache...");
        let vcs_dir = get_cache_dir()?.join("vcs");
        let cleared = clear_directory(&vcs_dir)?;
        total_cleared += cleared;
        info(&format!("VCS cache cleared: {}", format_bytes(cleared)));
    }

    if clear_all || args.cas {
        info("Clearing CAS cache...");
        let cas_dir = cas_cache::cas_dir();
        let cleared = clear_directory(&cas_dir)?;
        total_cleared += cleared;
        cas_cache::clear_cache()?;
        info(&format!("CAS cache cleared: {}", format_bytes(cleared)));
    }

    success(&format!("Total cleared: {}", format_bytes(total_cleared)));

    // Show remaining cache size
    let remaining = get_cache_size()?;
    info(&format!(
        "Remaining cache size: {}",
        format_bytes(remaining)
    ));

    Ok(())
}

/// Run the cache:list command
pub async fn run_list(args: CacheListArgs) -> Result<()> {
    use crate::output::table::Table;
    use crate::output::{header, info};

    header("Cache contents");

    let cache_dir = get_cache_dir()?;
    let list_all = !args.packages && !args.repo && !args.vcs;

    if list_all || args.packages {
        let packages_dir = cache_dir.join("packages");
        if packages_dir.exists() {
            info("Packages cache:");
            list_cache_directory(&packages_dir)?;
        }
    }

    if list_all || args.repo {
        let repo_dir = cache_dir.join("repo");
        if repo_dir.exists() {
            info("Repository cache:");
            list_cache_directory(&repo_dir)?;
        }
    }

    if list_all || args.vcs {
        let vcs_dir = cache_dir.join("vcs");
        if vcs_dir.exists() {
            info("VCS cache:");
            list_cache_directory(&vcs_dir)?;
        }
    }

    // Summary
    println!();
    let mut table = Table::new();
    table.headers(["Cache Type", "Size", "Files"]);

    if list_all || args.packages {
        let (size, count) = dir_stats(&cache_dir.join("packages"))?;
        table.row(["Packages", &format_bytes(size), &count.to_string()]);
    }

    if list_all || args.repo {
        let (size, count) = dir_stats(&cache_dir.join("repo"))?;
        table.row(["Repository", &format_bytes(size), &count.to_string()]);
    }

    if list_all || args.vcs {
        let (size, count) = dir_stats(&cache_dir.join("vcs"))?;
        table.row(["VCS", &format_bytes(size), &count.to_string()]);
    }

    // Add CAS cache stats
    let cas_size = cas_cache::cache_size();
    let cas_count = cas_cache::cached_package_count().unwrap_or(0);
    table.row([
        "CAS (hardlinks)",
        &format_bytes(cas_size),
        &cas_count.to_string(),
    ]);

    let (total_size, total_count) = dir_stats(&cache_dir)?;
    let total_with_cas = total_size + cas_size;
    table.row([
        "Total",
        &format_bytes(total_with_cas),
        &(total_count + cas_count).to_string(),
    ]);

    table.print();

    Ok(())
}

// Keep the old run function for backwards compatibility

/// Get the cache directory
fn get_cache_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "libretto")
        .ok_or_else(|| anyhow::anyhow!("Could not determine cache directory"))?;
    Ok(dirs.cache_dir().to_path_buf())
}

/// Clear a directory and return bytes freed
fn clear_directory(path: &PathBuf) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    let mut total: u64 = 0;

    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if entry.file_type().is_file()
            && let Ok(meta) = entry.metadata()
        {
            total += meta.len();
        }
    }

    std::fs::remove_dir_all(path)?;
    std::fs::create_dir_all(path)?;

    Ok(total)
}

/// Get total cache size
fn get_cache_size() -> Result<u64> {
    let cache_dir = get_cache_dir()?;
    let (size, _) = dir_stats(&cache_dir)?;
    Ok(size)
}

/// Get directory statistics
fn dir_stats(path: &PathBuf) -> Result<(u64, usize)> {
    if !path.exists() {
        return Ok((0, 0));
    }

    let mut size: u64 = 0;
    let mut count: usize = 0;

    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if entry.file_type().is_file()
            && let Ok(meta) = entry.metadata()
        {
            size += meta.len();
            count += 1;
        }
    }

    Ok((size, count))
}

/// List cache directory contents
fn list_cache_directory(path: &PathBuf) -> Result<()> {
    use crate::output::format_bytes;
    use owo_colors::OwoColorize;

    if !path.exists() {
        println!("  (empty)");
        return Ok(());
    }

    let colors = crate::output::colors_enabled();
    let mut entries: Vec<_> = std::fs::read_dir(path)?
        .filter_map(std::result::Result::ok)
        .collect();

    entries.sort_by_key(std::fs::DirEntry::file_name);

    let max_display = 20;
    let total = entries.len();

    for entry in entries.iter().take(max_display) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let meta = entry.metadata()?;

        if meta.is_dir() {
            let (size, count) = dir_stats(&entry.path())?;
            if colors {
                println!(
                    "  {} {} ({} files)",
                    name_str.cyan(),
                    format_bytes(size).dimmed(),
                    count
                );
            } else {
                println!("  {name_str} {} ({count} files)", format_bytes(size));
            }
        } else {
            let size = format_bytes(meta.len());
            if colors {
                println!("  {} {}", name_str.green(), size.dimmed());
            } else {
                println!("  {name_str} {size}");
            }
        }
    }

    if total > max_display {
        println!("  ... and {} more entries", total - max_display);
    }

    Ok(())
}

/// Parse a duration string (e.g., "30d", "12h", "45m")
fn parse_duration(s: &str) -> Result<std::time::Duration> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.len().saturating_sub(1));
    let num: u64 = num.parse().unwrap_or(30);

    let secs = match unit {
        "s" => num,
        "m" => num * 60,
        "h" => num * 3600,
        "d" => num * 86400,
        "w" => num * 604_800,
        _ => {
            // Assume days if no unit
            let num: u64 = s.parse().unwrap_or(30);
            num * 86400
        }
    };

    Ok(std::time::Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("30d").unwrap().as_secs(), 30 * 86400);
        assert_eq!(parse_duration("12h").unwrap().as_secs(), 12 * 3600);
        assert_eq!(parse_duration("30").unwrap().as_secs(), 30 * 86400);
    }
}
