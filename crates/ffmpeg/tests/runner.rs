//! End-to-end runner behavior against fake-ffmpeg script fixtures:
//! progress flows, watchdog kills the hung process, exit codes surface,
//! and the exec argv carries the progress flags. Unix-only (fixture is a
//! shell script; Windows ships real .exe fixtures with the bundle).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use ffmpeg::command::FFmpegCommand;
use ffmpeg::runner::{FFmpegOutcome, FFmpegRunner, WatchdogConfig};

/// Writes a fake ffmpeg shell script that ignores its arguments except
/// what `body` decides to do with stdout/stderr/exit.
fn fake_ffmpeg(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    let script = format!("#!/bin/sh\n{body}\n");
    std::fs::write(&path, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn fast_watchdog() -> WatchdogConfig {
    WatchdogConfig {
        // Production is 300s; tests run second-scale.
        no_output_timeout: Duration::from_millis(700),
        interval: Duration::from_millis(50),
    }
}

/// Records the invocation argv for inspection by echoing it to stderr.
fn recording_ffmpeg(dir: &Path) -> PathBuf {
    fake_ffmpeg(dir, "ffmpeg-record", r#"printf '%s\n' "$@" >&2"#)
}

#[test]
fn exec_command_inserts_progress_flags_after_the_binary() {
    let cmd = FFmpegCommand::remux_audio("in.m4a".as_ref(), "out.m4a".as_ref());
    let runner = FFmpegRunner::from_command("/usr/bin/ffmpeg", &cmd);
    assert_eq!(
        runner.exec_command(),
        vec![
            "/usr/bin/ffmpeg",
            "-progress",
            "pipe:1",
            "-nostats",
            "-y",
            "-i",
            "in.m4a",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-strict",
            "unofficial",
            "out.m4a",
        ]
    );
}

#[test]
fn progress_lines_become_percentages() {
    let dir = std::env::temp_dir().join("bilidown-t07-progress");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Realistic fake: stderr reports a 100s input; stdout emits three
    // -progress groups (ffmpeg writes one group per ~0.5s).
    let exe = fake_ffmpeg(
        &dir,
        "ffmpeg-progress",
        r#"
        echo "  Duration: 00:01:40.00, start: 0.000000, bitrate: 1234 kb/s" >&2
        printf 'frame=1\nout_time_us=25000000\nprogress=continue\n'
        sleep 0.05
        printf 'frame=2\nout_time_us=50000000\nprogress=continue\n'
        sleep 0.05
        printf 'frame=3\nout_time_us=100000000\nprogress=end\n'
        "#,
    );

    let mut runner = FFmpegRunner::new(&exe, vec!["ffmpeg".into(), "-y".into()]);
    runner.set_watchdog(WatchdogConfig {
        no_output_timeout: Duration::from_secs(5),
        interval: Duration::from_millis(50),
    });
    let mut seen = Vec::new();
    let outcome = runner.run(|p| seen.push(p));

    assert!(
        matches!(outcome, FFmpegOutcome::Success { code: 0, .. }),
        "{outcome:?}"
    );
    // 25%, 50%, then 99% (capped below 100).
    assert_eq!(seen, vec![25, 50, 99]);
}

#[test]
fn preset_duration_carries_progress_before_stderr_reports() {
    let dir = std::env::temp_dir().join("bilidown-t07-preset");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // No Duration on stderr at all; the preset must carry the math.
    let exe = fake_ffmpeg(&dir, "ffmpeg-preset", "printf 'out_time_us=50000000\\n'");

    let mut runner = FFmpegRunner::new(&exe, vec!["ffmpeg".into()]);
    runner.set_duration(100.0);
    let mut seen = Vec::new();
    let outcome = runner.run(|p| seen.push(p));
    assert!(matches!(outcome, FFmpegOutcome::Success { .. }));
    assert_eq!(seen, vec![50]);
}

#[test]
fn watchdog_kills_a_silent_hung_process() {
    let dir = std::env::temp_dir().join("bilidown-t07-hang");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Sleeps forever with zero output on either pipe — the stuck-on-a-
    // syscall shape (output file locked by AV, sleeping external drive).
    // `exec` replaces the shell so the killed pid IS the process holding
    // the pipe, matching production (ffmpeg never forks).
    let exe = fake_ffmpeg(&dir, "ffmpeg-hang", "exec sleep 30");

    let mut runner = FFmpegRunner::new(&exe, vec!["ffmpeg".into()]);
    runner.set_watchdog(fast_watchdog());
    let started = std::time::Instant::now();
    let outcome = runner.run(|_| {});

    match outcome {
        FFmpegOutcome::NoResponse { stderr_tail } => {
            assert!(stderr_tail.is_empty());
        }
        other => panic!("expected NoResponse, got {other:?}"),
    }
    // Killed by the ~0.7s watchdog, not by the 30s sleep.
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn heartbeat_output_keeps_the_watchdog_calm() {
    let dir = std::env::temp_dir().join("bilidown-t07-heartbeat");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Slow but alive for ~10s: progress lines every 0.1s, with the
    // final line emitted immediately before exit (no trailing silent
    // stretch that could race the watchdog). A slow merge must not be
    // killed for being slow.
    let exe = fake_ffmpeg(
        &dir,
        "ffmpeg-heartbeat",
        "n=0\nwhile [ $n -lt 100 ]; do printf 'out_time_us=1000000\\n'; n=$((n+1)); sleep 0.1; done\nprintf 'progress=end\\n'",
    );

    let mut runner = FFmpegRunner::new(&exe, vec!["ffmpeg".into()]);
    runner.set_watchdog(fast_watchdog());
    let outcome = runner.run(|_| {});
    assert!(
        matches!(outcome, FFmpegOutcome::Success { .. }),
        "slow-but-alive must survive: {outcome:?}"
    );
}

#[test]
fn nonzero_exit_surfaces_failed_with_code() {
    let dir = std::env::temp_dir().join("bilidown-t07-fail");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = fake_ffmpeg(&dir, "ffmpeg-fail", "echo 'boom' >&2; exit 3");

    let mut runner = FFmpegRunner::new(&exe, vec!["ffmpeg".into()]);
    let outcome = runner.run(|_| {});
    match outcome {
        FFmpegOutcome::Failed {
            code,
            message,
            stderr_tail,
        } => {
            assert_eq!(code, 3);
            assert_eq!(message, "FFmpeg failed with exit code 3");
            assert!(stderr_tail.contains("boom"));
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn missing_binary_surfaces_spawn_error() {
    let mut runner = FFmpegRunner::new("/nonexistent/bilidown-t07/ffmpeg", vec!["ffmpeg".into()]);
    let outcome = runner.run(|_| {});
    match outcome {
        FFmpegOutcome::Error { message } => {
            assert!(message.contains("An error occurred while running FFmpeg"));
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn cwd_is_honored_for_relative_paths() {
    let dir = std::env::temp_dir().join("bilidown-t07-cwd");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Fake ffmpeg writes its marker into the cwd the runner set.
    let exe = fake_ffmpeg(&dir, "ffmpeg-cwd", "pwd > marker.txt");

    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let mut runner = FFmpegRunner::new(&exe, vec!["ffmpeg".into()]);
    runner.set_cwd(&sub);
    let outcome = runner.run(|_| {});
    assert!(matches!(outcome, FFmpegOutcome::Success { .. }));
    let marker = std::fs::read_to_string(sub.join("marker.txt")).unwrap();
    let canonical_sub = sub.canonicalize().unwrap();
    let written = PathBuf::from(marker.trim()).canonicalize().unwrap();
    assert_eq!(written, canonical_sub);
}

#[test]
fn spawned_process_receives_the_full_built_argv() {
    let dir = std::env::temp_dir().join("bilidown-t07-argv");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let exe = recording_ffmpeg(&dir);

    let cmd = FFmpegCommand::merge_video_audio(
        "v.m4s".as_ref(),
        "a.m4s".as_ref(),
        "out.mp4".as_ref(),
        None,
        None,
        &[],
    );
    let mut runner = FFmpegRunner::from_command(&exe, &cmd);
    let outcome = runner.run(|_| {});

    let FFmpegOutcome::Success { stderr_tail, .. } = outcome else {
        panic!("expected Success, got {outcome:?}")
    };
    let received: Vec<&str> = stderr_tail.lines().collect();
    // Progress flags lead; the builder's argv follows verbatim.
    assert_eq!(
        received,
        vec![
            "-progress",
            "pipe:1",
            "-nostats",
            "-y",
            "-i",
            "v.m4s",
            "-i",
            "a.m4s",
            "-c:v",
            "copy",
            "-c:a",
            "copy",
            "-strict",
            "unofficial",
            "out.mp4",
        ]
    );
}

#[test]
fn stdin_is_closed_so_ffmpeg_cannot_wait_on_keys() {
    let dir = std::env::temp_dir().join("bilidown-t07-stdin");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // `read` succeeds only if stdin is open; with stdin closed it fails
    // immediately. A blocking stdin would hang past the watchdog and
    // turn this into NoResponse.
    let exe = fake_ffmpeg(&dir, "ffmpeg-stdin", "read line || true");

    let mut runner = FFmpegRunner::new(&exe, vec!["ffmpeg".into()]);
    runner.set_watchdog(fast_watchdog());
    let outcome = runner.run(|_| {});
    assert!(
        matches!(outcome, FFmpegOutcome::Success { .. }),
        "stdin must be closed, not interactive: {outcome:?}"
    );
}
