//! Download options dialog (ticket 21): the Rust port of
//! `src/gui/dialog/download_options/` — `dialog.py`, `media.py`,
//! `additional.py`, `download.py`, `preview.py` and the media-info half of
//! `card.py`.
//!
//! The dialog is a transaction: [`DownloadOptionsModel`] copies everything it
//! needs out of the [`ConfigStore`] on open, edits stay in the model, and
//! nothing touches the store (or the runtime naming state) until
//! [`DownloadOptionsModel::accept`] runs. Closing is a cancel.
//!
//! This module is the pure view-model — no gpui types — so the whole dialog
//! logic is testable headless (same split as `ui/parse/model.rs`); the render
//! half lives in `screen.rs`.

use std::collections::HashMap;

use config::ConfigStore;
use naming::conventions::{ConventionType, SampleShape, VariableListFactory};
use naming::formatter::{get_type_id_from_attribute, FileNameFormatter};
use naming::naming_rules::RuleStore;
use naming::task_naming::NamingState;
use parsers::playurl::MediaType;
use parsers::playurl::{audio_quality_label, codec_label, video_quality_label, CODEC_AUTO};
use parsers::preview::{AudioQueryInfo, ChoiceData, PreviewInfo, VideoQueryInfo};
use serde_json::Value;
use services::i18n::{self, Catalog};

use crate::ui::download_options::units::{format_bitrate, format_file_size, format_frame_rate};

/// Preview tag colors (`preview.py` tag_info) — one per downloadable content
/// kind, plus the gray "nothing selected" placeholder.
pub const TAG_COLORS: [(&str, &str); 8] = [
    ("video", "#0078D4"),
    ("audio", "#13A10E"),
    ("danmaku", "#8764B8"),
    ("subtitle", "#DA6A1E"),
    ("cover", "#00B7C3"),
    ("chapter", "#EF6950"),
    ("metadata", "#C239B3"),
    ("empty", "#8A8A8A"),
];

// ---------------------------------------------------------------------------
// Media page
// ---------------------------------------------------------------------------

/// `MediaInfoCard` + `MediaOptionsCard` state (`media.py`, `card.py`).
#[derive(Debug, Clone)]
pub struct MediaPage {
    /// Current selections (the combobox `currentData()`s).
    pub video_quality_id: i64,
    pub audio_quality_id: i64,
    pub video_codec_id: i64,
    /// Choice data straight from `PreviewInfo` (label keys, insertion order).
    pub video_quality_choices: ChoiceData,
    pub audio_quality_choices: ChoiceData,
    pub video_codec_choices: ChoiceData,
    /// Latest async query results; `None` = query still running/failed.
    pub video_info: Option<VideoQueryInfo>,
    pub audio_info: Option<AudioQueryInfo>,
    /// `MediaOptionsCard` switches (config-backed, copied on open).
    pub download_video_stream: bool,
    pub download_audio_stream: bool,
    pub merge_video_audio: bool,
    pub keep_original_files: bool,
    /// 0 = both, 1 = video only, 2 = audio only.
    pub keep_original_files_type: i64,
}

impl MediaPage {
    /// `MediaOptionsCard.on_change_download_stream_options` /
    /// `on_change_merge_option`: merge is only meaningful with both streams
    /// on (and is force-checked then), keep-originals needs merge on.
    pub fn set_stream_switches(&mut self, video: bool, audio: bool) {
        self.download_video_stream = video;
        self.download_audio_stream = audio;

        let both = video && audio;
        if !both {
            self.merge_video_audio = false;
        } else if !self.merge_video_audio {
            self.merge_video_audio = true;
        }
        self.sync_keep_original();
    }

    pub fn set_merge(&mut self, merge: bool) {
        self.merge_video_audio = merge;
        self.sync_keep_original();
    }

    fn sync_keep_original(&mut self) {
        let enabled =
            self.download_video_stream && self.download_audio_stream && self.merge_video_audio;
        if !enabled {
            self.keep_original_files = false;
        }
    }

    /// `has_media_to_download`.
    pub fn has_media_to_download(&self) -> bool {
        self.download_video_stream || self.download_audio_stream
    }

    /// `get_download_preview` (media half).
    pub fn preview_tags(&self) -> [(&'static str, bool); 2] {
        [
            ("video", self.download_video_stream),
            ("audio", self.download_audio_stream),
        ]
    }

    /// `update_source_description` (`update_source_description` in card.py):
    /// `#number - title`, with the fallback suffix when the media info came
    /// from a substitute video.
    pub fn source_description(&self, info: &PreviewInfo, c: &Catalog) -> String {
        let title = if info.episode_title.is_empty() {
            c.tr("MediaInfoCard", "Unknown").to_string()
        } else {
            info.episode_title.clone()
        };
        let content = if info.from_fallback {
            format!(
                "{title} · {}",
                c.tr("MediaInfoCard", "media info of the video the link points to is unavailable, this one is used instead")
            )
        } else {
            title
        };
        format!("#{} - {}", info.episode_number, content)
    }

    /// `update_video_quality_description`. `media_type` drives the MP4/FLV
    /// container suffix (the dialog keeps a copy; the parse screen owns the
    /// real one).
    pub fn video_quality_description(&self, media_type: MediaType, c: &Catalog) -> String {
        let Some(info) = &self.video_info else {
            return c
                .tr(
                    "MediaInfoCard",
                    "Video quality will be automatically selected based on priority settings",
                )
                .to_string();
        };
        let key = video_quality_label(info.quality_id).unwrap_or("");
        let mut parts = vec![
            i18n::video_quality(c, key).unwrap_or_else(|| "Unknown Video Quality".into()),
            format_frame_rate(value_as_f64(&info.frame_rate)),
            format_bitrate(value_as_i64(&info.bitrate)),
            format_file_size(info.file_size as i64),
        ];
        match media_type {
            MediaType::Mp4 => parts.push("MP4".into()),
            MediaType::Flv => parts.push("FLV".into()),
            _ => {}
        }
        if !info.is_full_video {
            parts.push(c.tr("MediaInfoCard", "preview").to_string());
        }
        parts.retain(|p| !p.is_empty());
        parts.join(", ")
    }

    /// `update_audio_quality_description`; empty info falls back to the
    /// per-media-type reason (`on_query_audio_info`).
    pub fn audio_quality_description(&self, media_type: MediaType, c: &Catalog) -> String {
        let Some(info) = &self.audio_info else {
            return match media_type {
                MediaType::Dash => c
                    .tr("MediaInfoCard", "No audio track (silent video stream)")
                    .to_string(),
                MediaType::Mp4 | MediaType::Flv => c
                    .tr(
                        "MediaInfoCard",
                        "Audio track is already embedded in the video stream",
                    )
                    .to_string(),
                MediaType::Unknown | MediaType::M4a => c
                    .tr(
                        "MediaInfoCard",
                        "Audio quality will be automatically selected based on priority settings",
                    )
                    .to_string(),
            };
        };
        let key = audio_quality_label(info.quality_id).unwrap_or("");
        let codec = audio_codec_name(&info.codec);
        let parts = [
            i18n::audio_quality(c, key).unwrap_or_else(|| "Unknown Audio Quality".into()),
            codec,
            format_bitrate(value_as_i64(&info.bitrate)),
            format_file_size(info.file_size as i64),
        ];
        parts
            .iter()
            .filter(|p| !p.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `get_codec_fallback_message` (card.py:200-220): the requested codec is
    /// missing from the source and auto (20) isn't a fallback.
    pub fn codec_fallback_message(&self, c: &Catalog) -> String {
        let Some(info) = &self.video_info else {
            return String::new();
        };
        let actual = info.codec_id;
        if self.video_codec_id == CODEC_AUTO || self.video_codec_id == actual {
            return String::new();
        }
        let requested_key = codec_label(self.video_codec_id).unwrap_or("");
        let requested =
            i18n::video_codec(c, requested_key).unwrap_or_else(|| requested_key.to_string());
        i18n::tip_messages(c, "VIDEO_CODEC_FALLBACK")
            .unwrap_or_else(|| "This video has no {requested}".into())
            .replace("{requested}", &requested)
    }

    /// `update_video_codec_description`: on fallback the line is the actual
    /// codec only; otherwise codec + compatibility tip.
    pub fn video_codec_description(&self, c: &Catalog) -> String {
        let Some(info) = &self.video_info else {
            return c
                .tr(
                    "MediaInfoCard",
                    "Video codec will be automatically selected based on priority settings",
                )
                .to_string();
        };
        let name = codec_label(info.codec_id)
            .and_then(|k| i18n::video_codec(c, k))
            .unwrap_or_else(|| "Unknown Video Codec".into());
        if !self.codec_fallback_message(c).is_empty() {
            return name;
        }
        let tip = match info.codec_id {
            7 => "Largest file size, best compatibility",
            12 => "Smaller file size, limited compatibility",
            13 => "Smallest file size, poorest compatibility",
            _ => "",
        };
        if tip.is_empty() {
            name
        } else {
            format!("{name}, {}", c.tr("MediaInfoCard", tip))
        }
    }
}

/// `reversed_audio_codec_map` (`util/common/data.py`): AAC LC → AAC LC.
fn audio_codec_name(codec: &str) -> String {
    match codec {
        "mp4a.40.2" => "AAC LC".to_string(),
        "fLaC" => "FLAC".to_string(),
        other => other.to_string(),
    }
}

fn value_as_f64(v: &Value) -> f64 {
    v.as_f64()
        .or_else(|| v.as_i64().map(|i| i as f64))
        .unwrap_or(0.0)
}

fn value_as_i64(v: &Value) -> i64 {
    v.as_i64()
        .or_else(|| v.as_f64().map(|f| f as i64))
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Additional page
// ---------------------------------------------------------------------------

/// `DanmakuSettingCard` + `SubtitleSettingCard` + `CoverSettingCard` +
/// `ChapterSettingCard` + `MetadataSettingCard` (`additional.py`).
#[derive(Debug, Clone)]
pub struct AdditionalPage {
    pub download_danmaku: bool,
    pub danmaku_type: String,
    pub embed_danmaku: bool,
    pub delete_danmaku_after_embed: bool,

    pub download_subtitle: bool,
    pub subtitle_type: String,
    pub embed_subtitle: bool,
    pub delete_subtitle_after_embed: bool,

    pub download_cover: bool,
    pub cover_type: String,
    pub attach_cover: bool,
    pub delete_cover_after_attach: bool,

    pub embed_chapter: bool,

    pub download_metadata: bool,
    pub metadata_type: String,

    /// Output container — lives on the download page but gates embed options
    /// here (`config.video_container.valueChanged` cross-card linkage).
    pub video_container: String,
}

impl AdditionalPage {
    fn embed_gating(&self, download: bool, type_: &str) -> bool {
        download && type_ == "ass" && self.video_container == "mkv"
    }

    pub fn danmaku_embed_enabled(&self) -> bool {
        self.embed_gating(self.download_danmaku, &self.danmaku_type)
    }

    pub fn subtitle_embed_enabled(&self) -> bool {
        self.embed_gating(self.download_subtitle, &self.subtitle_type)
    }

    pub fn danmaku_delete_after_embed_enabled(&self) -> bool {
        self.danmaku_embed_enabled() && self.embed_danmaku
    }

    pub fn subtitle_delete_after_embed_enabled(&self) -> bool {
        self.subtitle_embed_enabled() && self.embed_subtitle
    }

    /// `CoverSettingCard.on_change_cover_format`: avif can't be attached —
    /// switching to it force-unchecks attach (and the delete gate follows).
    pub fn set_cover_type(&mut self, type_: String) {
        if type_ == "avif" {
            self.attach_cover = false;
        }
        self.cover_type = type_;
    }

    /// `update_cover_option_states`.
    pub fn cover_attach_enabled(&self) -> bool {
        self.download_cover && self.cover_type != "avif"
    }

    pub fn cover_delete_after_attach_enabled(&self) -> bool {
        self.cover_attach_enabled() && self.attach_cover
    }

    /// `has_file_to_download` (additional.py:72-78) — chapter not included.
    pub fn has_file_to_download(&self) -> bool {
        self.download_danmaku
            || self.download_subtitle
            || self.download_cover
            || self.download_metadata
    }

    /// `get_download_preview` (additional half).
    pub fn preview_tags(&self) -> [(&'static str, bool); 5] {
        [
            ("danmaku", self.download_danmaku),
            ("subtitle", self.download_subtitle),
            ("cover", self.download_cover),
            ("chapter", self.embed_chapter),
            ("metadata", self.download_metadata),
        ]
    }
}

// ---------------------------------------------------------------------------
// Download page
// ---------------------------------------------------------------------------

/// One naming-rule combobox (`NamingConventionCard` /
/// `MultiTypeNamingConventionCard`): the rule list for one content type and
/// the selected rule id. `None` selection = "Not available" for that type.
#[derive(Debug, Clone)]
pub struct RuleChoice {
    pub type_id: i64,
    /// `(display_name, rule_id)` — defaults pre-selected.
    pub entries: Vec<(String, String)>,
    pub selected: Option<String>,
}

impl RuleChoice {
    pub fn from_store(store: &RuleStore, type_id: i64) -> Self {
        let list = store.get_rule_list_from_type(Some(type_id));
        let mut entries = Vec::new();
        let mut selected = None;
        for entry in &list {
            entries.push((naming::naming_rules::display_name(entry), entry.id.clone()));
            if entry.default {
                selected = Some(entry.id.clone());
            }
        }
        Self {
            type_id,
            entries,
            selected,
        }
    }
}

/// `DownloadSettingsPage` (`download.py`).
#[derive(Debug, Clone)]
pub struct DownloadPage {
    pub download_path: String,
    pub video_container: String,
    pub m4a_to_mp3: bool,
    pub show_dialog: bool,
    /// `File Naming.numbering_type` (0 from-specified, 1 parse list, 2 global).
    pub numbering_type: i64,
    pub global_starting_number: i64,
    /// Per-type rule selections; one entry per checked content type.
    pub rule_choices: Vec<RuleChoice>,
}

impl DownloadPage {
    /// `rule_ids` property: only enabled selects contribute.
    pub fn rule_ids(&self) -> HashMap<i64, String> {
        self.rule_choices
            .iter()
            .filter_map(|c| c.selected.clone().map(|id| (c.type_id, id)))
            .collect()
    }

    /// The rule that names the preview, with per-type fallback
    /// (`store.get_rule_from_config`).
    pub fn preview_rule(&self, store: &RuleStore, type_id: i64) -> Option<String> {
        let chosen = self
            .rule_choices
            .iter()
            .find(|c| c.type_id == type_id)
            .and_then(|c| c.selected.clone())
            .filter(|id| !id.is_empty());
        chosen
            .and_then(|id| store.get_rule_by_id(&id))
            .or_else(|| store.get_rule_from_config(Some(type_id)))
    }
}

// ---------------------------------------------------------------------------
// Whole-dialog model
// ---------------------------------------------------------------------------

/// The three abort cases of `DownloadOptionsDialog.accept()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptCheck {
    /// `on_check` wants the user to confirm downloading a silent video.
    ConfirmVideoOnly,
    /// `on_check` wants the user to confirm merge being disabled.
    ConfirmNoMerge,
    /// Neither media nor additional files are selected.
    NoFilesSelected,
    /// Everything checks out — the caller may commit.
    Ok,
}

/// `DownloadOptionsDialog` state (dialog.py), minus the Qt plumbing.
#[derive(Debug, Clone)]
pub struct DownloadOptionsModel {
    pub media: MediaPage,
    pub additional: AdditionalPage,
    pub download: DownloadPage,
    /// Media type + attribute snapshot from the parse screen's `PreviewInfo`.
    pub media_type: MediaType,
    pub attribute: i64,
    /// Priority arrays (`Download` group) — edited in-dialog by the minimal
    /// reorder editors, committed with everything else.
    pub video_quality_priority: Vec<i64>,
    pub audio_quality_priority: Vec<i64>,
    pub video_codec_priority: Vec<i64>,
}

fn cfg_bool(cfg: &ConfigStore, group: &str, key: &str) -> bool {
    cfg.get(group, key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn cfg_str(cfg: &ConfigStore, group: &str, key: &str) -> String {
    cfg.get(group, key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn cfg_i64(cfg: &ConfigStore, group: &str, key: &str) -> i64 {
    cfg.get(group, key).and_then(Value::as_i64).unwrap_or(0)
}

fn cfg_ids(cfg: &ConfigStore, key: &str) -> Vec<i64> {
    cfg.get("Download", key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}

impl DownloadOptionsModel {
    /// Copy everything the dialog edits out of the config + preview state.
    /// `type_ids` are the checked items' content types
    /// (`parse.py get_checked_type_ids`).
    pub fn open(
        cfg: &ConfigStore,
        info: &PreviewInfo,
        type_ids: &[i64],
        store: &RuleStore,
        runtime_quality: (i64, i64, i64),
    ) -> Self {
        let container = cfg_str(cfg, "Download", "video_container");
        let mut rule_choices: Vec<RuleChoice> = type_ids
            .iter()
            .map(|&id| RuleChoice::from_store(store, id))
            .collect();
        rule_choices.sort_by_key(|c| c.type_id);

        Self {
            media: MediaPage {
                video_quality_id: runtime_quality.0,
                audio_quality_id: runtime_quality.1,
                video_codec_id: runtime_quality.2,
                video_quality_choices: info.video_quality_choices.clone(),
                audio_quality_choices: info.audio_quality_choices.clone(),
                video_codec_choices: info.video_codec_choices.clone(),
                video_info: None,
                audio_info: None,
                download_video_stream: cfg_bool(cfg, "Download", "download_video_stream"),
                download_audio_stream: cfg_bool(cfg, "Download", "download_audio_stream"),
                merge_video_audio: cfg_bool(cfg, "Download", "merge_video_audio"),
                keep_original_files: cfg_bool(cfg, "Download", "keep_original_files"),
                keep_original_files_type: cfg_i64(cfg, "Download", "keep_original_files_type"),
            },
            additional: AdditionalPage {
                download_danmaku: cfg_bool(cfg, "Additional", "download_danmaku"),
                danmaku_type: cfg_str(cfg, "Additional", "danmaku_type"),
                embed_danmaku: cfg_bool(cfg, "Additional", "embed_danmaku"),
                delete_danmaku_after_embed: cfg_bool(
                    cfg,
                    "Additional",
                    "delete_danmaku_after_embed",
                ),
                download_subtitle: cfg_bool(cfg, "Additional", "download_subtitle"),
                subtitle_type: cfg_str(cfg, "Additional", "subtitle_type"),
                embed_subtitle: cfg_bool(cfg, "Additional", "embed_subtitle"),
                delete_subtitle_after_embed: cfg_bool(
                    cfg,
                    "Additional",
                    "delete_subtitle_after_embed",
                ),
                download_cover: cfg_bool(cfg, "Additional", "download_cover"),
                cover_type: cfg_str(cfg, "Additional", "cover_type"),
                attach_cover: cfg_bool(cfg, "Additional", "attach_cover"),
                delete_cover_after_attach: cfg_bool(cfg, "Additional", "delete_cover_after_attach"),
                embed_chapter: cfg_bool(cfg, "Additional", "embed_chapter"),
                download_metadata: cfg_bool(cfg, "Additional", "download_metadata"),
                metadata_type: cfg_str(cfg, "Additional", "metadata_type"),
                video_container: container.clone(),
            },
            download: DownloadPage {
                download_path: cfg_str(cfg, "Download", "download_path"),
                video_container: container,
                m4a_to_mp3: cfg_bool(cfg, "Download", "m4a_to_mp3"),
                show_dialog: cfg_bool(cfg, "Behavior", "show_download_options_dialog"),
                numbering_type: cfg_i64(cfg, "File Naming", "numbering_type"),
                global_starting_number: 1,
                rule_choices,
            },
            media_type: info.media_type,
            attribute: info.attribute.0 as i64,
            video_quality_priority: cfg_ids(cfg, "video_quality_priority"),
            audio_quality_priority: cfg_ids(cfg, "audio_quality_priority"),
            video_codec_priority: cfg_ids(cfg, "video_codec_priority"),
        }
    }

    /// Container select handler — the download page owns it, the additional
    /// page's embed gating reacts (`config.video_container.valueChanged`).
    pub fn set_video_container(&mut self, container: String) {
        self.download.video_container = container.clone();
        self.additional.video_container = container;
    }

    /// `accept()`'s gating (`on_check` + the no-files info box). Pure: the
    /// caller decides how to ask the user.
    pub fn accept_check(&self) -> AcceptCheck {
        let v = self.media.download_video_stream;
        let a = self.media.download_audio_stream;
        if !a && v {
            return AcceptCheck::ConfirmVideoOnly;
        }
        if !self.media.merge_video_audio && v && a {
            return AcceptCheck::ConfirmNoMerge;
        }
        if !self.media.has_media_to_download() && !self.additional.has_file_to_download() {
            return AcceptCheck::NoFilesSelected;
        }
        AcceptCheck::Ok
    }

    /// `MediaSettingsPage.on_save` + `DownloadSettingsPage.on_save` +
    /// `MediaOptionsCard.save`: commit the transaction. `naming` gets the
    /// per-type rule ids; the caller persists the config afterwards.
    pub fn commit(&self, cfg: &mut ConfigStore, naming: &mut NamingState) {
        cfg.set(
            "Download",
            "download_video_stream",
            Value::Bool(self.media.download_video_stream),
        );
        cfg.set(
            "Download",
            "download_audio_stream",
            Value::Bool(self.media.download_audio_stream),
        );
        cfg.set(
            "Download",
            "merge_video_audio",
            Value::Bool(self.media.merge_video_audio),
        );
        cfg.set(
            "Download",
            "keep_original_files",
            Value::Bool(self.media.keep_original_files),
        );
        cfg.set(
            "Download",
            "keep_original_files_type",
            Value::from(self.media.keep_original_files_type),
        );
        cfg.set(
            "Download",
            "video_container",
            Value::String(self.download.video_container.clone()),
        );
        cfg.set(
            "Download",
            "m4a_to_mp3",
            Value::Bool(self.download.m4a_to_mp3),
        );
        cfg.set(
            "Download",
            "download_path",
            Value::String(self.download.download_path.clone()),
        );
        cfg.set(
            "Download",
            "video_quality_priority",
            Value::Array(
                self.video_quality_priority
                    .iter()
                    .map(|i| Value::from(*i))
                    .collect(),
            ),
        );
        cfg.set(
            "Download",
            "audio_quality_priority",
            Value::Array(
                self.audio_quality_priority
                    .iter()
                    .map(|i| Value::from(*i))
                    .collect(),
            ),
        );
        cfg.set(
            "Download",
            "video_codec_priority",
            Value::Array(
                self.video_codec_priority
                    .iter()
                    .map(|i| Value::from(*i))
                    .collect(),
            ),
        );
        cfg.set(
            "Behavior",
            "show_download_options_dialog",
            Value::Bool(self.download.show_dialog),
        );
        cfg.set(
            "File Naming",
            "numbering_type",
            Value::from(self.download.numbering_type),
        );

        let a = &self.additional;
        cfg.set(
            "Additional",
            "download_danmaku",
            Value::Bool(a.download_danmaku),
        );
        cfg.set(
            "Additional",
            "danmaku_type",
            Value::String(a.danmaku_type.clone()),
        );
        cfg.set("Additional", "embed_danmaku", Value::Bool(a.embed_danmaku));
        cfg.set(
            "Additional",
            "delete_danmaku_after_embed",
            Value::Bool(a.delete_danmaku_after_embed),
        );
        cfg.set(
            "Additional",
            "download_subtitle",
            Value::Bool(a.download_subtitle),
        );
        cfg.set(
            "Additional",
            "subtitle_type",
            Value::String(a.subtitle_type.clone()),
        );
        cfg.set(
            "Additional",
            "embed_subtitle",
            Value::Bool(a.embed_subtitle),
        );
        cfg.set(
            "Additional",
            "delete_subtitle_after_embed",
            Value::Bool(a.delete_subtitle_after_embed),
        );
        cfg.set(
            "Additional",
            "download_cover",
            Value::Bool(a.download_cover),
        );
        cfg.set(
            "Additional",
            "cover_type",
            Value::String(a.cover_type.clone()),
        );
        cfg.set("Additional", "attach_cover", Value::Bool(a.attach_cover));
        cfg.set(
            "Additional",
            "delete_cover_after_attach",
            Value::Bool(a.delete_cover_after_attach),
        );
        cfg.set("Additional", "embed_chapter", Value::Bool(a.embed_chapter));
        cfg.set(
            "Additional",
            "download_metadata",
            Value::Bool(a.download_metadata),
        );
        cfg.set(
            "Additional",
            "metadata_type",
            Value::String(a.metadata_type.clone()),
        );

        // runtime.download.* — per-task selections, not persisted config.
        // runtime.naming.target_rule_ids — set below; global starting number
        // stays where the runtime had it unless the dialog changed it.
        naming.target_rule_ids = self.download.rule_ids();
        if self.download.numbering_type == naming::numbering::CONTINUOUS {
            naming.global_starting_number = self.download.global_starting_number;
        }
    }

    /// `on_update_preview` + `DownloadPreviewBar.update_preview`: the ordered
    /// set of visible tags, or `("empty", true)` when nothing is selected.
    pub fn preview_tags(&self) -> Vec<(&'static str, bool)> {
        let mut tags: Vec<(&'static str, bool)> = self
            .media
            .preview_tags()
            .into_iter()
            .chain(self.additional.preview_tags())
            .collect();
        let any = tags.iter().any(|(_, on)| *on);
        if !any {
            tags = vec![("empty", true)];
        }
        tags
    }

    /// Filename preview through the naming engine so it equals the final
    /// output (`FileNameFormatter.format` on the selected rule with the
    /// convention sample data).
    pub fn filename_preview(&self, store: &RuleStore, type_id: i64) -> Option<String> {
        // The attribute snapshot is the Python card's type_id fallback when
        // no rule choice covers the row.
        let type_id = if type_id == 0 {
            get_type_id_from_attribute(self.attribute).unwrap_or(type_id)
        } else {
            type_id
        };
        let rule = self.download.preview_rule(store, type_id)?;
        let convention = convention_from_id(type_id)?;
        let variable_data =
            VariableListFactory::new().build_variable_data(convention, first_shape(convention));
        let formatter = FileNameFormatter::new(variable_data, store);
        formatter.format(Some(type_id), 0, &rule)
    }

    /// `get_checked_type_ids` (`parse.py:757-776`): content types of the
    /// checked tree items.
    pub fn checked_type_ids(attributes: &[i64]) -> Vec<i64> {
        let mut ids: Vec<i64> = attributes
            .iter()
            .filter_map(|a| get_type_id_from_attribute(*a))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }
}

/// type_id → `ConventionType` (the crate exports no reverse constructor).
fn convention_from_id(id: i64) -> Option<ConventionType> {
    use ConventionType::*;
    Some(match id {
        11 => Normal,
        12 => Part,
        13 => Collection,
        14 => InteractiveVideo,
        20 => Bangumi,
        30 => Cheese,
        40 => Favorite,
        50 => Space,
        60 => History,
        70 => WatchLater,
        80 => Weekly,
        90 => Audio,
        _ => return None,
    })
}

/// Preview sample shape for one content type: the first shape its entries
/// can actually take (`supported_shapes`).
fn first_shape(type_: ConventionType) -> SampleShape {
    naming::conventions::supported_shapes(type_)[0]
}
