//! Composer script execution for lifecycle hooks.
//!
//! This module provides execution of Composer scripts during the install/update lifecycle:
//!
//! # Supported Events
//!
//! - `pre-install-cmd`: Before packages are installed
//! - `post-install-cmd`: After all packages are installed
//! - `pre-update-cmd`: Before packages are updated
//! - `post-update-cmd`: After all packages are updated
//! - `pre-autoload-dump`: Before autoloader is generated
//! - `post-autoload-dump`: After autoloader is generated
//! - `post-root-package-install`: After root package is installed (create-project)
//! - `pre-operations-exec`: Before package operations execute
//!
//! # Script Formats
//!
//! Scripts can be:
//! - A single command string
//! - An array of commands
//! - A reference to another script: `@script-name`
//! - Special directives: `@php`, `@composer`, `@putenv`
//!
//! # Example
//!
//! ```json
//! {
//!     "scripts": {
//!         "post-install-cmd": [
//!             "@php artisan package:discover",
//!             "echo Installation complete"
//!         ],
//!         "test": "phpunit",
//!         "cs-fix": "@php vendor/bin/php-cs-fixer fix"
//!     }
//! }
//! ```

use anyhow::{Context, Result, bail};
use sonic_rs::{JsonContainerTrait, JsonValueTrait, Value};
use std::collections::HashMap;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Script event types for lifecycle hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum ScriptEvent {
    /// Before packages are installed.
    PreInstallCmd,
    /// After packages are installed.
    PostInstallCmd,
    /// Before packages are updated.
    PreUpdateCmd,
    /// After packages are updated.
    PostUpdateCmd,
    /// Before autoloader is generated.
    PreAutoloadDump,
    /// After autoloader is generated.
    PostAutoloadDump,
    /// After root package is installed (create-project).
    PostRootPackageInstall,
    /// Before package operations execute.
    PreOperationsExec,
    /// After create-project command.
    PostCreateProjectCmd,
    /// Before status command.
    PreStatusCmd,
    /// After status command.
    PostStatusCmd,
    /// Before archive command.
    PreArchiveCmd,
    /// After archive command.
    PostArchiveCmd,
}

#[allow(dead_code)]
impl ScriptEvent {
    /// Get the script key name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PreInstallCmd => "pre-install-cmd",
            Self::PostInstallCmd => "post-install-cmd",
            Self::PreUpdateCmd => "pre-update-cmd",
            Self::PostUpdateCmd => "post-update-cmd",
            Self::PreAutoloadDump => "pre-autoload-dump",
            Self::PostAutoloadDump => "post-autoload-dump",
            Self::PostRootPackageInstall => "post-root-package-install",
            Self::PreOperationsExec => "pre-operations-exec",
            Self::PostCreateProjectCmd => "post-create-project-cmd",
            Self::PreStatusCmd => "pre-status-cmd",
            Self::PostStatusCmd => "post-status-cmd",
            Self::PreArchiveCmd => "pre-archive-cmd",
            Self::PostArchiveCmd => "post-archive-cmd",
        }
    }

    /// Parse from string.
    #[must_use]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pre-install-cmd" => Some(Self::PreInstallCmd),
            "post-install-cmd" => Some(Self::PostInstallCmd),
            "pre-update-cmd" => Some(Self::PreUpdateCmd),
            "post-update-cmd" => Some(Self::PostUpdateCmd),
            "pre-autoload-dump" => Some(Self::PreAutoloadDump),
            "post-autoload-dump" => Some(Self::PostAutoloadDump),
            "post-root-package-install" => Some(Self::PostRootPackageInstall),
            "pre-operations-exec" => Some(Self::PreOperationsExec),
            "post-create-project-cmd" => Some(Self::PostCreateProjectCmd),
            "pre-status-cmd" => Some(Self::PreStatusCmd),
            "post-status-cmd" => Some(Self::PostStatusCmd),
            "pre-archive-cmd" => Some(Self::PreArchiveCmd),
            "post-archive-cmd" => Some(Self::PostArchiveCmd),
            _ => None,
        }
    }
}

/// Result of script execution.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ScriptResult {
    /// Script name/event that was executed.
    pub name: String,
    /// Number of commands executed.
    pub commands_executed: usize,
    /// Whether all commands succeeded.
    pub success: bool,
    /// Exit code of the last failed command (if any).
    pub exit_code: Option<i32>,
    /// Error message (if failed).
    pub error: Option<String>,
    /// Total execution duration.
    pub duration: Duration,
}

impl ScriptResult {
    fn success(name: &str, commands: usize, duration: Duration) -> Self {
        Self {
            name: name.to_string(),
            commands_executed: commands,
            success: true,
            exit_code: None,
            error: None,
            duration,
        }
    }

    fn failure(name: &str, commands: usize, code: i32, error: &str, duration: Duration) -> Self {
        Self {
            name: name.to_string(),
            commands_executed: commands,
            success: false,
            exit_code: Some(code),
            error: Some(error.to_string()),
            duration,
        }
    }
}

/// Configuration for script execution.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ScriptConfig {
    /// Working directory.
    pub working_dir: std::path::PathBuf,
    /// PHP binary path.
    pub php_binary: String,
    /// Libretto binary path.
    pub composer_binary: String,
    /// Whether we're in dev mode.
    pub dev_mode: bool,
    /// Script timeout in seconds (0 = no timeout).
    pub timeout: u64,
    /// Additional environment variables.
    pub env: HashMap<String, String>,
    /// Whether to stop on first error.
    pub stop_on_error: bool,
}

impl Default for ScriptConfig {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_default(),
            php_binary: "php".to_string(),
            composer_binary: std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "libretto".to_string()),
            dev_mode: true,
            timeout: 300,
            env: HashMap::new(),
            stop_on_error: true,
        }
    }
}

/// Script executor for running Composer scripts.
#[allow(dead_code)]
pub struct ScriptExecutor {
    /// Parsed scripts from composer.json.
    scripts: HashMap<String, Vec<String>>,
    /// Configuration.
    config: ScriptConfig,
    /// Script call stack (for detecting recursion).
    call_stack: Vec<String>,
}

#[allow(dead_code)]
impl ScriptExecutor {
    /// Create a new script executor from composer.json content.
    pub fn new(composer_json: &Value, config: ScriptConfig) -> Self {
        let scripts = Self::parse_scripts(composer_json);
        Self {
            scripts,
            config,
            call_stack: Vec::new(),
        }
    }

    /// Parse scripts from composer.json.
    fn parse_scripts(composer_json: &Value) -> HashMap<String, Vec<String>> {
        let mut scripts = HashMap::new();

        if let Some(scripts_obj) = composer_json.get("scripts").and_then(|v| v.as_object()) {
            for (name, value) in scripts_obj {
                let commands = if let Some(cmd) = value.as_str() {
                    vec![cmd.to_string()]
                } else if let Some(arr) = value.as_array() {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                } else {
                    continue;
                };

                scripts.insert(name.to_string(), commands);
            }
        }

        scripts
    }

    /// Check if a script exists.
    #[must_use]
    pub fn has_script(&self, name: &str) -> bool {
        self.scripts.contains_key(name)
    }

    /// Check if an event script exists.
    #[must_use]
    pub fn has_event(&self, event: ScriptEvent) -> bool {
        self.has_script(event.as_str())
    }

    /// Get list of available scripts.
    #[must_use]
    pub fn available_scripts(&self) -> Vec<&str> {
        self.scripts.keys().map(|s| s.as_str()).collect()
    }

    /// Execute an event script.
    pub fn run_event(&mut self, event: ScriptEvent) -> Result<Option<ScriptResult>> {
        self.run_script(event.as_str())
    }

    /// Execute a named script.
    pub fn run_script(&mut self, name: &str) -> Result<Option<ScriptResult>> {
        let commands = match self.scripts.get(name) {
            Some(cmds) => cmds.clone(),
            None => return Ok(None),
        };

        if commands.is_empty() {
            return Ok(None);
        }

        // Check for recursion
        if self.call_stack.contains(&name.to_string()) {
            bail!(
                "Circular script reference detected: {} -> {}",
                self.call_stack.join(" -> "),
                name
            );
        }

        self.call_stack.push(name.to_string());

        let start = Instant::now();
        let mut executed = 0;

        info!(script = %name, commands = commands.len(), "executing script");

        for cmd in &commands {
            let result = self.execute_command(cmd)?;
            executed += 1;

            if let Some(status) = result {
                if !status.success() {
                    let code = status.code().unwrap_or(-1);
                    self.call_stack.pop();
                    return Ok(Some(ScriptResult::failure(
                        name,
                        executed,
                        code,
                        &format!("Command failed: {}", cmd),
                        start.elapsed(),
                    )));
                }
            }
        }

        self.call_stack.pop();

        Ok(Some(ScriptResult::success(name, executed, start.elapsed())))
    }

    /// Execute a single command.
    fn execute_command(&mut self, cmd: &str) -> Result<Option<ExitStatus>> {
        let cmd = cmd.trim();

        // Handle @reference syntax
        if let Some(ref_name) = cmd.strip_prefix('@') {
            // Handle special directives
            if ref_name.starts_with("php ") {
                let php_cmd = ref_name.strip_prefix("php ").unwrap();
                return self.execute_shell(&format!("{} {}", self.config.php_binary, php_cmd));
            }

            if ref_name.starts_with("composer ") {
                let composer_cmd = ref_name.strip_prefix("composer ").unwrap();
                return self
                    .execute_shell(&format!("{} {}", self.config.composer_binary, composer_cmd));
            }

            if ref_name.starts_with("putenv ") {
                let putenv = ref_name.strip_prefix("putenv ").unwrap();
                if let Some((key, value)) = putenv.split_once('=') {
                    self.config.env.insert(key.to_string(), value.to_string());
                }
                return Ok(None);
            }

            // Reference to another script
            if let Some(result) = self.run_script(ref_name)? {
                if !result.success {
                    bail!(
                        "Referenced script '{}' failed: {:?}",
                        ref_name,
                        result.error
                    );
                }
            }
            return Ok(None);
        }

        // Check for PHP class method syntax: Namespace\Class::method
        // This pattern matches class names like "Illuminate\Foundation\ComposerScripts::postAutoloadDump"
        if is_php_class_method(cmd) {
            return self.execute_php_callback(cmd);
        }

        self.execute_shell(cmd)
    }

    /// Execute a PHP class static method callback.
    ///
    /// Handles Composer-style callbacks like:
    /// - `Illuminate\Foundation\ComposerScripts::postAutoloadDump`
    /// - `MyNamespace\MyClass::myMethod`
    fn execute_php_callback(&self, callback: &str) -> Result<Option<ExitStatus>> {
        debug!(callback = %callback, "executing PHP callback");

        // Generate PHP code to call the static method
        // We need to ensure the autoloader is loaded first
        let php_code = format!(
            r#"<?php
require_once __DIR__ . '/vendor/autoload.php';
call_user_func('{}');
"#,
            callback.replace('\\', "\\\\")
        );

        // Execute via PHP
        self.execute_shell(&format!(
            "{} -r {}",
            self.config.php_binary,
            shell_escape(&php_code)
        ))
    }

    /// Execute a shell command.
    fn execute_shell(&self, cmd: &str) -> Result<Option<ExitStatus>> {
        debug!(command = %cmd, "executing shell command");

        // Build environment
        let mut env: HashMap<String, String> = std::env::vars().collect();

        // Add vendor/bin to PATH
        let vendor_bin = self.config.working_dir.join("vendor").join("bin");
        if vendor_bin.exists() {
            let path = env.get("PATH").cloned().unwrap_or_default();
            let separator = if cfg!(windows) { ";" } else { ":" };
            let new_path = format!("{}{}{}", vendor_bin.display(), separator, path);
            env.insert("PATH".to_string(), new_path);
        }

        // Add COMPOSER_* variables
        env.insert(
            "COMPOSER_BINARY".to_string(),
            self.config.composer_binary.clone(),
        );
        env.insert(
            "COMPOSER_DEV_MODE".to_string(),
            if self.config.dev_mode { "1" } else { "0" }.to_string(),
        );

        // Add custom environment
        env.extend(self.config.env.clone());

        // Determine shell
        let (shell, shell_arg) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };

        let mut command = Command::new(shell);
        command
            .arg(shell_arg)
            .arg(cmd)
            .current_dir(&self.config.working_dir)
            .envs(&env)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = command
            .status()
            .context(format!("Failed to execute: {}", cmd))?;

        Ok(Some(status))
    }
}

/// Run pre-install or pre-update scripts.
pub fn run_pre_install_scripts(
    composer_json: &Value,
    config: &ScriptConfig,
    is_update: bool,
) -> Result<Option<ScriptResult>> {
    let mut executor = ScriptExecutor::new(composer_json, config.clone());

    let event = if is_update {
        ScriptEvent::PreUpdateCmd
    } else {
        ScriptEvent::PreInstallCmd
    };

    if let Some(result) = executor.run_event(event)? {
        info!(
            event = event.as_str(),
            commands = result.commands_executed,
            success = result.success,
            "pre-install/update scripts completed"
        );
        return Ok(Some(result));
    }

    Ok(None)
}

/// Run post-install or post-update scripts.
pub fn run_post_install_scripts(
    composer_json: &Value,
    config: &ScriptConfig,
    is_update: bool,
) -> Result<Option<ScriptResult>> {
    let mut executor = ScriptExecutor::new(composer_json, config.clone());

    let event = if is_update {
        ScriptEvent::PostUpdateCmd
    } else {
        ScriptEvent::PostInstallCmd
    };

    if let Some(result) = executor.run_event(event)? {
        info!(
            event = event.as_str(),
            commands = result.commands_executed,
            success = result.success,
            "post-install/update scripts completed"
        );
        return Ok(Some(result));
    }

    Ok(None)
}

/// Run pre-autoload-dump scripts.
pub fn run_pre_autoload_scripts(
    composer_json: &Value,
    config: &ScriptConfig,
) -> Result<Option<ScriptResult>> {
    let mut executor = ScriptExecutor::new(composer_json, config.clone());

    if let Some(result) = executor.run_event(ScriptEvent::PreAutoloadDump)? {
        info!(
            event = "pre-autoload-dump",
            commands = result.commands_executed,
            success = result.success,
            "pre-autoload-dump scripts completed"
        );
        return Ok(Some(result));
    }

    Ok(None)
}

/// Run post-autoload-dump scripts.
pub fn run_post_autoload_scripts(
    composer_json: &Value,
    config: &ScriptConfig,
) -> Result<Option<ScriptResult>> {
    let mut executor = ScriptExecutor::new(composer_json, config.clone());

    if let Some(result) = executor.run_event(ScriptEvent::PostAutoloadDump)? {
        info!(
            event = "post-autoload-dump",
            commands = result.commands_executed,
            success = result.success,
            "post-autoload-dump scripts completed"
        );
        return Ok(Some(result));
    }

    Ok(None)
}

/// Run create-project scripts.
#[allow(dead_code)]
pub fn run_create_project_scripts(
    composer_json: &Value,
    config: &ScriptConfig,
) -> Result<Option<ScriptResult>> {
    let mut executor = ScriptExecutor::new(composer_json, config.clone());

    if let Some(result) = executor.run_event(ScriptEvent::PostCreateProjectCmd)? {
        info!(
            event = "post-create-project-cmd",
            commands = result.commands_executed,
            success = result.success,
            "post-create-project scripts completed"
        );
        return Ok(Some(result));
    }

    Ok(None)
}

/// Run root package install scripts (for create-project).
#[allow(dead_code)]
pub fn run_root_package_install_scripts(
    composer_json: &Value,
    config: &ScriptConfig,
) -> Result<Option<ScriptResult>> {
    let mut executor = ScriptExecutor::new(composer_json, config.clone());

    if let Some(result) = executor.run_event(ScriptEvent::PostRootPackageInstall)? {
        info!(
            event = "post-root-package-install",
            commands = result.commands_executed,
            success = result.success,
            "post-root-package-install scripts completed"
        );
        return Ok(Some(result));
    }

    Ok(None)
}

/// Check if a command looks like a PHP class method callback.
///
/// Matches patterns like:
/// - `Illuminate\Foundation\ComposerScripts::postAutoloadDump`
/// - `MyNamespace\MyClass::myMethod`
/// - `MyClass::method` (no namespace)
///
/// The pattern must:
/// - Contain `::` to indicate a static method call
/// - Have valid PHP identifier characters (alphanumeric, underscore, backslash for namespaces)
/// - Not start with common shell/unix commands
fn is_php_class_method(cmd: &str) -> bool {
    // Must contain :: for static method call
    if !cmd.contains("::") {
        return false;
    }

    // Split by :: and validate both parts
    let parts: Vec<&str> = cmd.splitn(2, "::").collect();
    if parts.len() != 2 {
        return false;
    }

    let class_part = parts[0].trim();
    let method_part = parts[1].trim();

    // Class part must be non-empty and look like a PHP class/namespace
    if class_part.is_empty() || method_part.is_empty() {
        return false;
    }

    // Class part should only contain valid PHP namespace characters
    // (letters, numbers, underscore, backslash for namespaces)
    let valid_class = class_part
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '\\');

    // Method part should be a valid PHP identifier (and may have parentheses for arguments)
    let method_name = method_part.split('(').next().unwrap_or(method_part).trim();
    let valid_method =
        !method_name.is_empty() && method_name.chars().all(|c| c.is_alphanumeric() || c == '_');

    // Must start with uppercase letter or backslash (namespace)
    let starts_valid = class_part
        .chars()
        .next()
        .is_some_and(|c| c.is_uppercase() || c == '\\');

    valid_class && valid_method && starts_valid
}

/// Escape a string for use in shell commands.
fn shell_escape(s: &str) -> String {
    // For single-quoted strings in shell, we need to:
    // 1. Replace ' with '\'' (end quote, escaped quote, start quote)
    // 2. Wrap in single quotes
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_composer_json() -> Value {
        sonic_rs::json!({
            "scripts": {
                "test": "echo 'Running tests'",
                "build": ["echo 'Step 1'", "echo 'Step 2'"],
                "post-install-cmd": ["@test", "echo 'Installed'"],
                "with-php": "@php -v",
                "with-env": "@putenv FOO=bar"
            }
        })
    }

    #[test]
    fn test_parse_scripts() {
        let json = create_test_composer_json();
        let scripts = ScriptExecutor::parse_scripts(&json);

        assert!(scripts.contains_key("test"));
        assert!(scripts.contains_key("build"));
        assert!(scripts.contains_key("post-install-cmd"));

        assert_eq!(scripts.get("test").unwrap(), &vec!["echo 'Running tests'"]);
        assert_eq!(scripts.get("build").unwrap().len(), 2);
    }

    #[test]
    fn test_has_script() {
        let json = create_test_composer_json();
        let executor = ScriptExecutor::new(&json, ScriptConfig::default());

        assert!(executor.has_script("test"));
        assert!(executor.has_script("build"));
        assert!(!executor.has_script("nonexistent"));
    }

    #[test]
    fn test_has_event() {
        let json = create_test_composer_json();
        let executor = ScriptExecutor::new(&json, ScriptConfig::default());

        assert!(executor.has_event(ScriptEvent::PostInstallCmd));
        assert!(!executor.has_event(ScriptEvent::PreInstallCmd));
    }

    #[test]
    fn test_script_event_as_str() {
        assert_eq!(ScriptEvent::PostInstallCmd.as_str(), "post-install-cmd");
        assert_eq!(ScriptEvent::PreUpdateCmd.as_str(), "pre-update-cmd");
    }

    #[test]
    fn test_script_event_from_str() {
        assert_eq!(
            ScriptEvent::from_str("post-install-cmd"),
            Some(ScriptEvent::PostInstallCmd)
        );
        assert_eq!(ScriptEvent::from_str("invalid"), None);
    }

    #[test]
    fn test_is_php_class_method() {
        // Valid PHP class methods
        assert!(is_php_class_method(
            "Illuminate\\Foundation\\ComposerScripts::postAutoloadDump"
        ));
        assert!(is_php_class_method("MyNamespace\\MyClass::myMethod"));
        assert!(is_php_class_method("MyClass::method"));
        assert!(is_php_class_method(
            "App\\Providers\\AppServiceProvider::boot"
        ));

        // Invalid - shell commands
        assert!(!is_php_class_method("echo 'hello'"));
        assert!(!is_php_class_method("php artisan serve"));
        assert!(!is_php_class_method("npm run build"));

        // Invalid - missing ::
        assert!(!is_php_class_method("MyClass"));
        assert!(!is_php_class_method(
            "Illuminate\\Foundation\\ComposerScripts"
        ));

        // Invalid - lowercase start (likely shell command)
        assert!(!is_php_class_method("myclass::method"));

        // Invalid - empty parts
        assert!(!is_php_class_method("::method"));
        assert!(!is_php_class_method("MyClass::"));
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("hello"), "'hello'");
        assert_eq!(shell_escape("hello world"), "'hello world'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }
}
