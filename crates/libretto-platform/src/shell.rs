//! Cross-platform shell integration.
//!
//! Provides unified interface for:
//! - Unix: bash, zsh, fish, sh
//! - Windows: cmd.exe, `PowerShell`

use crate::process::{ProcessBuilder, ProcessOutput, StdioConfig};
use crate::{Os, Platform, PlatformError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Shell types supported across platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellType {
    /// Bourne shell (sh).
    Sh,
    /// Bourne-again shell (bash).
    Bash,
    /// Z shell (zsh).
    Zsh,
    /// Fish shell.
    Fish,
    /// Windows Command Prompt.
    Cmd,
    /// `PowerShell` (Windows or cross-platform).
    PowerShell,
    /// `PowerShell` Core (cross-platform).
    Pwsh,
}

impl ShellType {
    /// Detect the default shell for the current platform.
    #[must_use]
    pub fn detect(os: &Os) -> Self {
        match os {
            Os::Windows => Self::Cmd,
            Os::Linux | Os::MacOs => Self::detect_unix_shell(),
            Os::Unknown => Self::Sh,
        }
    }

    /// Detect Unix shell from environment.
    fn detect_unix_shell() -> Self {
        // Check SHELL environment variable
        if let Ok(shell) = std::env::var("SHELL") {
            if shell.contains("zsh") {
                return Self::Zsh;
            }
            if shell.contains("fish") {
                return Self::Fish;
            }
            if shell.contains("bash") {
                return Self::Bash;
            }
        }

        // Default to bash on Unix
        Self::Bash
    }

    /// Get the shell executable name.
    #[must_use]
    pub const fn executable(&self) -> &'static str {
        match self {
            Self::Sh => "sh",
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Cmd => "cmd.exe",
            Self::PowerShell => "powershell.exe",
            Self::Pwsh => "pwsh",
        }
    }

    /// Get the command flag for executing a command string.
    #[must_use]
    pub const fn command_flag(&self) -> &'static str {
        match self {
            Self::Sh | Self::Bash | Self::Zsh => "-c",
            Self::Fish => "-c",
            Self::Cmd => "/C",
            Self::PowerShell | Self::Pwsh => "-Command",
        }
    }

    /// Check if this shell is available on the current system.
    #[must_use]
    pub fn is_available(&self) -> bool {
        crate::process::which(self.executable()).is_some()
    }

    /// Check if this is a Unix shell.
    #[must_use]
    pub const fn is_unix(&self) -> bool {
        matches!(self, Self::Sh | Self::Bash | Self::Zsh | Self::Fish)
    }

    /// Check if this is a Windows shell.
    #[must_use]
    pub const fn is_windows(&self) -> bool {
        matches!(self, Self::Cmd | Self::PowerShell | Self::Pwsh)
    }

    /// Get the comment prefix for this shell.
    #[must_use]
    pub const fn comment_prefix(&self) -> &'static str {
        match self {
            Self::Sh | Self::Bash | Self::Zsh | Self::Fish | Self::Pwsh => "#",
            Self::Cmd => "REM",
            Self::PowerShell => "#",
        }
    }

    /// Get the environment variable syntax for this shell.
    #[must_use]
    pub fn env_syntax(&self, name: &str) -> String {
        match self {
            Self::Sh | Self::Bash | Self::Zsh | Self::Fish => format!("${name}"),
            Self::Cmd => format!("%{name}%"),
            Self::PowerShell | Self::Pwsh => format!("$env:{name}"),
        }
    }

    /// Get the path separator for this shell.
    #[must_use]
    pub const fn path_separator(&self) -> char {
        match self {
            Self::Sh | Self::Bash | Self::Zsh | Self::Fish | Self::Pwsh => ':',
            Self::Cmd | Self::PowerShell => ';',
        }
    }
}

impl std::fmt::Display for ShellType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.executable())
    }
}

/// Shell command builder.
#[derive(Debug, Clone)]
pub struct ShellCommand {
    /// Shell type to use.
    shell: ShellType,
    /// Command to execute.
    command: String,
    /// Working directory.
    cwd: Option<PathBuf>,
    /// Environment variables.
    env: HashMap<String, String>,
    /// Whether to inherit environment.
    inherit_env: bool,
    /// Capture output.
    capture: bool,
    /// Timeout.
    timeout: Option<std::time::Duration>,
}

impl ShellCommand {
    /// Create a new shell command using the default shell.
    #[must_use]
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            shell: Platform::current().default_shell,
            command: command.into(),
            cwd: None,
            env: HashMap::new(),
            inherit_env: true,
            capture: true,
            timeout: None,
        }
    }

    /// Create a command for a specific shell.
    #[must_use]
    pub fn with_shell(shell: ShellType, command: impl Into<String>) -> Self {
        Self {
            shell,
            command: command.into(),
            cwd: None,
            env: HashMap::new(),
            inherit_env: true,
            capture: true,
            timeout: None,
        }
    }

    /// Set the working directory.
    #[must_use]
    pub fn cwd(mut self, dir: impl AsRef<Path>) -> Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Set an environment variable.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set multiple environment variables.
    #[must_use]
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (k, v) in vars {
            self.env.insert(k.into(), v.into());
        }
        self
    }

    /// Don't inherit parent environment.
    #[must_use]
    pub const fn no_inherit_env(mut self) -> Self {
        self.inherit_env = false;
        self
    }

    /// Don't capture output (inherit stdio).
    #[must_use]
    pub const fn no_capture(mut self) -> Self {
        self.capture = false;
        self
    }

    /// Set timeout.
    #[must_use]
    pub const fn timeout(mut self, duration: std::time::Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Execute the command.
    ///
    /// # Errors
    /// Returns error if shell is not available or command fails.
    pub fn run(self) -> Result<ProcessOutput> {
        if !self.shell.is_available() {
            return Err(PlatformError::ShellNotFound {
                shell: self.shell.executable().to_string(),
            });
        }

        let mut builder = ProcessBuilder::new(self.shell.executable())
            .arg(self.shell.command_flag())
            .arg(&self.command);

        if let Some(cwd) = self.cwd {
            builder = builder.cwd(cwd);
        }

        if !self.inherit_env {
            builder = builder.env_clear();
        }

        for (k, v) in self.env {
            builder = builder.env(k, v);
        }

        if self.capture {
            builder = builder.capture();
        } else {
            builder = builder.stdin(StdioConfig::Inherit);
        }

        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }

        builder.run()
    }

    /// Execute and check for success.
    ///
    /// # Errors
    /// Returns error if command fails.
    pub fn status(self) -> Result<()> {
        let output = self.run()?;
        if output.success() {
            Ok(())
        } else {
            Err(PlatformError::ShellCommand(format!(
                "Command failed with exit code {}: {}",
                output.code().unwrap_or(-1),
                output.stderr_trimmed()
            )))
        }
    }

    /// Execute and return stdout.
    ///
    /// # Errors
    /// Returns error if command fails.
    pub fn output(self) -> Result<String> {
        let output = self.run()?;
        output.into_result().map(|o| o.stdout_trimmed())
    }
}

/// Shell abstraction for interactive use.
#[derive(Debug, Clone)]
pub struct Shell {
    /// Shell type.
    shell_type: ShellType,
    /// Shell path.
    path: PathBuf,
}

impl Shell {
    /// Create a new shell instance.
    ///
    /// # Errors
    /// Returns error if shell is not found.
    pub fn new(shell_type: ShellType) -> Result<Self> {
        let path = crate::process::which(shell_type.executable()).ok_or_else(|| {
            PlatformError::ShellNotFound {
                shell: shell_type.executable().to_string(),
            }
        })?;

        Ok(Self { shell_type, path })
    }

    /// Get the system default shell.
    ///
    /// # Errors
    /// Returns error if default shell is not found.
    pub fn system_default() -> Result<Self> {
        Self::new(Platform::current().default_shell)
    }

    /// Get bash shell.
    ///
    /// # Errors
    /// Returns error if bash is not found.
    pub fn bash() -> Result<Self> {
        Self::new(ShellType::Bash)
    }

    /// Get zsh shell.
    ///
    /// # Errors
    /// Returns error if zsh is not found.
    pub fn zsh() -> Result<Self> {
        Self::new(ShellType::Zsh)
    }

    /// Get fish shell.
    ///
    /// # Errors
    /// Returns error if fish is not found.
    pub fn fish() -> Result<Self> {
        Self::new(ShellType::Fish)
    }

    /// Get `PowerShell`.
    ///
    /// # Errors
    /// Returns error if `PowerShell` is not found.
    pub fn powershell() -> Result<Self> {
        // Try pwsh first (cross-platform), then powershell.exe (Windows)
        Self::new(ShellType::Pwsh).or_else(|_| Self::new(ShellType::PowerShell))
    }

    /// Get shell type.
    #[must_use]
    pub const fn shell_type(&self) -> ShellType {
        self.shell_type
    }

    /// Get shell path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Execute a command.
    ///
    /// # Errors
    /// Returns error if command fails.
    pub fn exec(&self, command: &str) -> Result<ProcessOutput> {
        ShellCommand::with_shell(self.shell_type, command).run()
    }

    /// Execute a command and return stdout.
    ///
    /// # Errors
    /// Returns error if command fails.
    pub fn output(&self, command: &str) -> Result<String> {
        ShellCommand::with_shell(self.shell_type, command).output()
    }

    /// Execute a command interactively (no capture).
    ///
    /// # Errors
    /// Returns error if command fails.
    pub fn interactive(&self, command: &str) -> Result<ProcessOutput> {
        ShellCommand::with_shell(self.shell_type, command)
            .no_capture()
            .run()
    }

    /// Source a script (bash/zsh only).
    ///
    /// # Errors
    /// Returns error if sourcing fails.
    pub fn source(&self, script: &Path) -> Result<ProcessOutput> {
        let cmd = match self.shell_type {
            ShellType::Bash | ShellType::Zsh | ShellType::Sh => {
                format!(". {}", script.display())
            }
            ShellType::Fish => {
                format!("source {}", script.display())
            }
            ShellType::Cmd => {
                format!("call {}", script.display())
            }
            ShellType::PowerShell | ShellType::Pwsh => {
                format!(". {}", script.display())
            }
        };
        self.exec(&cmd)
    }

    /// Get environment variable value from shell.
    ///
    /// # Errors
    /// Returns error if variable cannot be retrieved.
    pub fn get_env(&self, name: &str) -> Result<Option<String>> {
        let cmd = match self.shell_type {
            ShellType::Sh | ShellType::Bash | ShellType::Zsh | ShellType::Fish => {
                format!("echo \"${name}\"")
            }
            ShellType::Cmd => {
                format!("echo %{name}%")
            }
            ShellType::PowerShell | ShellType::Pwsh => {
                format!("echo $env:{name}")
            }
        };

        let output = self.output(&cmd)?;
        let value = output.trim();

        // Check if variable was unset
        if value.is_empty() || value == format!("%{name}%") || value == format!("${name}") {
            Ok(None)
        } else {
            Ok(Some(value.to_string()))
        }
    }

    /// Set environment variable in a script context.
    #[must_use]
    pub fn export_syntax(&self, name: &str, value: &str) -> String {
        match self.shell_type {
            ShellType::Sh | ShellType::Bash | ShellType::Zsh => {
                format!("export {name}=\"{value}\"")
            }
            ShellType::Fish => {
                format!("set -gx {name} \"{value}\"")
            }
            ShellType::Cmd => {
                format!("set {name}={value}")
            }
            ShellType::PowerShell | ShellType::Pwsh => {
                format!("$env:{name} = \"{value}\"")
            }
        }
    }
}

/// Parse a shebang line from a script.
#[must_use]
pub fn parse_shebang(script: &Path) -> Option<(PathBuf, Vec<String>)> {
    let content = std::fs::read_to_string(script).ok()?;
    parse_shebang_line(content.lines().next()?)
}

/// Parse a shebang line.
#[must_use]
pub fn parse_shebang_line(line: &str) -> Option<(PathBuf, Vec<String>)> {
    let line = line.trim();
    if !line.starts_with("#!") {
        return None;
    }

    let content = line[2..].trim();
    let parts: Vec<&str> = content.split_whitespace().collect();

    if parts.is_empty() {
        return None;
    }

    let interpreter = PathBuf::from(parts[0]);
    let args: Vec<String> = parts[1..].iter().map(|&s| s.to_string()).collect();

    // Handle /usr/bin/env style shebangs
    if parts[0].ends_with("/env") && !args.is_empty() {
        let actual_interpreter = PathBuf::from(&args[0]);
        let actual_args = args[1..].to_vec();
        Some((actual_interpreter, actual_args))
    } else {
        Some((interpreter, args))
    }
}

/// Escape a string for shell usage.
#[must_use]
pub fn escape_shell_arg(arg: &str, shell: ShellType) -> String {
    match shell {
        ShellType::Sh | ShellType::Bash | ShellType::Zsh | ShellType::Fish => {
            escape_posix_shell_arg(arg)
        }
        ShellType::Cmd => escape_cmd_arg(arg),
        ShellType::PowerShell | ShellType::Pwsh => escape_powershell_arg(arg),
    }
}

fn escape_posix_shell_arg(arg: &str) -> String {
    // If the argument is simple, no escaping needed
    if arg
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return arg.to_string();
    }

    // Use single quotes, escaping any single quotes within
    let escaped = arg.replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn escape_cmd_arg(arg: &str) -> String {
    // CMD escaping is complex; use double quotes
    if arg
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '\\')
    {
        return arg.to_string();
    }

    // Escape special characters
    let escaped = arg
        .replace('^', "^^")
        .replace('&', "^&")
        .replace('<', "^<")
        .replace('>', "^>")
        .replace('|', "^|")
        .replace('%', "%%");

    format!("\"{escaped}\"")
}

fn escape_powershell_arg(arg: &str) -> String {
    // PowerShell escaping
    if arg
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '\\')
    {
        return arg.to_string();
    }

    // Use single quotes, doubling any single quotes within
    let escaped = arg.replace('\'', "''");
    format!("'{escaped}'")
}

/// Join arguments for shell command line.
#[must_use]
pub fn join_args<I, S>(args: I, shell: ShellType) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .map(|s| escape_shell_arg(s.as_ref(), shell))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_type_detection() {
        let shell = ShellType::detect(&Os::current());
        // Shell may not be available in CI, so we just verify detection doesn't panic
        let _ = shell.is_available();
    }

    #[test]
    fn shell_type_executable() {
        assert_eq!(ShellType::Bash.executable(), "bash");
        assert_eq!(ShellType::Cmd.executable(), "cmd.exe");
        assert_eq!(ShellType::PowerShell.executable(), "powershell.exe");
    }

    #[test]
    fn shell_type_command_flag() {
        assert_eq!(ShellType::Bash.command_flag(), "-c");
        assert_eq!(ShellType::Cmd.command_flag(), "/C");
        assert_eq!(ShellType::PowerShell.command_flag(), "-Command");
    }

    #[test]
    fn shell_env_syntax() {
        assert_eq!(ShellType::Bash.env_syntax("HOME"), "$HOME");
        assert_eq!(ShellType::Cmd.env_syntax("HOME"), "%HOME%");
        assert_eq!(ShellType::PowerShell.env_syntax("HOME"), "$env:HOME");
    }

    #[test]
    fn shell_command_echo() {
        // Test with simple echo command
        let output = ShellCommand::new("echo hello")
            .run()
            .expect("echo should work");

        assert!(output.success());
        assert!(output.stdout_trimmed().contains("hello"));
    }

    #[test]
    fn shell_command_env() {
        let output = ShellCommand::new(if cfg!(windows) {
            "echo %TEST_VAR%"
        } else {
            "echo $TEST_VAR"
        })
        .env("TEST_VAR", "test_value")
        .run()
        .expect("env command should work");

        assert!(output.stdout_trimmed().contains("test_value"));
    }

    #[test]
    fn shebang_parsing() {
        assert_eq!(
            parse_shebang_line("#!/bin/bash"),
            Some((PathBuf::from("/bin/bash"), vec![]))
        );

        assert_eq!(
            parse_shebang_line("#!/usr/bin/env python3"),
            Some((PathBuf::from("python3"), vec![]))
        );

        assert_eq!(
            parse_shebang_line("#!/bin/bash -e -x"),
            Some((
                PathBuf::from("/bin/bash"),
                vec!["-e".to_string(), "-x".to_string()]
            ))
        );

        assert_eq!(parse_shebang_line("# not a shebang"), None);
    }

    #[test]
    fn shell_escape_posix() {
        assert_eq!(escape_shell_arg("simple", ShellType::Bash), "simple");
        assert_eq!(
            escape_shell_arg("with space", ShellType::Bash),
            "'with space'"
        );
        assert_eq!(escape_shell_arg("it's", ShellType::Bash), "'it'\\''s'");
    }

    #[test]
    fn shell_escape_cmd() {
        assert_eq!(escape_shell_arg("simple", ShellType::Cmd), "simple");
        assert_eq!(
            escape_shell_arg("with space", ShellType::Cmd),
            "\"with space\""
        );
    }

    #[test]
    fn join_args_test() {
        let args = vec!["arg1", "arg with space", "arg3"];
        let joined = join_args(args, ShellType::Bash);
        assert_eq!(joined, "arg1 'arg with space' arg3");
    }

    #[test]
    fn shell_is_unix() {
        assert!(ShellType::Bash.is_unix());
        assert!(ShellType::Zsh.is_unix());
        assert!(!ShellType::Cmd.is_unix());
        assert!(!ShellType::PowerShell.is_unix());
    }

    #[test]
    fn shell_is_windows() {
        assert!(!ShellType::Bash.is_windows());
        assert!(ShellType::Cmd.is_windows());
        assert!(ShellType::PowerShell.is_windows());
    }
}
