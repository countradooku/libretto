//! Cross-platform signal handling.
//!
//! Provides unified interface for:
//! - Unix: SIGINT, SIGTERM, SIGHUP, etc.
//! - Windows: Ctrl+C, Ctrl+Break

#![allow(unsafe_code)]

use crate::{PlatformError, Result};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Global signal state.
static SIGNAL_STATE: Lazy<SignalState> = Lazy::new(SignalState::new);

/// Signal state container.
struct SignalState {
    /// Whether a termination signal was received.
    terminated: AtomicBool,
    /// Whether an interrupt signal was received.
    interrupted: AtomicBool,
    /// Custom handlers.
    handlers: Mutex<Vec<Box<dyn Fn(SignalKind) + Send + Sync>>>,
}

impl SignalState {
    fn new() -> Self {
        Self {
            terminated: AtomicBool::new(false),
            interrupted: AtomicBool::new(false),
            handlers: Mutex::new(Vec::new()),
        }
    }
}

/// Signal types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalKind {
    /// Interrupt (Ctrl+C on all platforms, SIGINT on Unix).
    Interrupt,
    /// Terminate (SIGTERM on Unix, Ctrl+Break on Windows).
    Terminate,
    /// Hangup (SIGHUP on Unix, not available on Windows).
    Hangup,
    /// User signal 1 (SIGUSR1 on Unix, not available on Windows).
    User1,
    /// User signal 2 (SIGUSR2 on Unix, not available on Windows).
    User2,
    /// Child process signal (SIGCHLD on Unix).
    Child,
    /// Window size change (SIGWINCH on Unix).
    WindowChange,
    /// Alarm (SIGALRM on Unix).
    Alarm,
    /// Pipe broken (SIGPIPE on Unix).
    Pipe,
}

impl SignalKind {
    /// Check if this signal is available on the current platform.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        match self {
            Self::Interrupt | Self::Terminate => true,
            #[cfg(unix)]
            Self::Hangup
            | Self::User1
            | Self::User2
            | Self::Child
            | Self::WindowChange
            | Self::Alarm
            | Self::Pipe => true,
            #[cfg(not(unix))]
            _ => false,
        }
    }

    /// Get Unix signal number if applicable.
    #[cfg(unix)]
    #[must_use]
    pub const fn unix_signal(&self) -> Option<i32> {
        match self {
            Self::Interrupt => Some(libc::SIGINT),
            Self::Terminate => Some(libc::SIGTERM),
            Self::Hangup => Some(libc::SIGHUP),
            Self::User1 => Some(libc::SIGUSR1),
            Self::User2 => Some(libc::SIGUSR2),
            Self::Child => Some(libc::SIGCHLD),
            Self::WindowChange => Some(libc::SIGWINCH),
            Self::Alarm => Some(libc::SIGALRM),
            Self::Pipe => Some(libc::SIGPIPE),
        }
    }

    /// Get human-readable name.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Interrupt => "SIGINT",
            Self::Terminate => "SIGTERM",
            Self::Hangup => "SIGHUP",
            Self::User1 => "SIGUSR1",
            Self::User2 => "SIGUSR2",
            Self::Child => "SIGCHLD",
            Self::WindowChange => "SIGWINCH",
            Self::Alarm => "SIGALRM",
            Self::Pipe => "SIGPIPE",
        }
    }
}

impl std::fmt::Display for SignalKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Signal handler registration and management.
#[derive(Debug, Clone, Copy)]
pub struct SignalHandler {
    _private: (),
}

impl SignalHandler {
    /// Initialize signal handling for the application.
    ///
    /// This should be called early in main() to set up signal handlers.
    ///
    /// # Errors
    /// Returns error if signal handlers cannot be registered.
    pub fn init() -> Result<()> {
        #[cfg(unix)]
        {
            Self::init_unix()?;
        }
        #[cfg(windows)]
        {
            Self::init_windows()?;
        }
        Ok(())
    }

    #[cfg(unix)]
    fn init_unix() -> Result<()> {
        use nix::sys::signal::{self, SaFlags, SigAction, SigHandler, SigSet};

        // Set up signal handlers
        let handler = SigHandler::Handler(unix_signal_handler);
        let flags = SaFlags::SA_RESTART;
        let mask = SigSet::empty();
        let action = SigAction::new(handler, flags, mask);

        unsafe {
            signal::sigaction(signal::Signal::SIGINT, &action)
                .map_err(|e| PlatformError::Signal(e.to_string()))?;
            signal::sigaction(signal::Signal::SIGTERM, &action)
                .map_err(|e| PlatformError::Signal(e.to_string()))?;
            signal::sigaction(signal::Signal::SIGHUP, &action)
                .map_err(|e| PlatformError::Signal(e.to_string()))?;
        }

        // Ignore SIGPIPE by default (common for network applications)
        let ignore = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
        unsafe {
            signal::sigaction(signal::Signal::SIGPIPE, &ignore)
                .map_err(|e| PlatformError::Signal(e.to_string()))?;
        }

        Ok(())
    }

    #[cfg(windows)]
    fn init_windows() -> Result<()> {
        use windows_sys::Win32::System::Console::{
            SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_C_EVENT,
        };

        unsafe extern "system" fn handler(ctrl_type: u32) -> i32 {
            match ctrl_type {
                CTRL_C_EVENT => {
                    SIGNAL_STATE.interrupted.store(true, Ordering::SeqCst);
                    let handlers = SIGNAL_STATE.handlers.lock();
                    for handler in handlers.iter() {
                        handler(SignalKind::Interrupt);
                    }
                    1 // Handled
                }
                CTRL_BREAK_EVENT => {
                    SIGNAL_STATE.terminated.store(true, Ordering::SeqCst);
                    let handlers = SIGNAL_STATE.handlers.lock();
                    for handler in handlers.iter() {
                        handler(SignalKind::Terminate);
                    }
                    1 // Handled
                }
                _ => 0, // Not handled
            }
        }

        let result = unsafe { SetConsoleCtrlHandler(Some(handler), 1) };
        if result == 0 {
            return Err(PlatformError::Signal(
                "Failed to set console control handler".to_string(),
            ));
        }

        Ok(())
    }

    /// Register a custom signal handler.
    ///
    /// The handler will be called when the specified signal is received.
    /// Multiple handlers can be registered for the same signal.
    pub fn on_signal<F>(handler: F)
    where
        F: Fn(SignalKind) + Send + Sync + 'static,
    {
        SIGNAL_STATE.handlers.lock().push(Box::new(handler));
    }

    /// Check if an interrupt signal (Ctrl+C) was received.
    #[must_use]
    pub fn was_interrupted() -> bool {
        SIGNAL_STATE.interrupted.load(Ordering::SeqCst)
    }

    /// Check if a termination signal was received.
    #[must_use]
    pub fn was_terminated() -> bool {
        SIGNAL_STATE.terminated.load(Ordering::SeqCst)
    }

    /// Check if any shutdown signal was received.
    #[must_use]
    pub fn should_shutdown() -> bool {
        Self::was_interrupted() || Self::was_terminated()
    }

    /// Reset the interrupt flag.
    pub fn reset_interrupt() {
        SIGNAL_STATE.interrupted.store(false, Ordering::SeqCst);
    }

    /// Reset the termination flag.
    pub fn reset_terminate() {
        SIGNAL_STATE.terminated.store(false, Ordering::SeqCst);
    }

    /// Reset all signal flags.
    pub fn reset_all() {
        Self::reset_interrupt();
        Self::reset_terminate();
    }

    /// Wait for any shutdown signal (blocking with polling).
    ///
    /// Returns the signal that was received.
    #[cfg(unix)]
    pub fn wait_for_signal() -> SignalKind {
        use std::time::Duration;

        // Poll for signal flags
        loop {
            if Self::was_interrupted() {
                return SignalKind::Interrupt;
            }
            if Self::was_terminated() {
                return SignalKind::Terminate;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Send a signal to a process.
    ///
    /// # Errors
    /// Returns error if signal cannot be sent.
    #[cfg(unix)]
    pub fn send_signal(pid: u32, signal: SignalKind) -> Result<()> {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        let sig = match signal {
            SignalKind::Interrupt => Signal::SIGINT,
            SignalKind::Terminate => Signal::SIGTERM,
            SignalKind::Hangup => Signal::SIGHUP,
            SignalKind::User1 => Signal::SIGUSR1,
            SignalKind::User2 => Signal::SIGUSR2,
            SignalKind::Alarm => Signal::SIGALRM,
            SignalKind::Pipe => Signal::SIGPIPE,
            _ => {
                return Err(PlatformError::Signal(format!(
                    "Unsupported signal: {signal}"
                )))
            }
        };

        kill(Pid::from_raw(pid as i32), sig).map_err(|e| PlatformError::Signal(e.to_string()))
    }
}

/// Unix signal handler function.
#[cfg(unix)]
extern "C" fn unix_signal_handler(sig: libc::c_int) {
    let kind = match sig {
        libc::SIGINT => {
            SIGNAL_STATE.interrupted.store(true, Ordering::SeqCst);
            SignalKind::Interrupt
        }
        libc::SIGTERM => {
            SIGNAL_STATE.terminated.store(true, Ordering::SeqCst);
            SignalKind::Terminate
        }
        libc::SIGHUP => SignalKind::Hangup,
        _ => return,
    };

    // Call registered handlers
    // Note: This is called from signal context, so we need to be careful
    // Only call async-signal-safe functions
    let handlers = SIGNAL_STATE.handlers.lock();
    for handler in handlers.iter() {
        handler(kind);
    }
}

/// RAII guard for setting up Ctrl+C handling.
#[derive(Debug)]
pub struct CtrlCGuard {
    /// Flag to check if Ctrl+C was pressed.
    flag: Arc<AtomicBool>,
}

impl CtrlCGuard {
    /// Create a new Ctrl+C guard.
    #[must_use]
    pub fn new() -> Self {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&flag);

        SignalHandler::on_signal(move |kind| {
            if kind == SignalKind::Interrupt {
                flag_clone.store(true, Ordering::SeqCst);
            }
        });

        Self { flag }
    }

    /// Check if Ctrl+C was pressed.
    #[must_use]
    pub fn was_pressed(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Reset the flag.
    pub fn reset(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

impl Default for CtrlCGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_kind_available() {
        assert!(SignalKind::Interrupt.is_available());
        assert!(SignalKind::Terminate.is_available());
    }

    #[test]
    fn signal_kind_str() {
        assert_eq!(SignalKind::Interrupt.as_str(), "SIGINT");
        assert_eq!(SignalKind::Terminate.as_str(), "SIGTERM");
    }

    #[cfg(unix)]
    #[test]
    fn unix_signal_numbers() {
        assert_eq!(SignalKind::Interrupt.unix_signal(), Some(libc::SIGINT));
        assert_eq!(SignalKind::Terminate.unix_signal(), Some(libc::SIGTERM));
    }

    #[test]
    fn signal_flags() {
        // Reset any previous state
        SignalHandler::reset_all();

        assert!(!SignalHandler::was_interrupted());
        assert!(!SignalHandler::was_terminated());
        assert!(!SignalHandler::should_shutdown());
    }

    #[test]
    fn ctrl_c_guard() {
        let guard = CtrlCGuard::new();
        assert!(!guard.was_pressed());
    }
}
