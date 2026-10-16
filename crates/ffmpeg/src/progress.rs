//! Progress math: converting FFmpeg output into a 0-99 percentage.
//! Port of the parsing half of `src/util/ffmpeg/runner.py`.

/// `-progress` emits `key=value` lines; this one holds processed
/// microseconds (`out_time_us=`).
pub const PROGRESS_TIME_KEY: &str = "out_time_us=";

/// Resolves the total duration used for percentage math.
///
/// FFmpeg's self-reported duration and the caller's preset are kept
/// separate on purpose: upstream APIs report duration in inconsistent
/// units (some milliseconds). Mixing them under one max() lets an
/// inflated preset pin the percentage at 0 forever. The probed value
/// fully takes over once FFmpeg reports one.
#[derive(Debug, Default)]
pub struct DurationResolution {
    /// Caller preset, in seconds; a lower-bound fallback only.
    duration: f64,
    /// Longest `Duration:` FFmpeg printed across its (multi-input) runs.
    probed_duration: f64,
}

impl DurationResolution {
    /// Preset in seconds; ignored when zero/negative (same guard as the
    /// Python `set_duration`).
    pub fn set_duration(&mut self, duration: f64) {
        if duration > 0.0 {
            self.duration = duration;
        }
    }

    pub fn total(&self) -> f64 {
        if self.probed_duration > 0.0 {
            self.probed_duration
        } else {
            self.duration
        }
    }

    /// Feed one stderr line. Every `Duration: H:MM:SS.f` occurrence is
    /// examined because merges have several inputs (video, audio, cover)
    /// and the longest input is the output's real duration; `N/A` and
    /// other unparseable values are skipped, matching the Python regex.
    pub fn parse_stderr_line(&mut self, line: &str) {
        for seconds in parse_durations(line) {
            // Max only within the probed group, never against the preset.
            if seconds > self.probed_duration {
                self.probed_duration = seconds;
            }
        }
    }

    /// One `-progress` stdout line → new percentage, or `None` when no
    /// total is known or the line carries nothing usable.
    ///
    /// Capped at 99: the real tail (renaming, temp-file cleanup) follows
    /// FFmpeg, and sitting at 100% while that happens reads as a hang.
    pub fn parse_progress_line(&mut self, line: &str) -> Option<u32> {
        let value = line.strip_prefix(PROGRESS_TIME_KEY)?.trim();
        // N/A until actual output exists.
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let micros: u64 = value.parse().ok()?;

        let duration = self.total();
        if duration <= 0.0 {
            // concat inputs report `Duration: N/A` and some upstreams give
            // no duration at all: percentage is uncomputable, not an error.
            return None;
        }

        let progress = micros as f64 / 1_000_000.0 / duration * 100.0;
        Some(progress.min(99.0) as u32)
    }
}

/// Every `Duration: H+:MM:SS.f+` in the line, as seconds. ffmpeg's shape
/// is fixed, so a scanner beats a regex dependency.
// ponytail: fixed-format scanner; swap for a regex crate only if ffmpeg
// ever emits a duration form this misses.
fn parse_durations(line: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(idx) = rest.find("Duration:") {
        rest = &rest[idx + "Duration:".len()..];
        let mut pos = rest.len() - rest.trim_start().len();
        let mut groups: [&str; 3] = ["", "", ""];
        for (slot, sep) in groups.iter_mut().zip([':', ':', '.']) {
            let Some(end) = rest[pos..].find(|c: char| !c.is_ascii_digit()) else {
                return out;
            };
            let field = &rest[pos..pos + end];
            if field.is_empty() || !rest[pos + end..].starts_with(sep) {
                return out;
            }
            *slot = field;
            pos += end + 1;
        }
        let fraction_end = rest[pos..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| pos + i)
            .unwrap_or(rest.len());
        let fraction = &rest[pos..fraction_end];
        if fraction.is_empty() {
            return out;
        }
        let (h, m, s): (f64, f64, f64) = (
            groups[0].parse().unwrap_or(0.0),
            groups[1].parse().unwrap_or(0.0),
            groups[2].parse().unwrap_or(0.0),
        );
        let frac: f64 = fraction.parse().unwrap_or(0.0);
        out.push(h * 3600.0 + m * 60.0 + s + frac / 10f64.powi(fraction.len() as i32));
        rest = &rest[fraction_end..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_scanned_from_typical_stderr() {
        let line = "  Duration: 00:01:40.00, start: 0.000000, bitrate: 1234 kb/s";
        assert_eq!(parse_durations(line), vec![100.0]);
    }

    #[test]
    fn durations_with_multi_digit_hours() {
        assert_eq!(parse_durations("Duration: 12:34:56.789, x"), vec![45296.789]);
    }

    #[test]
    fn na_duration_yields_nothing() {
        assert!(parse_durations("  Duration: N/A, start: 0.000000").is_empty());
    }
}
