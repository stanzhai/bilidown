//! Command-builder argv, pinned against the Python build's expectations
//! (`test/test_ffmpeg_command.py` plus the four classmethod shapes).

use std::path::Path;

use ffmpeg::command::{FFmpegCommand, SubtitleEntry};

fn s(file: &str, title: Option<&str>, language: Option<&str>, default: bool) -> SubtitleEntry {
    SubtitleEntry {
        file: file.to_string(),
        title: title.map(String::from),
        language: language.map(String::from),
        default,
    }
}

#[test]
fn remux_audio_argv_is_frozen() {
    let argv = FFmpegCommand::remux_audio("audio_1.m4a".as_ref(), "remux_1.m4a".as_ref()).build();
    assert_eq!(
        argv,
        vec![
            "ffmpeg",
            "-y",
            "-i",
            "audio_1.m4a",
            "-c",
            "copy",
            "-movflags",
            "+faststart",
            "-strict",
            "unofficial",
            "remux_1.m4a",
        ]
    );
}

#[test]
fn remux_audio_params_do_not_depend_on_ext() {
    let baseline = FFmpegCommand::remux_audio("in.m4a".as_ref(), "out.m4a".as_ref()).build();
    for ext in ["m4a", "flac", "ec3"] {
        let argv =
            FFmpegCommand::remux_audio(format!("in.{ext}").as_ref(), format!("out.{ext}").as_ref())
                .build();
        assert_eq!(argv[4..argv.len() - 1], baseline[4..baseline.len() - 1]);
        assert_eq!(argv[3], format!("in.{ext}"));
        assert_eq!(argv[argv.len() - 1], format!("out.{ext}"));
    }
}

#[test]
fn remux_audio_is_a_stream_copy() {
    let argv = FFmpegCommand::remux_audio("in.m4a".as_ref(), "out.m4a".as_ref()).build();
    assert!(argv.contains(&"copy".to_string()));
    assert!(!argv.contains(&"libmp3lame".to_string()));
}

#[test]
fn convert_m4a_to_mp3_uses_lame_v2() {
    let argv = FFmpegCommand::convert_m4a_to_mp3("in.m4a".as_ref(), "out.mp3".as_ref()).build();
    assert_eq!(
        argv,
        vec![
            "ffmpeg",
            "-y",
            "-i",
            "in.m4a",
            "-c:a",
            "libmp3lame",
            "-q:a",
            "2",
            "out.mp3",
        ]
    );
}

#[test]
fn merge_video_audio_plain_copy() {
    let argv = FFmpegCommand::merge_video_audio(
        "v.m4s".as_ref(),
        "a.m4s".as_ref(),
        "out.mp4".as_ref(),
        None,
        None,
        &[],
    )
    .build();
    assert_eq!(
        argv,
        vec![
            "ffmpeg",
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
fn merge_video_audio_with_cover_maps_all_three_inputs() {
    let argv = FFmpegCommand::merge_video_audio(
        "v.m4s".as_ref(),
        "a.m4s".as_ref(),
        "out.mp4".as_ref(),
        Some("cover.png".as_ref()),
        None,
        &[],
    )
    .build();
    let joined = argv.join(" ");
    // Cover is the third input (index 2) and rides along as attached_pic.
    assert!(joined.contains("-i cover.png"));
    assert!(joined.contains("-map 0:v:0 -map 1:a:0 -map 2:v:0"));
    assert!(joined.contains("-c:v:1 png"));
    assert!(joined.contains("-disposition:v:1 attached_pic"));
    assert!(joined.contains("-pix_fmt:v:1 rgba"));
    assert_eq!(argv[argv.len() - 1], "out.mp4");
}

#[test]
fn merge_video_audio_with_subtitles_maps_main_streams_and_dispositions() {
    let subs = [
        s("danmaku.ass", Some("弹幕"), Some("chi"), false),
        s("zh.ass", Some("简体中文"), Some("chi"), true),
    ];
    let argv = FFmpegCommand::merge_video_audio(
        "v.m4s".as_ref(),
        "a.m4s".as_ref(),
        "out.mkv".as_ref(),
        None,
        None,
        &subs,
    )
    .build();
    let joined = argv.join(" ");

    // Any -map disables default stream selection; main streams must be
    // mapped explicitly or the output would contain only subtitles.
    assert!(joined.contains("-map 0:v:0 -map 1:a:0"));
    // Subtitle inputs land after video/audio: indices 2 and 3.
    assert!(joined.contains("-map 2:s:0"));
    assert!(joined.contains("-map 3:s:0"));
    assert!(joined.contains("-metadata:s:s:0 title=弹幕"));
    assert!(joined.contains("-metadata:s:s:1 language=chi"));
    // A danmaku track first must not auto-display; explicit default only.
    assert!(joined.contains("-disposition:s:0 0"));
    assert!(joined.contains("-disposition:s:1 default"));
    assert!(joined.contains("-c:s copy"));
}

#[test]
fn merge_video_audio_with_cover_and_chapter_keeps_order() {
    let argv = FFmpegCommand::merge_video_audio(
        "v.m4s".as_ref(),
        "a.m4s".as_ref(),
        "out.mp4".as_ref(),
        Some("cover.png".as_ref()),
        Some("chapters.txt".as_ref()),
        &[],
    )
    .build();
    let joined = argv.join(" ");
    assert!(joined.contains("-map 2:v:0"));
    // ffmetadata is a stream-less input: it cannot shift the cover's
    // hard-coded input index.
    assert!(joined.contains("-f ffmetadata -i chapters.txt"));
    assert!(joined.contains("-map_chapters 3"));
}

#[test]
fn merge_video_parts_uses_concat_demuxer() {
    let argv =
        FFmpegCommand::merge_video_parts("list.txt".as_ref(), "out.mp4".as_ref(), None, None, &[])
            .build();
    assert_eq!(
        argv,
        vec![
            "ffmpeg",
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            "list.txt",
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
fn merge_video_parts_with_cover_maps_optional_audio() {
    // Part lists may lack an audio stream; `0:a?` tolerates both.
    let argv = FFmpegCommand::merge_video_parts(
        "list.txt".as_ref(),
        "out.mp4".as_ref(),
        Some("cover.png".as_ref()),
        None,
        &[],
    )
    .build();
    let joined = argv.join(" ");
    assert!(joined.contains("-map 0:v:0 -map 0:a? -map 1:v:0"));
}

#[test]
fn custom_paths_survive_odd_characters() {
    // Paths with spaces and brackets (multi-P part names) flow through
    // argv untouched — no shell, no quoting games.
    let argv = FFmpegCommand::merge_video_audio(
        Path::new("/tmp/dir with space/v [P1].m4s"),
        Path::new("/tmp/dir with space/a [P1].m4s"),
        Path::new("/tmp/out [P1].mp4"),
        None,
        None,
        &[],
    )
    .build();
    assert_eq!(argv[3], "/tmp/dir with space/v [P1].m4s");
    assert_eq!(argv[argv.len() - 1], "/tmp/out [P1].mp4");
}
