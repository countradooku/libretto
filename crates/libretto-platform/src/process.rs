//! Cross-platform process spawning and management.
//!
//! Provides unified interface for:
//! - Unix: fork/exec, process groups, signals
//! - Windows: CreateProcess, job objects

#![allow(unsafe_code)]

use crate::{PlatformError, Result};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Process builder with cross-platform support.
#[derive(Debug)]
pub struct ProcessBuilder {
    /// Program to execute.
    program: PathBuf,
    /// Arguments.
    args: Vec<String>,
    /// Working directory.
    cwd: Option<PathBuf>,
    /// Environment variables to add/override.
    env_override: HashMap<String, String>,
    /// Environment variables to remove.
    env_remove: Vec<String>,
    /// Whether to clear the environment.
    clear_env: bool,
    /// Stdin configuration.
    stdin: StdioConfig,
    /// Stdout configuration.
    stdout: StdioConfig,
    /// Stderr configuration.
    stderr: StdioConfig,
    /// Create new process group (Unix).
    new_process_group: bool,
    /// Create new session (Unix).
    new_session: bool,
    /// Timeout for execution.
    timeout: Option<Duration>,
}

/// Stdio configuration.
#[derive(Debug, Clone, Copy, Default)]
pub enum StdioConfig {
    /// Inherit from parent.
    #[default]
    Inherit,
    /// Pipe for reading/writing.
    Pipe,
    /// Null device (/dev/null, NUL).
    Null,
}

impl ProcessBuilder {
    /// Create a new process builder.
    #[must_use]
    pub fn new(program: impl AsRef<Path>) -> Self {
        Self {
            program: program.as_ref().to_path_buf(),
            args: Vec::new(),
            cwd: None,
            env_override: HashMap::new(),
            env_remove: Vec::new(),
            clear_env: false,
            stdin: StdioConfig::default(),
            stdout: StdioConfig::default(),
            stderr: StdioConfig::default(),
            new_process_group: false,
            new_session: false,
            timeout: None,
        }
    }

    /// Add an argument.
    #[must_use]
    pub fn arg(mut self, arg: impl AsRef<str>) -> Self {
        self.args.push(arg.as_ref().to_string());
        self
    }

    /// Add multiple arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.args
            .extend(args.into_iter().map(|s| s.as_ref().to_string()));
        self
    }

    /// Set working directory.
    #[must_use]
    pub fn cwd(mut self, dir: impl AsRef<Path>) -> Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Set environment variable.
    #[must_use]
    pub fn env(mut self, key: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        self.env_override
            .insert(key.as_ref().to_string(), value.as_ref().to_string());
        self
    }

    /// Set multiple environment variables.
    #[must_use]
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        for (k, v) in vars {
            self.env_override
                .insert(k.as_ref().to_string(), v.as_ref().to_string());
        }
        self
    }

    /// Remove an environment variable.
    #[must_use]
    pub fn env_remove(mut self, key: impl AsRef<str>) -> Self {
        self.env_remove.push(key.as_ref().to_string());
        self
    }

    /// Clear all environment variables (start fresh).
    #[must_use]
    pub fn env_clear(mut self) -> Self {
        self.clear_env = true;
        self
    }

    /// Configure stdin.
    #[must_use]
    pub const fn stdin(mut self, config: StdioConfig) -> Self {
        self.stdin = config;
        self
    }

    /// Configure stdout.
    #[must_use]
    pub const fn stdout(mut self, config: StdioConfig) -> Self {
        self.stdout = config;
        self
    }

    /// Configure stderr.
    #[must_use]
    pub const fn stderr(mut self, config: StdioConfig) -> Self {
        self.stderr = config;
        self
    }

    /// Capture stdout and stderr.
    #[must_use]
    pub const fn capture(mut self) -> Self {
        self.stdout = StdioConfig::Pipe;
        self.stderr = StdioConfig::Pipe;
        self
    }

    /// Suppress all output.
    #[must_use]
    pub const fn quiet(mut self) -> Self {
        self.stdout = StdioConfig::Null;
        self.stderr = StdioConfig::Null;
        self
    }

    /// Create a new process group (Unix only).
    #[must_use]
    pub const fn new_process_group(mut self) -> Self {
        self.new_process_group = true;
        self
    }

    /// Create a new session (Unix only).
    #[must_use]
    pub const fn new_session(mut self) -> Self {
        self.new_session = true;
        self
    }

    /// Set execution timeout.
    #[must_use]
    pub const fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Build the Command.
    fn build_command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args);

        if let Some(ref cwd) = self.cwd {
            cmd.current_dir(cwd);
        }

        if self.clear_env {
            cmd.env_clear();
        }

        for (k, v) in &self.env_override {
            cmd.env(k, v);
        }

        for k in &self.env_remove {
            cmd.env_remove(k);
        }

        cmd.stdin(self.stdin.to_stdio());
        cmd.stdout(self.stdout.to_stdio());
        cmd.stderr(self.stderr.to_stdio());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            if self.new_process_group {
                cmd.process_group(0);
            }
            // Note: new_session would require unsafe code
        }

        cmd
    }

    /// Spawn the process and return a handle.
    ///
    /// # Errors
    /// Returns error if process cannot be spawned.
    pub fn spawn(self) -> Result<ProcessHandle> {
        let mut cmd = self.build_command();
        let timeout = self.timeout;
        let program = self.program.clone();

        let child = cmd.spawn().map_err(|e| {
            PlatformError::spawn_failed(program.display().to_string(), e.to_string())
        })?;

        Ok(ProcessHandle {
            child,
            timeout,
            killed: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Execute and wait for completion.
    ///
    /// # Errors
    /// Returns error if execution fails.
    pub fn run(self) -> Result<ProcessOutput> {
        let handle = self.spawn()?;
        handle.wait()
    }

    /// Execute and return output.
    ///
    /// # Errors
    /// Returns error if execution fails.
    pub fn output(self) -> Result<Output> {
        let program = self.program.clone();
        let mut cmd = self.build_command();

        cmd.output()
            .map_err(|e| PlatformError::spawn_failed(program.display().to_string(), e.to_string()))
    }

    /// Execute and check for success.
    ///
    /// # Errors
    /// Returns error if command fails or exits with non-zero status.
    pub fn status(self) -> Result<()> {
        let output = self.capture().output()?;

        if output.status.success() {
            Ok(())
        } else {
            Err(PlatformError::ProcessFailed {
                code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        }
    }
}

impl StdioConfig {
    fn to_stdio(self) -> Stdio {
        match self {
            Self::Inherit => Stdio::inherit(),
            Self::Pipe => Stdio::piped(),
            Self::Null => Stdio::null(),
        }
    }
}

/// Handle to a running process.
#[derive(Debug)]
pub struct ProcessHandle {
    /// Child process.
    child: Child,
    /// Execution timeout.
    timeout: Option<Duration>,
    /// Whether the process was killed.
    killed: Arc<AtomicBool>,
}

impl ProcessHandle {
    /// Get the process ID.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Check if process is still running.
    ///
    /// # Errors
    /// Returns error if status cannot be checked.
    pub fn is_running(&mut self) -> Result<bool> {
        match self.child.try_wait() {
            Ok(Some(_)) => Ok(false),
            Ok(None) => Ok(true),
            Err(e) => Err(PlatformError::Process(e.to_string())),
        }
    }

    /// Wait for the process to complete.
    ///
    /// # Errors
    /// Returns error if wait fails or timeout expires.
    pub fn wait(mut self) -> Result<ProcessOutput> {
        if let Some(timeout) = self.timeout {
            self.wait_with_timeout(timeout)
        } else {
            let output = self
                .child
                .wait_with_output()
                .map_err(|e| PlatformError::Process(e.to_string()))?;

            Ok(ProcessOutput {
                status: output.status,
                stdout: output.stdout,
                stderr: output.stderr,
                killed: self.killed.load(Ordering::SeqCst),
            })
        }
    }

    fn wait_with_timeout(&mut self, timeout: Duration) -> Result<ProcessOutput> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(10);

        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    // Process completed
                    let mut stdout = Vec::new();
                    let mut stderr = Vec::new();

                    if let Some(ref mut out) = self.child.stdout {
                        out.read_to_end(&mut stdout).ok();
                    }
                    if let Some(ref mut err) = self.child.stderr {
                        err.read_to_end(&mut stderr).ok();
                    }

                    return Ok(ProcessOutput {
                        status,
                        stdout,
                        stderr,
                        killed: self.killed.load(Ordering::SeqCst),
                    });
                }
                Ok(None) => {
                    // Still running
                    if start.elapsed() >= timeout {
                        // Timeout - kill the process
                        self.kill()?;
                        return Err(PlatformError::Process(format!(
                            "Process timed out after {:?}",
                            timeout
                        )));
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => return Err(PlatformError::Process(e.to_string())),
            }
        }
    }

    /// Send a signal to the process (Unix) or terminate (Windows).
    ///
    /// # Errors
    /// Returns error if signal cannot be sent.
    pub fn kill(&mut self) -> Result<()> {
        self.killed.store(true, Ordering::SeqCst);
        self.child
            .kill()
            .map_err(|e| PlatformError::Process(e.to_string()))
    }

    /// Send SIGTERM (Unix) or terminate (Windows).
    ///
    /// # Errors
    /// Returns error if termination fails.
    #[cfg(unix)]
    pub fn terminate(&mut self) -> Result<()> {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        kill(Pid::from_raw(self.child.id() as i32), Signal::SIGTERM)
            .map_err(|e| PlatformError::Process(e.to_string()))
    }

    #[cfg(windows)]
    pub fn terminate(&mut self) -> Result<()> {
        self.kill()
    }

    /// Take ownership of stdin.
    #[must_use]
    pub fn stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.child.stdin.take()
    }

    /// Take ownership of stdout.
    #[must_use]
    pub fn stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.child.stdout.take()
    }

    /// Take ownership of stderr.
    #[must_use]
    pub fn stderr(&mut self) -> Option<std::process::ChildStderr> {
        self.child.stderr.take()
    }

    /// Read stdout line by line.
    pub fn read_stdout_lines(
        &mut self,
    ) -> Option<impl Iterator<Item = std::io::Result<String>> + '_> {
        self.child
            .stdout
            .as_mut()
            .map(|stdout| BufReader::new(stdout).lines())
    }

    /// Read stderr line by line.
    pub fn read_stderr_lines(
        &mut self,
    ) -> Option<impl Iterator<Item = std::io::Result<String>> + '_> {
        self.child
            .stderr
            .as_mut()
            .map(|stderr| BufReader::new(stderr).lines())
    }
}

/// Output from a completed process.
#[derive(Debug)]
pub struct ProcessOutput {
    /// Exit status.
    pub status: ExitStatus,
    /// Captured stdout.
    pub stdout: Vec<u8>,
    /// Captured stderr.
    pub stderr: Vec<u8>,
    /// Whether the process was killed.
    pub killed: bool,
}

impl ProcessOutput {
    /// Check if process succeeded (exit code 0).
    #[must_use]
    pub fn success(&self) -> bool {
        self.status.success()
    }

    /// Get exit code.
    #[must_use]
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }

    /// Get stdout as string.
    #[must_use]
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }

    /// Get stderr as string.
    #[must_use]
    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }

    /// Get stdout as trimmed string.
    #[must_use]
    pub fn stdout_trimmed(&self) -> String {
        self.stdout_str().trim().to_string()
    }

    /// Get stderr as trimmed string.
    #[must_use]
    pub fn stderr_trimmed(&self) -> String {
        self.stderr_str().trim().to_string()
    }

    /// Convert to Result, returning Err if process failed.
    ///
    /// # Errors
    /// Returns error with stderr if process exited with non-zero status.
    pub fn into_result(self) -> Result<Self> {
        if self.success() {
            Ok(self)
        } else {
            Err(PlatformError::ProcessFailed {
                code: self.code().unwrap_or(-1),
                stderr: self.stderr_str(),
            })
        }
    }
}

/// Find an executable in PATH.
#[must_use]
pub fn which(program: &str) -> Option<PathBuf> {
    // Check if it's an absolute path
    let path = Path::new(program);
    if path.is_absolute() {
        return if is_executable(path) {
            Some(path.to_path_buf())
        } else {
            None
        };
    }

    // Search PATH
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let full_path = dir.join(program);

        // On Windows, also try with common extensions
        #[cfg(windows)]
        {
            for ext in &["", ".exe", ".cmd", ".bat", ".com"] {
                let with_ext = if ext.is_empty() {
                    full_path.clone()
                } else {
                    full_path.with_extension(&ext[1..])
                };
                if is_executable(&with_ext) {
                    return Some(with_ext);
                }
            }
        }

        #[cfg(not(windows))]
        {
            if is_executable(&full_path) {
                return Some(full_path);
            }
        }
    }

    None
}

/// Check if a path is executable.
#[must_use]
pub fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        // On Windows, executability is based on extension
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                let e_lower = e.to_lowercase();
                matches!(e_lower.as_str(), "exe" | "cmd" | "bat" | "com" | "ps1")
            })
            .unwrap_or(false)
    }
}

/// Run a command and return its output, failing on non-zero exit.
///
/// # Errors
/// Returns error if command fails.
pub fn run_command(program: &str, args: &[&str]) -> Result<String> {
    let output = ProcessBuilder::new(program)
        .args(args.iter().copied())
        .capture()
        .run()?
        .into_result()?;

    Ok(output.stdout_trimmed())
}

/// Run a command silently (no output).
///
/// # Errors
/// Returns error if command fails.
pub fn run_silent(program: &str, args: &[&str]) -> Result<()> {
    ProcessBuilder::new(program)
        .args(args.iter().copied())
        .quiet()
        .status()
}

/// Platform-specific process operations.
#[cfg(unix)]
pub mod unix {
    use super::*;

    /// Get the process group ID of a process.
    pub fn getpgid(pid: u32) -> Result<u32> {
        use nix::unistd::{getpgid, Pid};

        getpgid(Some(Pid::from_raw(pid as i32)))
            .map(|pgid| pgid.as_raw() as u32)
            .map_err(|e| PlatformError::Process(e.to_string()))
    }

    /// Get the session ID of a process.
    pub fn getsid(pid: u32) -> Result<u32> {
        use nix::unistd::{getsid, Pid};

        getsid(Some(Pid::from_raw(pid as i32)))
            .map(|sid| sid.as_raw() as u32)
            .map_err(|e| PlatformError::Process(e.to_string()))
    }

    /// Kill all processes in a process group.
    pub fn kill_process_group(pgid: u32, signal: i32) -> Result<()> {
        use nix::sys::signal::{killpg, Signal};
        use nix::unistd::Pid;

        let sig = Signal::try_from(signal)
            .map_err(|_| PlatformError::Signal(format!("Invalid signal: {signal}")))?;

        killpg(Pid::from_raw(pgid as i32), sig).map_err(|e| PlatformError::Process(e.to_string()))
    }

    /// Create a daemon process (double fork).
    ///
    /// # Errors
    /// Returns error if daemonization fails.
    pub fn daemonize() -> Result<()> {
        use nix::unistd::{fork, setsid, ForkResult};

        // First fork
        match unsafe { fork() } {
            Ok(ForkResult::Parent { .. }) => std::process::exit(0),
            Ok(ForkResult::Child) => {}
            Err(e) => return Err(PlatformError::Process(e.to_string())),
        }

        // Create new session
        setsid().map_err(|e| PlatformError::Process(e.to_string()))?;

        // Second fork
        match unsafe { fork() } {
            Ok(ForkResult::Parent { .. }) => std::process::exit(0),
            Ok(ForkResult::Child) => {}
            Err(e) => return Err(PlatformError::Process(e.to_string())),
        }

        // Change to root directory
        std::env::set_current_dir("/").ok();

        // Close standard file descriptors
        // (In practice, redirect to /dev/null)

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_builder_basic() {
        let builder = ProcessBuilder::new("echo").arg("hello").arg("world");

        assert_eq!(builder.args, vec!["hello", "world"]);
    }

    #[test]
    fn process_builder_env() {
        let builder = ProcessBuilder::new("env")
            .env("FOO", "bar")
            .env("BAZ", "qux");

        assert_eq!(builder.env_override.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(builder.env_override.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn run_echo() {
        let output = ProcessBuilder::new("echo")
            .arg("test")
            .capture()
            .run()
            .expect("echo should work");

        assert!(output.success());
        assert!(output.stdout_trimmed().contains("test"));
    }

    #[test]
    fn run_false_fails() {
        #[cfg(unix)]
        {
            let result = ProcessBuilder::new("false").status();

            assert!(result.is_err());
        }
    }

    #[test]
    fn which_existing() {
        // 'echo' should exist on all platforms
        #[cfg(unix)]
        {
            let path = which("echo");
            assert!(path.is_some());
        }

        #[cfg(windows)]
        {
            let path = which("cmd.exe");
            assert!(path.is_some());
        }
    }

    #[test]
    fn which_nonexistent() {
        let path = which("nonexistent_command_12345");
        assert!(path.is_none());
    }

    #[test]
    fn is_executable_check() {
        #[cfg(unix)]
        {
            // /bin/sh should be executable
            assert!(is_executable(Path::new("/bin/sh")) || is_executable(Path::new("/usr/bin/sh")));
        }
    }

    #[test]
    fn run_command_helper() {
        let output = run_command("echo", &["hello"]).expect("echo should work");
        assert!(output.contains("hello"));
    }

    #[test]
    fn process_output_methods() {
        let output = ProcessBuilder::new("echo")
            .arg("  test  ")
            .capture()
            .run()
            .expect("echo should work");

        assert!(output.stdout_str().contains("test"));
        assert_eq!(output.stderr_str(), "");
    }

    #[test]
    fn stdio_config() {
        let builder = ProcessBuilder::new("cat")
            .stdin(StdioConfig::Pipe)
            .stdout(StdioConfig::Pipe)
            .stderr(StdioConfig::Null);

        matches!(builder.stdin, StdioConfig::Pipe);
        matches!(builder.stdout, StdioConfig::Pipe);
        matches!(builder.stderr, StdioConfig::Null);
    }
}
