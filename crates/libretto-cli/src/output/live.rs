//! Beautiful Bun-style live progress display.
//!
//! Single-line animated progress that shows current operation.

use owo_colors::OwoColorize;
use parking_lot::Mutex;
use std::io::{Write, stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Spinner frames for smooth animation
const SPINNER_FRAMES: &[&str] = &["◐", "◓", "◑", "◒"];
const SPINNER_INTERVAL: Duration = Duration::from_millis(80);

/// Beautiful live progress display
pub struct LiveProgress {
    state: Arc<ProgressState>,
    handle: Option<std::thread::JoinHandle<()>>,
}

struct ProgressState {
    running: AtomicBool,
    phase: Mutex<Phase>,
    current_package: Mutex<String>,
    completed: AtomicUsize,
    total: AtomicUsize,
    cached: AtomicUsize,
    downloaded_bytes: AtomicU64,
    start_time: Instant,
}

#[derive(Clone)]
enum Phase {
    Resolving,
    Downloading,
    Linking,
    Done,
}

impl LiveProgress {
    /// Create and start a new live progress display
    pub fn new() -> Self {
        let state = Arc::new(ProgressState {
            running: AtomicBool::new(true),
            phase: Mutex::new(Phase::Resolving),
            current_package: Mutex::new(String::new()),
            completed: AtomicUsize::new(0),
            total: AtomicUsize::new(0),
            cached: AtomicUsize::new(0),
            downloaded_bytes: AtomicU64::new(0),
            start_time: Instant::now(),
        });

        let state_clone = Arc::clone(&state);
        let handle = std::thread::spawn(move || {
            render_loop(state_clone);
        });

        Self {
            state,
            handle: Some(handle),
        }
    }

    /// Set resolving phase
    pub fn set_resolving(&self) {
        *self.state.phase.lock() = Phase::Resolving;
    }

    /// Set downloading phase with total count
    pub fn set_downloading(&self, total: usize, cached: usize) {
        self.state.total.store(total, Ordering::Relaxed);
        self.state.cached.store(cached, Ordering::Relaxed);
        self.state.completed.store(0, Ordering::Relaxed);
        *self.state.phase.lock() = Phase::Downloading;
    }

    /// Set linking phase (all from cache)
    pub fn set_linking(&self, total: usize) {
        self.state.total.store(total, Ordering::Relaxed);
        self.state.cached.store(total, Ordering::Relaxed); // All are cached
        self.state.completed.store(0, Ordering::Relaxed);
        *self.state.phase.lock() = Phase::Linking;
    }

    /// Update current package being processed
    pub fn set_current(&self, package: &str) {
        *self.state.current_package.lock() = package.to_string();
    }

    /// Increment completed count
    pub fn inc_completed(&self) {
        self.state.completed.fetch_add(1, Ordering::Relaxed);
    }

    /// Add downloaded bytes
    pub fn add_bytes(&self, bytes: u64) {
        self.state
            .downloaded_bytes
            .fetch_add(bytes, Ordering::Relaxed);
    }

    /// Finish with success
    pub fn finish_success(&self, message: &str) {
        // Stop the render loop first
        self.state.running.store(false, Ordering::Relaxed);

        // Wait for render thread to stop
        std::thread::sleep(std::time::Duration::from_millis(50));

        *self.state.phase.lock() = Phase::Done;

        // Get final stats for summary
        let total = self.state.total.load(Ordering::Relaxed);
        let cached = self.state.cached.load(Ordering::Relaxed);
        let bytes = self.state.downloaded_bytes.load(Ordering::Relaxed);

        // Clear line and print success with stats
        print!("\r\x1b[K");
        if total > 0 {
            let downloaded = total.saturating_sub(cached);
            let stats = if downloaded > 0 && bytes > 0 {
                format!(
                    " {} packages ({} downloaded, {} cached, {})",
                    total,
                    downloaded,
                    cached,
                    format_bytes(bytes)
                )
            } else if cached > 0 {
                format!(" {total} packages (from cache)")
            } else {
                format!(" {total} packages")
            };
            println!("{} {}{}", "✔".green().bold(), message, stats.dimmed());
        } else {
            println!("{} {}", "✔".green().bold(), message);
        }
        let _ = stdout().flush();
    }

    /// Finish with error
    pub fn finish_error(&self, message: &str) {
        *self.state.phase.lock() = Phase::Done;
        self.state.running.store(false, Ordering::Relaxed);

        // Clear line and print error
        print!("\r\x1b[K");
        println!("{} {}", "✖".red().bold(), message.red());
        let _ = stdout().flush();
    }

    /// Stop without message
    pub fn stop(&self) {
        self.state.running.store(false, Ordering::Relaxed);
        print!("\r\x1b[K");
        let _ = stdout().flush();
    }
}

impl Drop for LiveProgress {
    fn drop(&mut self) {
        self.state.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn render_loop(state: Arc<ProgressState>) {
    let mut frame = 0usize;
    let mut last_render = Instant::now();

    while state.running.load(Ordering::Relaxed) {
        if last_render.elapsed() >= SPINNER_INTERVAL {
            render_frame(&state, frame);
            frame = (frame + 1) % SPINNER_FRAMES.len();
            last_render = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(16));
    }

    // Final clear
    print!("\r\x1b[K");
    let _ = stdout().flush();
}

fn render_frame(state: &ProgressState, frame: usize) {
    let spinner = SPINNER_FRAMES[frame];
    let phase = state.phase.lock().clone();
    let elapsed = state.start_time.elapsed();
    let elapsed_str = format_duration(elapsed);

    let line = match phase {
        Phase::Resolving => {
            let pkg = state.current_package.lock();
            if pkg.is_empty() {
                format!(
                    "{} {} {}",
                    spinner.cyan().bold(),
                    "Resolving dependencies...".white(),
                    elapsed_str.dimmed()
                )
            } else {
                format!(
                    "{} {} {} {}",
                    spinner.cyan().bold(),
                    "Resolving".white(),
                    pkg.cyan(),
                    elapsed_str.dimmed()
                )
            }
        }
        Phase::Downloading => {
            let completed = state.completed.load(Ordering::Relaxed);
            let total = state.total.load(Ordering::Relaxed);
            let cached = state.cached.load(Ordering::Relaxed);
            let bytes = state.downloaded_bytes.load(Ordering::Relaxed);
            let pkg = state.current_package.lock();

            let progress_bar = render_progress_bar(completed, total, 20);
            let bytes_str = format_bytes(bytes);

            // Truncate package name if too long
            let pkg_display = if pkg.len() > 25 {
                format!("{}...", &pkg[..22])
            } else if pkg.is_empty() {
                "...".to_string()
            } else {
                pkg.to_string()
            };

            // Show phase based on progress vs cached count
            let phase_text = if completed < cached {
                "Linking".white()
            } else {
                "Installing".white()
            };

            format!(
                "{} {} {} {} {}/{} {} {}",
                spinner.green().bold(),
                phase_text,
                pkg_display.cyan(),
                progress_bar,
                completed.to_string().green(),
                total.to_string().white(),
                bytes_str.dimmed(),
                elapsed_str.dimmed()
            )
        }
        Phase::Linking => {
            let completed = state.completed.load(Ordering::Relaxed);
            let total = state.total.load(Ordering::Relaxed);
            let progress_bar = render_progress_bar(completed, total, 20);

            format!(
                "{} {} {} {}/{} {}",
                spinner.magenta().bold(),
                "Linking from cache".white(),
                progress_bar,
                completed.to_string().magenta(),
                total.to_string().white(),
                elapsed_str.dimmed()
            )
        }
        Phase::Done => String::new(),
    };

    print!("\r\x1b[K{line}");
    let _ = stdout().flush();
}

fn render_progress_bar(current: usize, total: usize, width: usize) -> String {
    if total == 0 {
        return format!("[{}]", " ".repeat(width));
    }

    let progress = (current as f64 / total as f64).min(1.0);
    let filled = (progress * width as f64) as usize;
    let empty = width - filled;

    format!(
        "{}{}{}{}",
        "[".dimmed(),
        "█".repeat(filled).green(),
        "░".repeat(empty).dimmed(),
        "]".dimmed()
    )
}

fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs_f64();
    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else {
        format!("{secs:.1}s")
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;

    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    }
}
