//! FFmpeg argv builder. Port of `src/util/ffmpeg/command.py`, arguments
//! byte-for-byte identical to the Python build.

use std::path::Path;

/// One subtitle/ASS track to embed as a soft subtitle stream (MKV only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleEntry {
    pub file: String,
    pub title: Option<String>,
    pub language: Option<String>,
    pub default: bool,
}

#[derive(Default)]
pub struct FFmpegCommand {
    /// (前置参数如 -f concat, 输入路径) pairs, kept in input order.
    inputs: Vec<(Vec<String>, String)>,
    outputs: Vec<String>,
    params: Vec<String>,
}

impl FFmpegCommand {
    pub fn new() -> Self {
        Self::default()
    }

    /// Options are prepended to this input (e.g. `-f concat`); routed
    /// through here so input indices stay reliable.
    pub fn add_input(&mut self, input_path: impl Into<String>, options: &[&str]) -> &mut Self {
        self.inputs
            .push((options.iter().map(|s| s.to_string()).collect(), input_path.into()));
        self
    }

    pub fn add_output(&mut self, output_path: impl Into<String>) -> &mut Self {
        self.outputs.push(output_path.into());
        self
    }

    pub fn add_param(&mut self, args: &[&str]) -> &mut Self {
        self.params.extend(args.iter().map(|s| s.to_string()));
        self
    }

    /// Dolby Vision config records live in the non-standard dvcC/dvvC MP4
    /// boxes; without `-strict unofficial` FFmpeg skips them and the
    /// remuxed file stops being recognized as DV. MKV is unaffected, and
    /// the flag is harmless there, so every mux path carries it.
    pub fn allow_unofficial(&mut self) -> &mut Self {
        self.add_param(&["-strict", "unofficial"])
    }

    /// Attach the cover as an `attached_pic` video stream; `maps` are the
    /// `-map` selectors for every stream that must survive.
    pub fn add_cover(&mut self, cover_path: impl Into<String>, maps: &[&str]) -> &mut Self {
        self.add_input(cover_path, &[]);
        for map_param in maps {
            self.add_param(&["-map", map_param]);
        }
        self.add_param(&["-c:v:1", "png"])
            .add_param(&["-disposition:v:1", "attached_pic"])
            .add_param(&["-pix_fmt:v:1", "rgba"])
    }

    /// Embed ASS danmaku/subtitles as independent subtitle tracks (MKV
    /// only). Callers must already `-map` the main streams: any `-map`
    /// disables FFmpeg's default stream selection.
    pub fn add_subtitles(&mut self, subtitle_list: &[SubtitleEntry]) -> &mut Self {
        for (index, entry) in subtitle_list.iter().enumerate() {
            let input_index = self.inputs.len();
            self.add_input(entry.file.as_str(), &[]);
            self.add_param(&["-map", &format!("{input_index}:s:0")]);

            if let Some(title) = &entry.title {
                self.add_param(&[&format!("-metadata:s:s:{index}"), &format!("title={title}")]);
            }
            if let Some(language) = &entry.language {
                self.add_param(&[
                    &format!("-metadata:s:s:{index}"),
                    &format!("language={language}"),
                ]);
            }
            // Every track needs an explicit disposition: otherwise FFmpeg
            // marks the first subtitle track default, and a danmaku track
            // landing first would display on open.
            let disposition = if entry.default { "default" } else { "0" };
            self.add_param(&[&format!("-disposition:s:{index}"), disposition]);
        }
        self.add_param(&["-c:s", "copy"])
    }

    /// Add an ffmetadata chapter file as an extra input and map chapters
    /// from it. MKV gets native chapters; MP4 gets both Nero chpl and a
    /// QuickTime chapter track — one command serves both. Only
    /// `-map_chapters`, never `-map_metadata`, which would overwrite the
    /// global metadata with the chapter file's.
    pub fn add_chapter(&mut self, chapter_path: impl Into<String>) -> &mut Self {
        let index = self.inputs.len();
        // ffmetadata input carries no streams, so it cannot shift the
        // stream mapping used by cover `-map`s.
        self.add_input(chapter_path, &["-f", "ffmetadata"]);
        self.add_param(&["-map_chapters", &index.to_string()])
    }

    /// Final argv (program name first), exactly what the Python
    /// `build()` produced.
    pub fn build(&self) -> Vec<String> {
        let mut command = vec!["ffmpeg".to_string(), "-y".to_string()];
        for (options, input_path) in &self.inputs {
            command.extend(options.iter().cloned());
            command.push("-i".to_string());
            command.push(input_path.clone());
        }
        command.extend(self.params.iter().cloned());
        command.extend(self.outputs.iter().cloned());
        command
    }

    /// DASH video+audio merge.
    #[allow(clippy::too_many_arguments)]
    pub fn merge_video_audio(
        video_path: &Path,
        audio_path: &Path,
        output_path: &Path,
        cover_path: Option<&Path>,
        chapter_path: Option<&Path>,
        subtitle_list: &[SubtitleEntry],
    ) -> FFmpegCommand {
        let mut c = FFmpegCommand::new();
        c.add_input(video_path.to_string_lossy(), &[])
            .add_input(audio_path.to_string_lossy(), &[])
            .add_param(&["-c:v", "copy"])
            .add_param(&["-c:a", "copy"])
            .allow_unofficial();

        match (cover_path, !subtitle_list.is_empty()) {
            // Cover is the third input (index 2).
            (Some(cover), _) => {
                c.add_cover(
                    cover.to_string_lossy(),
                    &["0:v:0", "1:a:0", "2:v:0"],
                );
            }
            // Without a cover the main streams ride on FFmpeg's default
            // selection — until a subtitle `-map` disables it.
            (None, true) => {
                c.add_param(&["-map", "0:v:0"]).add_param(&["-map", "1:a:0"]);
            }
            _ => {}
        }

        // Subtitle inputs must come after the cover: add_cover hard-codes
        // the cover's input index.
        if !subtitle_list.is_empty() {
            c.add_subtitles(subtitle_list);
        }
        if let Some(chapter) = chapter_path {
            c.add_chapter(chapter.to_string_lossy());
        }
        c.add_output(output_path.to_string_lossy());
        c
    }

    /// Legacy FLV multi-part concat merge.
    #[allow(clippy::too_many_arguments)]
    pub fn merge_video_parts(
        lists_path: &Path,
        output_path: &Path,
        cover_path: Option<&Path>,
        chapter_path: Option<&Path>,
        subtitle_list: &[SubtitleEntry],
    ) -> FFmpegCommand {
        let mut c = FFmpegCommand::new();
        c.add_input(lists_path.to_string_lossy(), &["-f", "concat", "-safe", "0"])
            .add_param(&["-c:v", "copy"])
            .add_param(&["-c:a", "copy"])
            .allow_unofficial();

        match (cover_path, !subtitle_list.is_empty()) {
            // Parts are input 0, the cover input 1. `0:a?` keeps the
            // audio when present without failing part-lists without one.
            (Some(cover), _) => {
                c.add_cover(cover.to_string_lossy(), &["0:v:0", "0:a?", "1:v:0"]);
            }
            (None, true) => {
                c.add_param(&["-map", "0:v:0"]).add_param(&["-map", "0:a?"]);
            }
            _ => {}
        }

        if !subtitle_list.is_empty() {
            c.add_subtitles(subtitle_list);
        }
        if let Some(chapter) = chapter_path {
            c.add_chapter(chapter.to_string_lossy());
        }
        c.add_output(output_path.to_string_lossy());
        c
    }

    /// m4a → mp3 conversion.
    pub fn convert_m4a_to_mp3(input_path: &Path, output_path: &Path) -> FFmpegCommand {
        let mut c = FFmpegCommand::new();
        c.add_input(input_path.to_string_lossy(), &[])
            .add_param(&["-c:a", "libmp3lame"])
            .add_param(&["-q:a", "2"])
            .add_output(output_path.to_string_lossy());
        c
    }

    /// Remux a standalone DASH audio stream into the standard container
    /// matching its extension.
    ///
    /// Bilibili DASH audio is an fMP4 **fragmented** container (ftyp +
    /// moov without a sample table + sidx + [moof + mdat] × N); strict
    /// parsers like foobar2000 reject it even with a `.m4a` extension.
    /// `-c copy` rewrites the container only; `+faststart` moves moov to
    /// the front and is silently ignored by non-mov containers (flac,
    /// ec3), so one command serves all three extensions.
    pub fn remux_audio(input_path: &Path, output_path: &Path) -> FFmpegCommand {
        let mut c = FFmpegCommand::new();
        c.add_input(input_path.to_string_lossy(), &[])
            .add_param(&["-c", "copy"])
            .add_param(&["-movflags", "+faststart"])
            .allow_unofficial()
            .add_output(output_path.to_string_lossy());
        c
    }
}
