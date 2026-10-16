//! Locating the ffmpeg executable.
//!
//! Port of `src/util/ffmpeg/__init__.py`: resolution order comes from the
//! config's ffmpeg source (bundled/system/custom), falling back along
//! bundled → system → custom when the preferred source is missing.

use std::path::{Path, PathBuf};

/// Which ffmpeg source the config asks for. Serialized values match the
/// Python `FFmpegSource` enum ("bundled"/"system"/"custom" in config.json).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FFmpegSource {
    Bundled,
    System,
    Custom,
}

impl FFmpegSource {
    pub fn from_config(value: &str) -> Option<Self> {
        match value {
            "bundled" => Some(Self::Bundled),
            "system" => Some(Self::System),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// Per-platform executable name (`ffmpeg.exe` on Windows, `ffmpeg`
/// elsewhere), same as the Python module-level `ffmpeg_executable`.
pub fn ffmpeg_executable() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// Where the bundled ffmpeg lives: `<install_root>/bundle/ffmpeg(.exe)`.
/// `install_root` mirrors the Python `Directory.get_cwd()` (the app's own
/// directory, not the user-data directory).
pub fn bundled_ffmpeg_path(install_root: &Path) -> PathBuf {
    install_root.join("bundle").join(ffmpeg_executable())
}

/// Everything the resolution step can report to the caller.
#[derive(Debug)]
pub struct ResolvedFFmpeg {
    /// Absolute path to the executable that will be invoked.
    pub path: PathBuf,
    /// Which source actually satisfied the request (after fallbacks).
    pub source: FFmpegSource,
}

/// The config inputs resolution needs, without depending on the config
/// crate (ticket 02 owns config.json parsing; its keys feed this).
#[derive(Debug, Clone)]
pub struct FFmpegSelection {
    pub source: FFmpegSource,
    /// Empty when not a custom selection.
    pub custom_path: PathBuf,
    /// The app's own directory, where `bundle/` lives.
    pub install_root: PathBuf,
}

impl FFmpegSelection {
    /// Resolve per the Python `init_ffmpeg()` fallback chain.
    ///
    /// Bundled: `bundle/ffmpeg`, falling back to system PATH.
    /// System: PATH, falling back to the bundle.
    /// Custom: the configured path, falling back to bundle, then system.
    ///
    /// Returns `Err` with the surfaced missing-binary message when nothing
    /// is found — the caller shows this to the user instead of proceeding.
    pub fn resolve(&self) -> anyhow::Result<ResolvedFFmpeg> {
        let bundled = bundled_ffmpeg_path(&self.install_root);
        let bundled_ok = bundled.is_file();

        let (path, source) = match self.source {
            FFmpegSource::Bundled => {
                if bundled_ok {
                    (bundled, FFmpegSource::Bundled)
                } else if let Some(sys) = which_ffmpeg() {
                    (sys, FFmpegSource::System)
                } else {
                    return Err(anyhow::anyhow!("{MISSING_MESSAGE}"));
                }
            }
            FFmpegSource::System => {
                if let Some(sys) = which_ffmpeg() {
                    (sys, FFmpegSource::System)
                } else if bundled_ok {
                    (bundled, FFmpegSource::Bundled)
                } else {
                    return Err(anyhow::anyhow!("{MISSING_MESSAGE}"));
                }
            }
            FFmpegSource::Custom => {
                if self.custom_path.is_file() {
                    (self.custom_path.clone(), FFmpegSource::Custom)
                } else if bundled_ok {
                    (bundled, FFmpegSource::Bundled)
                } else if let Some(sys) = which_ffmpeg() {
                    (sys, FFmpegSource::System)
                } else {
                    return Err(anyhow::anyhow!("{MISSING_MESSAGE}"));
                }
            }
        };

        Ok(ResolvedFFmpeg { path, source })
    }
}

/// Message surfaced when no ffmpeg binary can be located.
pub const MISSING_MESSAGE: &str =
    "No usable FFmpeg executable was found (not bundled, not on PATH, no valid custom path)";

/// Find ffmpeg on the system PATH, honoring `PATH` like a fresh shell.
fn which_ffmpeg() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(ffmpeg_executable());
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
