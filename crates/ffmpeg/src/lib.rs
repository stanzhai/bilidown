//! FFmpeg subprocess wrapper.
//!
//! Locates the ffmpeg executable (bundled `bundle/ffmpeg(.exe)` → system
//! PATH → custom path), builds invocation argv, runs them with
//! `-progress pipe:1 -nostats`, parses progress, and kills hung processes
//! via a no-output watchdog. Port of `src/util/ffmpeg/`.

pub mod command;
pub mod locate;
pub mod progress;
pub mod runner;

pub use command::FFmpegCommand;
pub use runner::{FFmpegOutcome, FFmpegRunner, WatchdogConfig};

/// Error messages surfaced by the runner, ported verbatim from the
/// Python translator's ERROR_MESSAGES entries (the zh/en translation
/// layer arrives with the UI tickets; the keys stay stable).
pub mod error_messages {
    pub const FFMPEG_FAILED: &str = "An error occurred while running FFmpeg";
    pub const FFMPEG_FAILED_WITH_CODE: &str = "FFmpeg failed with exit code {code}";
    pub const FFMPEG_NO_RESPONSE: &str = "FFmpeg stopped responding and has been terminated. The output file may be locked by another program (such as antivirus software), or the destination drive may be unavailable.";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_message_with_code_is_formatted() {
        let msg = error_messages::FFMPEG_FAILED_WITH_CODE.replace("{code}", "3");
        assert_eq!(msg, "FFmpeg failed with exit code 3");
    }
}
