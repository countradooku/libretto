//! Color configuration and utilities.

use owo_colors::{OwoColorize, Style};

/// Color configuration based on terminal capabilities
#[derive(Debug, Clone, Copy)]
pub struct ColorConfig {
    pub enabled: bool,
}

impl ColorConfig {
    /// Create a new color config
    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Check if colors are enabled
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Predefined color styles for consistent output
#[derive(Debug, Clone, Copy)]
pub struct Colors;

impl Colors {
    /// Style for package names
    pub const fn package() -> Style {
        Style::new().green()
    }

    /// Style for version strings
    pub const fn version() -> Style {
        Style::new().yellow()
    }

    /// Style for success messages
    pub const fn success() -> Style {
        Style::new().green().bold()
    }

    /// Style for error messages
    pub const fn error() -> Style {
        Style::new().red().bold()
    }

    /// Style for warning messages
    pub const fn warning() -> Style {
        Style::new().yellow()
    }

    /// Style for info messages
    pub const fn info() -> Style {
        Style::new().blue()
    }

    /// Style for debug/dim messages
    pub const fn dim() -> Style {
        Style::new().dimmed()
    }

    /// Style for headers
    pub const fn header() -> Style {
        Style::new().cyan().bold()
    }

    /// Style for commands
    pub const fn command() -> Style {
        Style::new().magenta()
    }

    /// Style for paths
    pub const fn path() -> Style {
        Style::new().cyan()
    }

    /// Style for URLs
    pub const fn url() -> Style {
        Style::new().blue().underline()
    }

    /// Style for numbers/counts
    pub const fn number() -> Style {
        Style::new().bright_white().bold()
    }

    /// Style for critical severity
    pub const fn critical() -> Style {
        Style::new().bright_red().bold()
    }

    /// Style for high severity
    pub const fn high() -> Style {
        Style::new().red()
    }

    /// Style for medium severity
    pub const fn medium() -> Style {
        Style::new().yellow()
    }

    /// Style for low severity
    pub const fn low() -> Style {
        Style::new().blue()
    }

    /// Style for added items
    pub const fn added() -> Style {
        Style::new().green()
    }

    /// Style for removed items
    pub const fn removed() -> Style {
        Style::new().red()
    }

    /// Style for changed items
    pub const fn changed() -> Style {
        Style::new().yellow()
    }
}

/// Apply a style conditionally based on colors being enabled
pub fn styled<T: std::fmt::Display>(value: T, style: Style, colors_enabled: bool) -> String {
    if colors_enabled {
        format!("{}", value.style(style))
    } else {
        value.to_string()
    }
}

/// Colorize text with a named color
pub fn colorize(text: &str, color: &str, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }

    match color {
        "red" => text.red().to_string(),
        "green" => text.green().to_string(),
        "yellow" => text.yellow().to_string(),
        "blue" => text.blue().to_string(),
        "magenta" => text.magenta().to_string(),
        "cyan" => text.cyan().to_string(),
        "white" => text.white().to_string(),
        "dim" => text.dimmed().to_string(),
        "bold" => text.bold().to_string(),
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_styled_enabled() {
        let result = styled("test", Colors::success(), true);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_styled_disabled() {
        let result = styled("test", Colors::success(), false);
        assert_eq!(result, "test");
    }

    #[test]
    fn test_colorize() {
        assert_eq!(colorize("test", "red", false), "test");
        assert!(!colorize("test", "red", true).is_empty());
    }
}
