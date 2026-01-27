//! About command - display information about Libretto.

use clap::Args;
use owo_colors::OwoColorize;

/// Arguments for the about command
#[derive(Args, Debug, Clone)]
pub struct AboutArgs {}

/// Run the about command
pub async fn run(_args: AboutArgs) -> anyhow::Result<()> {
    let colors = crate::output::colors_enabled();

    let logo = r#"
    __    _ __              __  __
   / /   (_) /_  ________  / /_/ /_____
  / /   / / __ \/ ___/ _ \/ __/ __/ __ \
 / /___/ / /_/ / /  /  __/ /_/ /_/ /_/ /
/_____/_/_.___/_/   \___/\__/\__/\____/
"#;

    if colors {
        println!("{}", logo.cyan().bold());
    } else {
        println!("{logo}");
    }

    println!();
    print_info("Libretto", env!("CARGO_PKG_VERSION"), colors);
    print_info(
        "A high-performance Composer-compatible package manager",
        "",
        colors,
    );
    println!();

    print_section("Features", colors);
    let features = [
        "Drop-in replacement for Composer",
        "Parallel package downloads with HTTP/2 multiplexing",
        "SIMD-accelerated JSON parsing and hashing",
        "Multi-tier caching with zstd compression",
        "PubGrub-based dependency resolution (from uv project)",
        "Zero-copy deserialization for cached data",
        "Cross-platform support (Linux, macOS, Windows)",
    ];
    for feature in &features {
        print_bullet(feature, colors);
    }
    println!();

    print_section("Links", colors);
    print_link(
        "Repository",
        "https://github.com/libretto-pm/libretto",
        colors,
    );
    print_link("Documentation", "https://libretto.dev/docs", colors);
    print_link(
        "Issues",
        "https://github.com/libretto-pm/libretto/issues",
        colors,
    );
    println!();

    print_section("License", colors);
    println!("  MIT OR Apache-2.0");
    println!();

    Ok(())
}

fn print_info(label: &str, value: &str, colors: bool) {
    if colors {
        if value.is_empty() {
            println!("  {}", label.dimmed());
        } else {
            println!("  {} {}", label.cyan().bold(), value.yellow());
        }
    } else if value.is_empty() {
        println!("  {label}");
    } else {
        println!("  {label} {value}");
    }
}

fn print_section(title: &str, colors: bool) {
    if colors {
        println!("{}", title.green().bold());
    } else {
        println!("{title}");
    }
}

fn print_bullet(text: &str, colors: bool) {
    let bullet = if crate::output::unicode_enabled() {
        "\u{2022}"
    } else {
        "*"
    };
    if colors {
        println!("  {} {}", bullet.green(), text);
    } else {
        println!("  {bullet} {text}");
    }
}

fn print_link(label: &str, url: &str, colors: bool) {
    if colors {
        println!("  {}: {}", label.cyan(), url.blue().underline());
    } else {
        println!("  {label}: {url}");
    }
}
