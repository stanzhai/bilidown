//! Locating the executable: resolution order and the surfaced
//! missing-binary error, exercised with temp install roots and PATH.
//!
//! PATH is process-global: tests that mutate it serialize on a static
//! mutex so a sibling's Drop (restoring the real PATH mid-test) can't
//! leak the host's ffmpeg into `which_ffmpeg()`.
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use ffmpeg::locate::{
    bundled_ffmpeg_path, ffmpeg_executable, FFmpegSelection, FFmpegSource, MISSING_MESSAGE,
};

static PATH_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn path_lock() -> std::sync::MutexGuard<'static, ()> {
    PATH_LOCK.lock()
}

struct PathGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    old: Vec<PathBuf>,
}

impl PathGuard {
    /// Holds the serialization lock for the test's whole body, so the
    /// guard's Drop restores PATH before the next test starts.
    fn set(paths: Vec<PathBuf>) -> Self {
        let lock = path_lock();
        let old: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        let joined = std::env::join_paths(&paths).unwrap();
        std::env::set_var("PATH", &joined);
        Self { _lock: lock, old }
    }
}

impl Drop for PathGuard {
    fn drop(&mut self) {
        let joined = std::env::join_paths(&self.old).unwrap();
        std::env::set_var("PATH", joined);
    }
}

/// An empty dir keeps the resolution under test from finding the host's
/// real ffmpeg; anything on PATH must come from our fixture.
fn empty_path() -> PathGuard {
    PathGuard::set(vec![std::env::temp_dir().join("bilidown-t07-empty-path")])
}

fn make_executable(dir: &std::path::Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[test]
fn executable_name_matches_platform() {
    if cfg!(windows) {
        assert_eq!(ffmpeg_executable(), "ffmpeg.exe");
    } else {
        assert_eq!(ffmpeg_executable(), "ffmpeg");
    }
}

#[test]
fn bundled_path_is_install_root_bundle() {
    let p = bundled_ffmpeg_path(Path::new("/opt/app").as_ref());
    assert_eq!(p, PathBuf::from("/opt/app").join("bundle").join(ffmpeg_executable()));
}

#[test]
fn bundled_source_prefers_the_bundle() {
    let root = std::env::temp_dir().join("bilidown-t07-bundled");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bundle")).unwrap();
    make_executable(&root.join("bundle"), ffmpeg_executable());

    let _path = empty_path();
    let sel = FFmpegSelection {
        source: FFmpegSource::Bundled,
        custom_path: PathBuf::new(),
        install_root: root.clone(),
    };
    let resolved = sel.resolve().unwrap();
    assert_eq!(resolved.path, root.join("bundle").join(ffmpeg_executable()));
    assert_eq!(resolved.source, FFmpegSource::Bundled);
}

#[test]
fn missing_bundle_falls_back_to_system_path() {
    let root = std::env::temp_dir().join("bilidown-t07-no-bundle");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let sys = std::env::temp_dir().join("bilidown-t07-sys");
    std::fs::create_dir_all(&sys).unwrap();
    make_executable(&sys, ffmpeg_executable());

    let _path = PathGuard::set(vec![sys.clone()]);
    let sel = FFmpegSelection {
        source: FFmpegSource::Bundled,
        custom_path: PathBuf::new(),
        install_root: root,
    };
    let resolved = sel.resolve().unwrap();
    assert_eq!(resolved.source, FFmpegSource::System);
    assert!(resolved.path.starts_with(&sys));
}

#[test]
fn system_source_falls_back_to_bundle() {
    let root = std::env::temp_dir().join("bilidown-t07-sys-fb");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bundle")).unwrap();
    make_executable(&root.join("bundle"), ffmpeg_executable());

    let _path = empty_path();
    let sel = FFmpegSelection {
        source: FFmpegSource::System,
        custom_path: PathBuf::new(),
        install_root: root.clone(),
    };
    let resolved = sel.resolve().unwrap();
    assert_eq!(resolved.source, FFmpegSource::Bundled);
    assert_eq!(resolved.path, root.join("bundle").join(ffmpeg_executable()));
}

#[test]
fn custom_source_uses_configured_path() {
    let custom_dir = std::env::temp_dir().join("bilidown-t07-custom");
    let _ = std::fs::remove_dir_all(&custom_dir);
    std::fs::create_dir_all(&custom_dir).unwrap();
    let custom = make_executable(&custom_dir, ffmpeg_executable());

    let root = std::env::temp_dir().join("bilidown-t07-custom-root");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let _path = empty_path();
    let sel = FFmpegSelection {
        source: FFmpegSource::Custom,
        custom_path: custom.clone(),
        install_root: root,
    };
    let resolved = sel.resolve().unwrap();
    assert_eq!(resolved.source, FFmpegSource::Custom);
    assert_eq!(resolved.path, custom);
}

#[test]
fn invalid_custom_path_falls_back_to_bundle_then_system() {
    let root = std::env::temp_dir().join("bilidown-t07-custom-fb");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bundle")).unwrap();
    make_executable(&root.join("bundle"), ffmpeg_executable());

    let _path = empty_path();
    let sel = FFmpegSelection {
        source: FFmpegSource::Custom,
        custom_path: PathBuf::from("/nonexistent/ffmpeg"),
        install_root: root.clone(),
    };
    let resolved = sel.resolve().unwrap();
    assert_eq!(resolved.source, FFmpegSource::Bundled);
}

#[test]
fn nothing_found_surfaces_missing_binary_error() {
    let root = std::env::temp_dir().join("bilidown-t07-nothing");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let _path = empty_path();
    let sel = FFmpegSelection {
        source: FFmpegSource::System,
        custom_path: PathBuf::new(),
        install_root: root,
    };
    let err = sel.resolve().unwrap_err().to_string();
    assert_eq!(err, MISSING_MESSAGE);
}

#[test]
fn source_values_round_trip_from_config_strings() {
    assert_eq!(FFmpegSource::from_config("bundled"), Some(FFmpegSource::Bundled));
    assert_eq!(FFmpegSource::from_config("system"), Some(FFmpegSource::System));
    assert_eq!(FFmpegSource::from_config("custom"), Some(FFmpegSource::Custom));
    assert_eq!(FFmpegSource::from_config("nope"), None);
}
