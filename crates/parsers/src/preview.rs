//! Preview state machine (ticket 12): the Rust port of
//! `src/util/parse/preview/previewer.py` + `info.py` + the per-stream query
//! halves of `video_info.py` / `audio_info.py` / `worker.py`.
//!
//! The GUI (ticket 21) owns the dialog; this module resolves everything the
//! dialog shows: choice data, the selected stream after priority + fallback,
//! file sizes, and the candidate walk.

use std::collections::HashMap;

use network::cdn::CdnConfig;
use network::HttpClient;
use serde_json::{json, Value};
use std::future::Future;

use crate::playurl::{
    available_codecs, audio_quality_label, build_available_streams, check_drm,
    extract_stream_urls, fetch_video_streams, merge_video_streams, parse_declared_quality_map,
    probe_file_size, query_mp4_info, resolve_codec, resolve_quality, sort_audio_ids_preview,
    video_quality_label, warn_codec_fallback, AvailableStreams, CodecFallbackWarning, FetchedMedia,
    MediaType, PlayurlApis, PlayurlError, AUDIO_QUALITY_AUTO, CODEC_AUTO, QUALITY_AUTO,
};
use crate::tree::Attribute;

// ---------------------------------------------------------------------------
// Choice data (video_info.py parse_quality_info / parse_codec_info,
// audio_info.py parse_info)
// ---------------------------------------------------------------------------

/// `{label: id}` — Python's insertion-ordered dict; `Vec` of pairs keeps order.
pub type ChoiceData = Vec<(String, i64)>;

/// Quality entries whose id has no label are skipped (Python would insert a
/// `None` key — not replicated).
pub fn video_quality_choice_data(quality_ids: &[i64]) -> ChoiceData {
    let mut out = vec![("auto".to_string(), QUALITY_AUTO)];
    for id in quality_ids {
        if let Some(label) = video_quality_label(*id) {
            out.push((label.to_string(), *id));
        }
    }
    out
}

pub fn audio_quality_choice_data(quality_ids: &[i64]) -> ChoiceData {
    let mut out = vec![("auto".to_string(), AUDIO_QUALITY_AUTO)];
    for id in quality_ids {
        if let Some(label) = audio_quality_label(*id) {
            out.push((label.to_string(), *id));
        }
    }
    out
}

pub fn video_codec_choice_data() -> ChoiceData {
    [("auto", CODEC_AUTO), ("AVC/H.264", 7), ("HEVC/H.265", 12), ("AV1", 13)]
        .into_iter()
        .map(|(l, id)| (l.to_string(), id))
        .collect()
}

// ---------------------------------------------------------------------------
// Preview state
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct PreviewInfo {
    pub episode_title: String,
    pub episode_number: String,
    pub from_fallback: bool,
    pub attribute: Attribute,
    pub parser_type: String,
    pub media_type: MediaType,
    pub info_data: Value,
    pub error_occurred: bool,
    pub error_message: String,
    /// Incremented on every clear; results carrying an older token are dropped.
    pub generation: u64,
    pub quality_ids: Vec<i64>,
    pub audio_quality_ids: Vec<i64>,
    pub video_quality_choices: ChoiceData,
    pub audio_quality_choices: ChoiceData,
    pub video_codec_choices: ChoiceData,
}

impl PreviewInfo {
    pub fn new() -> Self {
        Self { error_occurred: true, ..Default::default() }
    }
}

/// The query outcome the dialog shows for one selection.
#[derive(Clone, Debug)]
pub struct VideoQueryInfo {
    pub quality_id: i64,
    pub codec_id: i64,
    pub frame_rate: Value,
    pub bitrate: Value,
    pub file_size: u64,
    pub is_full_video: bool,
}

#[derive(Clone, Debug)]
pub struct AudioQueryInfo {
    pub quality_id: i64,
    pub file_size: u64,
    pub bitrate: Value,
    pub codec: String,
}

#[derive(Debug, Default)]
pub struct PreviewCache {
    video: HashMap<(i64, i64), VideoQueryInfo>,
    audio: HashMap<i64, AudioQueryInfo>,
}

/// Priority lists read from the config `Download` group by the caller.
#[derive(Clone, Debug)]
pub struct Priorities {
    pub video_quality: Vec<i64>,
    pub audio_quality: Vec<i64>,
    pub video_codec: Vec<i64>,
}

impl Default for Priorities {
    fn default() -> Self {
        // crates/config/src/defaults.json "Download" group.
        Self {
            video_quality: vec![127, 126, 125, 122, 120, 116, 112, 100, 80, 64, 32, 16],
            audio_quality: vec![30251, 30250, 30280, 30232, 30216],
            video_codec: vec![7, 12, 13],
        }
    }
}

pub struct Preview {
    pub info: PreviewInfo,
    pub priorities: Priorities,
    streams: AvailableStreams,
    declared: Option<Vec<(i64, Vec<i64>)>>,
    cache: PreviewCache,
    pub bvid: String,
    pub cid: i64,
}

/// `check_need_parse` (previewer.py): list-family attributes carry no media
/// info of their own.
pub fn check_need_parse(attribute: Attribute) -> bool {
    !(attribute.contains(Attribute::SPACE_BIT)
        || attribute.contains(Attribute::FAVLIST_BIT)
        || attribute.contains(Attribute::COLLECTION_LIST_BIT)
        || attribute.contains(Attribute::WATCH_LATER_BIT)
        || attribute.contains(Attribute::HISTORY_BIT))
}

impl Preview {
    pub fn new(priorities: Priorities) -> Self {
        Self {
            info: PreviewInfo::new(),
            priorities,
            streams: AvailableStreams::default(),
            declared: None,
            cache: PreviewCache::default(),
            bvid: String::new(),
            cid: 0,
        }
    }

    /// One candidate of the walk: fetch + post-process + choice data.
    /// Callers run this per candidate and check `info.error_occurred`.
    pub async fn start(
        &mut self,
        attribute: Attribute,
        bvid: &str,
        cid: i64,
        fetch: impl Future<Output = Result<FetchedMedia, PlayurlError>>,
    ) -> Result<(), PlayurlError> {
        self.clear_cache();
        self.info.attribute = attribute;
        self.bvid = bvid.to_string();
        self.cid = cid;

        if !check_need_parse(attribute) {
            return Ok(());
        }

        let fetched = fetch.await?;
        self.adopt(fetched)
    }

    /// `post_process` + `parse_info`: media type detection, DRM guard,
    /// available lists and choice data.
    pub fn adopt(&mut self, fetched: FetchedMedia) -> Result<(), PlayurlError> {
        let FetchedMedia { parser_type, media_type, info_data } = fetched;
        check_drm(&info_data)?;
        self.declared = if parser_type == "video" {
            parse_declared_quality_map(&info_data)
        } else {
            None
        };
        self.streams = build_available_streams(parser_type, media_type, &info_data);
        if media_type == MediaType::Dash {
            sort_audio_ids_preview(&mut self.streams.audio_ids);
        }

        self.info.parser_type = parser_type.to_string();
        self.info.media_type = media_type;
        self.info.info_data = info_data;
        self.info.quality_ids = self.streams.quality_ids.clone();
        self.info.audio_quality_ids = self.streams.audio_ids.clone();
        self.info.video_quality_choices = video_quality_choice_data(&self.streams.quality_ids);
        self.info.audio_quality_choices = audio_quality_choice_data(&self.streams.audio_ids);
        self.info.video_codec_choices = video_codec_choice_data();
        self.info.error_occurred = false;
        self.info.error_message = String::new();
        Ok(())
    }

    pub fn clear_cache(&mut self) {
        self.info.generation += 1;
        self.info.media_type = MediaType::Unknown;
        self.info.attribute = Attribute::default();
        self.info.episode_title.clear();
        self.info.from_fallback = false;
        self.info.error_occurred = true;
        self.info.error_message.clear();
        self.cache = PreviewCache::default();
    }

    /// `resolve_target` + `query_info` for video: returns the resolved
    /// selection's info, supplementing missing streams first.
    pub async fn query_video(
        &mut self,
        client: &HttpClient,
        apis: &PlayurlApis,
        requested_quality: i64,
        requested_codec: i64,
    ) -> Result<Option<VideoQueryInfo>, PlayurlError> {
        let Some(&quality) = resolve_quality(
            &self.streams.quality_ids,
            requested_quality,
            &self.priorities.video_quality,
        )
        .as_ref()
        .filter(|_| !self.streams.quality_ids.is_empty())
        else {
            return Ok(None);
        };
        let codecs = available_codecs(&self.streams, self.declared.as_deref(), quality);
        let Some(&codec) = resolve_codec(&codecs, requested_codec, &self.priorities.video_codec)
            .as_ref()
            .filter(|_| !codecs.is_empty())
        else {
            return Ok(None);
        };

        if let Some(cached) = self.cache.video.get(&(quality, codec)) {
            return Ok(Some(cached.clone()));
        }

        let mut node = self
            .streams
            .video_map
            .get(&quality)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|(c, _)| *c == codec)
                    .map(|(_, v)| v.clone())
            })
            .unwrap_or(Value::Null);

        if node.is_null() && self.info.parser_type == "video" {
            // Supplement (worker.py supplement_video_stream): fetch the qn,
            // prefer the requested codec, else first entry at that quality.
            let list = fetch_video_streams(client, apis, &self.bvid, self.cid, quality).await;
            let matched: Vec<&Value> =
                list.iter().filter(|e| e.get("id").and_then(Value::as_i64) == Some(quality)).collect();
            let Some(picked) = matched
                .iter()
                .find(|e| e.get("codecid").and_then(Value::as_i64) == Some(codec))
                .or(matched.first())
            else {
                return Err(PlayurlError::Api("无法获取该清晰度的视频流".into()));
            };
            merge_video_streams(&mut self.streams.video_map, &list);
            node = (*picked).clone();
        }

        if node.is_null() {
            return Ok(None);
        }

        let file_size = if let Some(size) = node.get("size") {
            size.as_u64().unwrap_or(0)
        } else if self.info.media_type == MediaType::Dash {
            probe_file_size(&extract_stream_urls(&node), &CdnConfig::default())
                .await
                .unwrap_or(0)
        } else {
            let (size, timelength) =
                query_mp4_info(client, &self.info.parser_type, &self.info.info_data, quality).await?;
            if let Some(obj) = node.as_object_mut() {
                obj.insert("timelength".into(), json!(timelength));
            }
            size
        };

        let info = VideoQueryInfo {
            quality_id: quality,
            codec_id: codec,
            frame_rate: node.get("frame_rate").cloned().unwrap_or(Value::Null),
            bitrate: node.get("bandwidth").cloned().unwrap_or(Value::Null),
            file_size,
            is_full_video: crate::playurl::check_is_full_video(
                self.info.media_type,
                &self.info.info_data,
                &node,
            ),
        };
        self.cache.video.insert((quality, codec), info.clone());
        Ok(Some(info))
    }

    /// `query_info` for audio: 30300 (auto) resolves by priority, like Python.
    pub fn query_audio(
        &mut self,
        requested_quality: i64,
    ) -> Option<AudioQueryInfo> {
        let id = if requested_quality == AUDIO_QUALITY_AUTO {
            self.priorities
                .audio_quality
                .iter()
                .copied()
                .find(|id| self.streams.audio_map.contains_key(id))
                .or(self.streams.audio_ids.first().copied())?
        } else {
            requested_quality
        };
        let node = self.streams.audio_map.get(&id).cloned()?;
        if let Some(cached) = self.cache.audio.get(&id) {
            return Some(cached.clone());
        }
        let file_size = node
            .get("size")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let info = AudioQueryInfo {
            quality_id: id,
            file_size,
            bitrate: node.get("bandwidth").cloned().unwrap_or(Value::Null),
            codec: node
                .get("codecs")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        self.cache.audio.entry(id).or_insert_with(|| info.clone());
        Some(info)
    }

    /// Codec-fallback observation hook (Python checks the final selection
    /// against the request and warns through the signal bus).
    pub fn note_codec_fallback(
        &self,
        requested: Option<i64>,
        actual: i64,
    ) -> Option<CodecFallbackWarning> {
        warn_codec_fallback(requested, actual)
    }

    pub fn audio_ids(&self) -> &[i64] {
        &self.streams.audio_ids
    }
}

