//! Running FFmpeg: spawn with progress flags, read both pipes, watch for
//! hangs. Port of the process half of `src/util/ffmpeg/runner.py`.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::command::FFmpegCommand;
use crate::progress::DurationResolution;

/// Line-budget tails for diagnostics: with `-nostats` stderr carries only
/// occasional warnings (keep more), stdout is pure progress (keep less).
const STDERR_KEEP_LINES: usize = 500;
const STDOUT_KEEP_LINES: usize = 40;

/// Production watchdog thresholds (seconds), matching the Python build.
pub const NO_OUTPUT_TIMEOUT: f64 = 300.0;
pub const WATCHDOG_INTERVAL: f64 = 5.0;

/// Tunable watchdog — production values are the consts above; tests
/// inject second-scale ones.
#[derive(Debug, Clone, Copy)]
pub struct WatchdogConfig {
    pub no_output_timeout: Duration,
    pub interval: Duration,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            no_output_timeout: Duration::from_secs_f64(NO_OUTPUT_TIMEOUT),
            interval: Duration::from_secs_f64(WATCHDOG_INTERVAL),
        }
    }
}

/// Final report of one FFmpeg run, replacing the Qt signal trio.
#[derive(Debug)]
pub enum FFmpegOutcome {
    /// Exit code 0.
    Success { code: i32, stderr_tail: String },
    /// Non-zero exit; message uses the ported FFMPEG_FAILED_WITH_CODE
    /// wording.
    Failed {
        code: i32,
        message: String,
        stderr_tail: String,
    },
    /// Killed by the watchdog for producing no output; message uses the
    /// ported FFMPEG_NO_RESPONSE wording.
    NoResponse { stderr_tail: String },
    /// Spawn or I/O error before/while running; message uses the ported
    /// FFMPEG_FAILED wording.
    Error { message: String },
}

impl FFmpegOutcome {
    pub fn message(&self) -> String {
        match self {
            Self::Success { .. } => String::new(),
            Self::Failed { message, .. } => message.clone(),
            Self::NoResponse { .. } => crate::error_messages::FFMPEG_NO_RESPONSE.to_string(),
            Self::Error { message } => message.clone(),
        }
    }
}

/// One FFmpeg invocation.
pub struct FFmpegRunner {
    exe: PathBuf,
    argv: Vec<String>,
    cwd: Option<PathBuf>,
    watchdog: WatchdogConfig,
    preset_duration: f64,
}

impl FFmpegRunner {
    pub fn new(exe: impl Into<PathBuf>, argv: Vec<String>) -> Self {
        Self {
            exe: exe.into(),
            argv,
            cwd: None,
            watchdog: WatchdogConfig::default(),
            preset_duration: 0.0,
        }
    }

    pub fn from_command(exe: impl Into<PathBuf>, command: &FFmpegCommand) -> Self {
        Self::new(exe, command.build())
    }

    pub fn set_cwd(&mut self, cwd: impl Into<PathBuf>) -> &mut Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Preset media duration (seconds) so percentages work before FFmpeg
    /// prints its own `Duration:`; the probed value wins once reported.
    pub fn set_duration(&mut self, duration: f64) -> &mut Self {
        if duration > 0.0 {
            self.preset_duration = duration;
        }
        self
    }

    pub fn set_watchdog(&mut self, watchdog: WatchdogConfig) -> &mut Self {
        self.watchdog = watchdog;
        self
    }

    /// Final argv: executable first, then the progress flags, then the
    /// built command. `-progress` writes machine-readable progress to
    /// stdout and `-nostats` suppresses the human copy on stderr that
    /// would bury diagnostics; both are global options and must sit
    /// immediately after the binary. (`pipe:1` needs the pipe protocol,
    /// enabled in the bundled build.)
    pub fn exec_command(&self) -> Vec<String> {
        let mut cmd = Vec::with_capacity(self.argv.len() + 4);
        cmd.push(self.exe.to_string_lossy().into_owned());
        cmd.extend(["-progress", "pipe:1", "-nostats"].map(String::from));
        cmd.extend(self.argv.iter().skip(1).cloned());
        cmd
    }

    /// Spawn and drive FFmpeg to completion, invoking `on_progress` for
    /// each new percentage (0-99).
    pub fn run<F: FnMut(u32)>(&mut self, mut on_progress: F) -> FFmpegOutcome {
        let exec = self.exec_command();
        let mut command = Command::new(&exec[0]);
        command
            .args(&exec[1..])
            // FFmpeg reads stdin for interactive keys ([q]); this process
            // has no console, so the channel can only cause trouble.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        #[cfg(windows)]
        {
            // Keep the console window from flashing on every invocation.
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                return FFmpegOutcome::Error {
                    message: format!("{}: {e}", crate::error_messages::FFMPEG_FAILED),
                }
            }
        };

        // Shared duration state: the stderr thread records FFmpeg's
        // self-reported duration, the stdout thread resolves percentages
        // against it. A slightly stale snapshot only skips one tick — the
        // probed value fully takes over the preset once present.
        let duration = Arc::new(Mutex::new(DurationResolution::default()));
        duration.lock().set_duration(self.preset_duration);

        let last_output = Arc::new(Mutex::new(Instant::now()));
        let fired = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));

        let watchdog = spawn_watchdog(
            child.id(),
            Arc::clone(&last_output),
            Arc::clone(&fired),
            Arc::clone(&stop),
            self.watchdog,
        );

        // Both pipes need a reader: either filling its OS buffer
        // deadlocks FFmpeg inside a write call. stderr drains on its own
        // thread while the calling thread reads stdout — with `-nostats`
        // stderr is quiet, so it must never block the stdout path.
        let stderr_reader = read_stderr(child.stderr.take(), &last_output, &duration);
        read_stdout(
            child.stdout.take(),
            &last_output,
            &duration,
            &mut on_progress,
        );
        let stderr_tail = stderr_reader.join().unwrap_or_default();

        stop.store(true, Ordering::Release);
        let _ = watchdog.join();

        // Both pipes are at EOF so the process is normally exiting; cap
        // the wait so a stuck final write can't pin this thread.
        let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);

        if fired.load(Ordering::Acquire) {
            // Killed for silence: the exit code only says "terminated";
            // reporting "exit code N" from that misleads.
            return FFmpegOutcome::NoResponse { stderr_tail };
        }
        if code == 0 {
            FFmpegOutcome::Success { code, stderr_tail }
        } else {
            FFmpegOutcome::Failed {
                code,
                message: crate::error_messages::FFMPEG_FAILED_WITH_CODE
                    .replace("{code}", &code.to_string()),
                stderr_tail,
            }
        }
    }
}

fn spawn_watchdog(
    pid: u32,
    last_output: Arc<Mutex<Instant>>,
    fired: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    config: WatchdogConfig,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        std::thread::sleep(config.interval);
        if stop.load(Ordering::Acquire) {
            return;
        }
        // ponytail: no liveness probe — signaling an already-dead pid is a
        // harmless no-op; probe with waitpid only if that ever matters.
        if last_output.lock().elapsed() >= config.no_output_timeout {
            // Flag first, then kill: run() distinguishes "killed for
            // silence" from "genuinely failed" by this bit.
            fired.store(true, Ordering::Release);
            terminate(pid);
            return;
        }
    })
}

/// SIGTERM on unix, TerminateProcess on Windows — same semantics as the
/// Python `proc.terminate()`. A pid that already exited is a no-op.
fn terminate(pid: u32) {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    }
    #[cfg(windows)]
    {
        // taskkill /T takes the process tree (ffmpeg never spawns
        // children today, but the flag costs nothing and matches
        // TerminateProcess-with-tree semantics on a stuck process).
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Read stderr on a side thread, feeding the shared duration state and
/// keeping a bounded tail.
fn read_stderr(
    stderr: Option<std::process::ChildStderr>,
    last_output: &Arc<Mutex<Instant>>,
    duration: &Arc<Mutex<DurationResolution>>,
) -> std::thread::JoinHandle<String> {
    let Some(stderr) = stderr else {
        return std::thread::spawn(String::new);
    };
    let last_output = Arc::clone(last_output);
    let duration = Arc::clone(duration);
    std::thread::spawn(move || {
        let mut tail: std::collections::VecDeque<String> =
            std::collections::VecDeque::with_capacity(STDERR_KEEP_LINES);
        for line in BufReader::new(stderr).split(b'\n') {
            let Ok(bytes) = line else { break };
            *last_output.lock() = Instant::now();
            let line = String::from_utf8_lossy(&bytes).into_owned();
            duration.lock().parse_stderr_line(&line);
            if tail.len() == STDERR_KEEP_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
        // Pipe closed: not silence, just the end of output (see
        // read_stdout's matching note).
        *last_output.lock() = Instant::now();
        tail.make_contiguous().join("\n")
    })
}

/// Read stdout progress lines on the calling thread, emitting each new
/// percentage.
fn read_stdout<F: FnMut(u32)>(
    stdout: Option<std::process::ChildStdout>,
    last_output: &Arc<Mutex<Instant>>,
    duration: &Arc<Mutex<DurationResolution>>,
    on_progress: &mut F,
) {
    let Some(stdout) = stdout else { return };
    let mut tail: std::collections::VecDeque<String> =
        std::collections::VecDeque::with_capacity(STDOUT_KEEP_LINES);
    let mut last_progress: Option<u32> = None;
    for line in BufReader::new(stdout).split(b'\n') {
        let Ok(bytes) = line else { break };
        *last_output.lock() = Instant::now();
        let line = String::from_utf8_lossy(&bytes).into_owned();
        if tail.len() == STDOUT_KEEP_LINES {
            tail.pop_front();
        }
        tail.push_back(line.clone());
        let progress = duration.lock().parse_progress_line(&line);
        if let Some(progress) = progress {
            if last_progress != Some(progress) {
                last_progress = Some(progress);
                on_progress(progress);
            }
        }
    }
    // Both pipes drained: FFmpeg has closed up shop, and its final
    // flush/cleanup is ordinary work, not a hang. Without this the
    // watchdog would misread the post-EOF stretch (pipes closed, nothing
    // left to read) as silence and kill a succeeding run.
    *last_output.lock() = Instant::now();
    drop(tail);
}
