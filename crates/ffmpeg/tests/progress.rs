//! Progress math, driven with canned `-progress pipe:1` stdout lines and
//! ffmpeg stderr lines. Port of `test/test_ffmpeg_runner.py`'s
//! TestDurationResolution vectors, reused verbatim.

use ffmpeg::progress::DurationResolution;

fn feed_stderr(d: &mut DurationResolution, durations: &[&str]) {
    for value in durations {
        d.parse_stderr_line(&format!(
            "  Duration: {value}, start: 0.000000, bitrate: 1234 kb/s\n"
        ));
    }
}

#[test]
fn probed_duration_overrides_inflated_preset() {
    // Preset inflated 1000x (milliseconds passed as seconds); ffmpeg's
    // real duration is 100s. A max()-merge would stay on 100000 forever
    // and the percentage would read 0 the whole way.
    let mut d = DurationResolution::default();
    d.set_duration(100_000.0);
    feed_stderr(&mut d, &["00:01:40.00"]);
    assert_eq!(d.parse_progress_line("out_time_us=50000000\n"), Some(50));
}

#[test]
fn preset_is_used_until_ffmpeg_reports() {
    // Before ffmpeg prints its input info there is no probed duration;
    // the preset must carry the first seconds or progress starts empty.
    let mut d = DurationResolution::default();
    d.set_duration(100.0);
    assert_eq!(d.parse_progress_line("out_time_us=25000000\n"), Some(25));
}

#[test]
fn longest_reported_input_wins() {
    // Merges have several inputs (video, audio, cover); each prints its
    // own Duration, and image inputs are tiny. The longest is the output
    // file's real duration.
    let mut d = DurationResolution::default();
    feed_stderr(&mut d, &["00:00:00.04", "00:01:40.00", "00:00:00.04"]);
    assert_eq!(d.parse_progress_line("out_time_us=50000000\n"), Some(50));
}

#[test]
fn unknown_duration_emits_nothing() {
    // concat inputs report `Duration: N/A` and some upstreams give no
    // duration at all: uncomputable, but never an exception.
    let mut d = DurationResolution::default();
    d.parse_stderr_line("  Duration: N/A, start: 0.000000, bitrate: N/A\n");
    assert_eq!(d.parse_progress_line("out_time_us=50000000\n"), None);
}

#[test]
fn progress_is_capped_below_100() {
    // Reserve the last 1%: renaming and temp-file cleanup follow ffmpeg,
    // and sitting at 100% while those run reads as a hang.
    let mut d = DurationResolution::default();
    d.set_duration(100.0);
    assert_eq!(d.parse_progress_line("out_time_us=100000000\n"), Some(99));
}

#[test]
fn na_progress_value_is_ignored() {
    // `-progress` writes N/A for this key until real output exists.
    let mut d = DurationResolution::default();
    d.set_duration(100.0);
    assert_eq!(d.parse_progress_line("out_time_us=N/A\n"), None);
}

#[test]
fn zero_and_negative_presets_are_rejected() {
    let mut d = DurationResolution::default();
    d.set_duration(0.0);
    d.set_duration(-5.0);
    assert_eq!(d.parse_progress_line("out_time_us=1000000\n"), None);
}
