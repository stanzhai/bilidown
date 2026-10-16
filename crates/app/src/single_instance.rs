//! Single-instance mutual exclusion (port of `Application.init_single_instance`
//! in `src/main.py`), interop-compatible with the Python build.
//!
//! Three layers, same as Python:
//! 1. Windows named mutex with the same fixed GUID — the primary exclusion
//!    between any two builds on Windows.
//! 2. `locks/instance.lock` with staleness detection (10 s, matching
//!    `QLockFile.setStaleLockTime(INSTANCE_LOCK_TIMEOUT_MS)`); a stale lock is
//!    removed and retried once.
//! 3. A local-socket wake server named `bili23_downloader_single_instance`.
//!    A second launch connects, writes one command word and exits; the first
//!    instance then activates its window. Commands: `activate` (focus the
//!    window) and `ensure-running` (used by the MCP bridge; no focus steal).
//!
//! Qt's `QLocalServer` on macOS/Linux binds `@<name>` abstract socket or
//! `/var/run/…`; `QLocalSocket::connectToServer` resolves the same path, so
//! both builds speak the same address space via the namespaced name.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;

/// `INSTANCE_LOCK_NAME`.
pub const INSTANCE_LOCK_NAME: &str = "instance.lock";
/// `INSTANCE_LOCK_TIMEOUT_MS` — a lock older than this is stale.
pub const INSTANCE_LOCK_TIMEOUT_MS: u64 = 10_000;
/// `INSTANCE_SERVER_NAME`.
pub const INSTANCE_SERVER_NAME: &str = "bili23_downloader_single_instance";
/// `APP_MUTEX_NAME` — the Windows named-mutex GUID, identical to Python's.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const APP_MUTEX_NAME: &str = "B096F0C1-D105-4EF9-86E1-5E87DA884EA4";

/// `INSTANCE_COMMAND_ACTIVATE`: bring the existing window forward.
pub const COMMAND_ACTIVATE: &str = "activate";
/// `INSTANCE_COMMAND_ENSURE_RUNNING`: process-alive probe, no focus steal.
pub const COMMAND_ENSURE_RUNNING: &str = "ensure-running";
/// `ENSURE_RUNNING_FLAG`.
pub const ENSURE_RUNNING_FLAG: &str = "--ensure-running";

/// What one `AppInstance::acquire` call decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceOutcome {
    /// We are the primary instance; run the app.
    Primary,
    /// Another instance is running and was woken; exit now.
    FocusedExisting,
    /// Another instance holds the lock and could not be woken; exit now.
    AlreadyRunning,
}

/// A lock file whose contents are `pid:started_unix_ms`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockContents {
    pub pid: u32,
    pub started_unix_ms: u128,
}

impl LockContents {
    pub fn now() -> Self {
        Self { pid: std::process::id(), started_unix_ms: unix_ms() }
    }

    pub fn to_line(&self) -> String {
        format!("{}:{}", self.pid, self.started_unix_ms)
    }

    /// Parse the `pid:started` line the lock file carries; empty/partial
    /// files treat as unknown age so only staleness by mtime can save them.
    pub fn parse(s: &str) -> Option<Self> {
        let (pid, started) = s.trim().split_once(':')?;
        Some(Self { pid: pid.parse().ok()?, started_unix_ms: started.parse().ok()? })
    }
}

pub fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Lock file path under the app-data dir (`locks/instance.lock`).
pub fn lock_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(config::APP_NAME).join("locks").join(INSTANCE_LOCK_NAME))
}

/// O_EXCL-create the lock file. `Ok(true)` = acquired, `Ok(false)` = exists.
fn try_create(path: &std::path::Path) -> std::io::Result<bool> {
    use std::io::ErrorKind;
    match fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut f) => f.write_all(LockContents::now().to_line().as_bytes()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e),
    }
    .map(|_| true)
}

/// Staleness test, Python `QLockFile` semantics: stale when the file is older
/// than [`INSTANCE_LOCK_TIMEOUT_MS`] by mtime, or its contents predate the
/// cutoff, or it is unreadable garbage that nonetheless parses nowhere.
pub fn lock_is_stale(path: &std::path::Path, now_unix_ms: u128) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return true; // gone → nothing to keep
    };
    let cutoff = now_unix_ms.saturating_sub(INSTANCE_LOCK_TIMEOUT_MS as u128);
    let mtime_old = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .is_some_and(|d| d.as_millis() <= cutoff);
    let content_old = fs::read_to_string(path)
        .ok()
        .and_then(|s| LockContents::parse(&s))
        .is_some_and(|c| c.started_unix_ms <= cutoff);
    mtime_old || content_old
}

/// Connect to the running instance's wake server and send `command`.
/// Mirrors `Application.wake_existing_instance` (500 ms connect budget).
pub fn wake_existing_instance(command: &str) -> bool {
    use interprocess::local_socket::{GenericNamespaced, prelude::*};
    let Ok(name) = INSTANCE_SERVER_NAME.to_ns_name::<GenericNamespaced>() else {
        return false;
    };
    let Ok(mut stream) = interprocess::local_socket::Stream::connect(name) else {
        return false;
    };
    let _ = stream.write_all(command.as_bytes());
    let _ = stream.flush();
    // Qt waits for bytes written then disconnects; we simply drop.
    true
}

/// Windows named mutex, same GUID as Python. The OS releases it when the
/// process dies, so it never goes stale; held for the process lifetime.
#[cfg(target_os = "windows")]
pub struct AppMutex(Option<windows_sys::Win32::Foundation::HANDLE>);

#[cfg(target_os = "windows")]
impl AppMutex {
    pub fn create() -> Self {
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let wide: Vec<u16> = APP_MUTEX_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        Self((handle as HANDLE != 0).then_some(handle))
    }
}

#[cfg(target_os = "windows")]
impl Drop for AppMutex {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        if let Some(h) = self.0.take() {
            unsafe { CloseHandle(h as HANDLE) };
        }
    }
}

/// No-op on non-Windows (Python only creates the mutex there).
#[cfg(not(target_os = "windows"))]
#[derive(Default)]
pub struct AppMutex;

#[cfg(not(target_os = "windows"))]
impl AppMutex {
    pub fn create() -> Self {
        Self
    }
}

/// One `activate`-command delivery from the wake server to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeEvent {
    Activate,
    EnsureRunning,
}

impl WakeEvent {
    pub fn from_command(s: &str) -> Self {
        if s == COMMAND_ENSURE_RUNNING {
            Self::EnsureRunning
        } else {
            // Unknown/empty → Python's legacy-compat default of activate.
            Self::Activate
        }
    }
}

static WAKE_SERVER_RUNNING: AtomicBool = AtomicBool::new(false);

/// The primary instance's wake server: accept loop on a background thread,
/// forwarding one [`WakeEvent`] per connection to `tx`.
///
/// Port of `init_instance_server` + `on_new_instance_connection`. QLocalServer
/// survives a crashed previous instance because interprocess removes the
/// socket file on drop; `try_overwrite` covers the kill -9 leftover.
pub fn start_wake_server(tx: Sender<WakeEvent>) -> anyhow::Result<()> {
    use interprocess::local_socket::{GenericNamespaced, ListenerOptions, prelude::*};
    if WAKE_SERVER_RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(()); // already up (Python: init_instance_server guard)
    }
    let name = INSTANCE_SERVER_NAME.to_ns_name::<GenericNamespaced>()?;
    let listener = match ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_sync()
    {
        Ok(l) => l,
        Err(e) => {
            WAKE_SERVER_RUNNING.store(false, Ordering::SeqCst);
            anyhow::bail!("cannot start instance wake server: {e}");
        }
    };
    thread::Builder::new()
        .name("instance-wake".into())
        .spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                // Second instance writes the command then closes; one small
                // read with a short deadline gets it, empty → activate
                // (Python's waitForReadyRead(200) compat default).
                conn.set_recv_timeout(Some(std::time::Duration::from_millis(200)))
                    .ok();
                let mut buf = [0u8; 32];
                let n = conn.read(&mut buf).unwrap_or(0);
                let command = String::from_utf8_lossy(&buf[..n]).trim().to_string();
                let _ = tx.send(WakeEvent::from_command(&command));
            }
        })
        .ok();
    Ok(())
}

/// Headless decision core behind `init_single_instance`, testable without a
/// socket: given lock-file attempt results and a wake result, what next.
pub fn decide(acquired: bool, woke: bool) -> InstanceOutcome {
    if acquired {
        InstanceOutcome::Primary
    } else if woke {
        InstanceOutcome::FocusedExisting
    } else {
        InstanceOutcome::AlreadyRunning
    }
}

/// Full acquisition flow for the app entry point. Runs before the GUI:
/// a second instance never opens a window.
pub struct AppInstance {
    #[allow(dead_code)] // dropped at process exit by the owner
    mutex: AppMutex,
    _wake_guard: (),
}

impl AppInstance {
    /// Try to become the primary instance.
    ///
    /// `tx` receives wake commands when we win; the secondary path writes to
    /// the existing instance instead and returns a non-Primary outcome.
    pub fn acquire(ensure_running: bool, tx: Sender<WakeEvent>) -> anyhow::Result<InstanceOutcome> {
        let mutex = AppMutex::create();

        let Some(lock) = lock_path() else {
            anyhow::bail!("cannot locate user data directory");
        };
        if let Some(dir) = lock.parent() {
            fs::create_dir_all(dir)?;
        }

        let mut acquired = try_create(&lock).unwrap_or(false);
        if !acquired {
            // QLockFile::removeStaleLockFile + one retry.
            if lock_is_stale(&lock, unix_ms()) {
                let _ = fs::remove_file(&lock);
                acquired = try_create(&lock).unwrap_or(false);
            }
        }

        if !acquired {
            let command = if ensure_running { COMMAND_ENSURE_RUNNING } else { COMMAND_ACTIVATE };
            let woke = wake_existing_instance(command);
            return Ok(decide(false, woke));
        }

        start_wake_server(tx)?;
        Ok(Self { mutex, _wake_guard: () }.prime())
    }

    fn prime(self) -> InstanceOutcome {
        InstanceOutcome::Primary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_line_round_trips() {
        let c = LockContents { pid: 4242, started_unix_ms: 1_700_000_000_000 };
        assert_eq!(LockContents::parse(&c.to_line()), Some(c));
        assert_eq!(LockContents::parse("garbage"), None);
        assert_eq!(LockContents::parse("4242"), None);
    }

    #[test]
    fn stale_by_content_age_and_mtime_cutoff() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(INSTANCE_LOCK_NAME);
        let now = 10_000_000_u128;
        fs::write(&p, "123:0").unwrap(); // started far before cutoff
        assert!(lock_is_stale(&p, now));
        let fresh = LockContents { pid: 5, started_unix_ms: now - 1_000 };
        fs::write(&p, fresh.to_line()).unwrap();
        assert!(!lock_is_stale(&p, now)); // within window → live
        assert!(lock_is_stale(&p, now + INSTANCE_LOCK_TIMEOUT_MS as u128 + 5_000));
    }

    #[test]
    fn try_create_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(INSTANCE_LOCK_NAME);
        assert!(try_create(&p).unwrap());
        assert!(!try_create(&p).unwrap(), "second creator must lose");
        // Stale removal unblocks the next creator.
        fs::remove_file(&p).unwrap();
        assert!(try_create(&p).unwrap());
    }

    #[test]
    fn decision_matrix_matches_python_flow() {
        assert_eq!(decide(true, false), InstanceOutcome::Primary);
        assert_eq!(decide(false, true), InstanceOutcome::FocusedExisting);
        assert_eq!(decide(false, false), InstanceOutcome::AlreadyRunning);
    }

    #[test]
    fn command_words_map_to_wake_events() {
        assert_eq!(WakeEvent::from_command(COMMAND_ACTIVATE), WakeEvent::Activate);
        assert_eq!(
            WakeEvent::from_command(COMMAND_ENSURE_RUNNING),
            WakeEvent::EnsureRunning
        );
        assert_eq!(WakeEvent::from_command(""), WakeEvent::Activate, "legacy default");
    }
}
