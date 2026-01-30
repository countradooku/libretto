//! Styling utilities for terminal output.

use owo_colors::OwoColorize;
use std::fmt;

/// Output mode for commands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// Normal human-friendly output
    #[default]
    Normal,
    /// Minimal output (only errors and essential info)
    Quiet,
    /// Detailed progress information
    Verbose,
    /// Full debugging information with timing
    Debug,
    /// Machine-readable JSON output
    Json,
}

impl OutputMode {
    /// Check if verbose or debug output is enabled
    pub const fn is_verbose(&self) -> bool {
        matches!(self, Self::Verbose | Self::Debug)
    }

    /// Check if debug output is enabled
    pub const fn is_debug(&self) -> bool {
        matches!(self, Self::Debug)
    }

    /// Check if quiet mode is enabled
    pub const fn is_quiet(&self) -> bool {
        matches!(self, Self::Quiet)
    }

    /// Check if JSON output is requested
    pub const fn is_json(&self) -> bool {
        matches!(self, Self::Json)
    }

    /// Check if human-friendly output should be shown
    pub const fn is_human(&self) -> bool {
        !self.is_json()
    }
}

/// Unicode/ASCII icons for terminal output
#[derive(Debug, Clone, Copy)]
pub enum Icon {
    Success,
    Error,
    Warning,
    Info,
    Arrow,
    Bullet,
    Package,
    Lock,
    Unlock,
    Download,
    Upload,
    Search,
    Check,
    Cross,
    Star,
    Heart,
    Lightning,
    Clock,
    Folder,
    File,
    Link,
    Security,
    Update,
    Add,
    Remove,
}

impl Icon {
    /// Get Unicode representation
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "\u{2714}",   // ✔
            Self::Error => "\u{2718}",     // ✘
            Self::Warning => "\u{26A0}",   // ⚠
            Self::Info => "\u{2139}",      // ℹ
            Self::Arrow => "\u{2192}",     // →
            Self::Bullet => "\u{2022}",    // •
            Self::Package => "\u{1F4E6}",  // 📦
            Self::Lock => "\u{1F512}",     // 🔒
            Self::Unlock => "\u{1F513}",   // 🔓
            Self::Download => "\u{2B07}",  // ⬇
            Self::Upload => "\u{2B06}",    // ⬆
            Self::Search => "\u{1F50D}",   // 🔍
            Self::Check => "\u{2713}",     // ✓
            Self::Cross => "\u{2717}",     // ✗
            Self::Star => "\u{2605}",      // ★
            Self::Heart => "\u{2665}",     // ♥
            Self::Lightning => "\u{26A1}", // ⚡
            Self::Clock => "\u{23F1}",     // ⏱
            Self::Folder => "\u{1F4C1}",   // 📁
            Self::File => "\u{1F4C4}",     // 📄
            Self::Link => "\u{1F517}",     // 🔗
            Self::Security => "\u{1F6E1}", // 🛡
            Self::Update => "\u{1F504}",   // 🔄
            Self::Add => "\u{2795}",       // ➕
            Self::Remove => "\u{2796}",    // ➖
        }
    }

    /// Get ASCII fallback
    pub const fn ascii(&self) -> &'static str {
        match self {
            Self::Success => "[OK]",
            Self::Error => "[ERR]",
            Self::Warning => "[WARN]",
            Self::Info => "[INFO]",
            Self::Arrow => "->",
            Self::Bullet => "*",
            Self::Package => "[PKG]",
            Self::Lock => "[LOCK]",
            Self::Unlock => "[UNLOCK]",
            Self::Download => "[DL]",
            Self::Upload => "[UL]",
            Self::Search => "[?]",
            Self::Check => "[v]",
            Self::Cross => "[x]",
            Self::Star => "[*]",
            Self::Heart => "<3",
            Self::Lightning => "[!]",
            Self::Clock => "[T]",
            Self::Folder => "[DIR]",
            Self::File => "[FILE]",
            Self::Link => "[LINK]",
            Self::Security => "[SEC]",
            Self::Update => "[UPD]",
            Self::Add => "[+]",
            Self::Remove => "[-]",
        }
    }
}

impl fmt::Display for Icon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            if crate::output::unicode_enabled() {
                self.as_str()
            } else {
                self.ascii()
            }
        )
    }
}

/// Theme for consistent styling
#[derive(Debug, Clone)]
pub struct Theme {
    pub colors_enabled: bool,
    pub unicode_enabled: bool,
}

impl Theme {
    /// Create a new theme
    pub const fn new(colors_enabled: bool, unicode_enabled: bool) -> Self {
        Self {
            colors_enabled,
            unicode_enabled,
        }
    }

    /// Create a theme from current settings
    pub fn from_env() -> Self {
        Self {
            colors_enabled: crate::output::colors_enabled(),
            unicode_enabled: crate::output::unicode_enabled(),
        }
    }

    /// Get an icon
    pub const fn icon(&self, icon: Icon) -> &'static str {
        if self.unicode_enabled {
            icon.as_str()
        } else {
            icon.ascii()
        }
    }

    /// Format a package name
    pub fn package(&self, name: &str) -> String {
        if self.colors_enabled {
            name.green().to_string()
        } else {
            name.to_string()
        }
    }

    /// Format a version
    pub fn version(&self, version: &str) -> String {
        if self.colors_enabled {
            version.yellow().to_string()
        } else {
            version.to_string()
        }
    }

    /// Format an error
    pub fn error(&self, text: &str) -> String {
        if self.colors_enabled {
            text.red().bold().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format a warning
    pub fn warning(&self, text: &str) -> String {
        if self.colors_enabled {
            text.yellow().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format success text
    pub fn success(&self, text: &str) -> String {
        if self.colors_enabled {
            text.green().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format dim/secondary text
    pub fn dim(&self, text: &str) -> String {
        if self.colors_enabled {
            text.dimmed().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format header text
    pub fn header(&self, text: &str) -> String {
        if self.colors_enabled {
            text.cyan().bold().to_string()
        } else {
            text.to_string()
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_env()
    }
}

/// Styled text wrapper
pub struct Styled<T> {
    value: T,
    style: owo_colors::Style,
    colors_enabled: bool,
}

impl<T: fmt::Display> Styled<T> {
    /// Create a new styled value
    pub const fn new(value: T, style: owo_colors::Style, colors_enabled: bool) -> Self {
        Self {
            value,
            style,
            colors_enabled,
        }
    }
}

impl<T: fmt::Display> fmt::Display for Styled<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.colors_enabled {
            write!(f, "{}", self.value.style(self.style))
        } else {
            write!(f, "{}", self.value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_mode() {
        assert!(OutputMode::Verbose.is_verbose());
        assert!(OutputMode::Debug.is_debug());
        assert!(OutputMode::Quiet.is_quiet());
        assert!(OutputMode::Json.is_json());
        assert!(OutputMode::Normal.is_human());
    }

    #[test]
    fn test_icon_display() {
        // Just ensure no panics
        for icon in [Icon::Success, Icon::Error, Icon::Warning] {
            let _ = icon.as_str();
            let _ = icon.ascii();
        }
    }
}
