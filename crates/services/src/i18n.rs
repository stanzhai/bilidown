//! i18n: zh_CN / zh_TW / en catalogs generated from the Python build's
//! `.ts` files, plus the `Translator` map API the Python code calls
//! (`Translator.VIDEO_QUALITY("1080P")` etc.).
//!
//! Lookup semantics match QTranslator + `QCoreApplication.translate`:
//! `(context, source)` → translation; en (sourcelanguage `en_US`) returns
//! the source itself. Unknown key → source string (`mapping.get(key)` in
//! Python returns `None`, but every caller treats the English source as the
//! fallback text, so we return the source — see `COLUMN_NAME`'s
//! `map.get(key, key)`).

use crate::i18n_data::CATALOG;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Display language. Serialized values are the strings stored in
/// `config.json` under `Interface.language` (`zh_CN`/`zh_TW`/`en_US`/`Auto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    ChineseSimplified,
    ChineseTraditional,
    English,
    /// Follow the OS locale (`QLocale()` default).
    Auto,
}

impl Language {
    /// Parse the serialized `Interface.language` config value.
    /// Anything else falls back to `Auto` (OptionsValidator behavior:
    /// unknown value → first option... but the first option is `zh_CN`
    /// while QConfig loads the raw string; `Auto` is the Python default).
    pub fn from_config(v: &str) -> Self {
        match v {
            "zh_CN" => Self::ChineseSimplified,
            "zh_TW" => Self::ChineseTraditional,
            "en_US" => Self::English,
            _ => Self::Auto,
        }
    }

    /// Resolve `Auto` against the system locale the way `QLocale()` does:
    /// the OS UI language, falling back to Simplified Chinese when it is
    /// neither Chinese variant nor English (the app's own default).
    pub fn resolve(self) -> ResolvedLanguage {
        match self {
            Self::ChineseSimplified => ResolvedLanguage::ZhCn,
            Self::ChineseTraditional => ResolvedLanguage::ZhTw,
            Self::English => ResolvedLanguage::En,
            Self::Auto => {
                let locale = sys_locale::get_locale().unwrap_or_default();
                // BCP-47 tag, e.g. `zh-Hans-CN`, `zh_TW`, `en_US`
                let lang = locale.split(['-', '_']).next().unwrap_or("");
                let region = locale.rsplit(['-', '_']).next().unwrap_or("");
                match (lang, region) {
                    ("zh", "TW" | "HK" | "MO" | "Hant") => ResolvedLanguage::ZhTw,
                    ("zh", _) => ResolvedLanguage::ZhCn,
                    ("en", _) => ResolvedLanguage::En,
                    _ => ResolvedLanguage::ZhCn,
                }
            }
        }
    }
}

/// A concrete language a catalog exists for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedLanguage {
    ZhCn,
    ZhTw,
    En,
}

/// One resolved catalog: `(context, source)` → translated text.
pub struct Catalog {
    entries: HashMap<(&'static str, &'static str), &'static str>,
}

impl Catalog {
    /// `QCoreApplication.translate(context, source)`: the translation for
    /// `(context, source)`; `None` when the catalog has no such entry (Qt
    /// then renders the source; callers use [`Catalog::tr`]).
    pub fn lookup(&self, context: &str, source: &str) -> Option<&'static str> {
        self.entries.get(&(context, source)).copied()
    }

    /// Translate with the Qt fallback: unknown entry → the source itself.
    pub fn tr<'a>(&self, context: &str, source: &'a str) -> &'a str {
        self.lookup(context, source).unwrap_or(source)
    }
}

/// The resolved catalog for `lang` (`en` = identity: sources are English).
pub fn catalog(lang: ResolvedLanguage) -> &'static Catalog {
    static ZH_CN: LazyLock<Catalog> = LazyLock::new(|| build(|zh, _| zh));
    static ZH_TW: LazyLock<Catalog> = LazyLock::new(|| build(|_, tw| tw));
    static EN: LazyLock<Catalog> = LazyLock::new(|| Catalog { entries: HashMap::new() });
    match lang {
        ResolvedLanguage::ZhCn => &ZH_CN,
        ResolvedLanguage::ZhTw => &ZH_TW,
        // sourcelanguage is en_US: every source renders as itself
        ResolvedLanguage::En => &EN,
    }
}


/// Number of live (non-vanished) catalog entries — completeness test uses
/// this to pin the `.ts` extraction (125 contexts × 973 messages).
pub fn catalog_len() -> usize {
    CATALOG.len()
}

/// Context names present in the catalog.
pub fn catalog_contexts() -> Vec<&'static str> {
    let mut ctxs: Vec<&str> = CATALOG.iter().map(|(c, _, _, _)| *c).collect();
    ctxs.sort_unstable();
    ctxs.dedup();
    ctxs
}
fn build(pick: fn(&'static str, &'static str) -> &'static str) -> Catalog {
    Catalog {
        entries: CATALOG
            .iter()
            .map(|(ctx, _source, zh, tw)| ((*ctx, *_source), pick(zh, tw)))
            .collect(),
    }
}

// ---------- Translator: the map-style helpers from util/common/translator.py ----------
//
// Keys are the Python map keys verbatim; values are the English source
// strings, translated at call time through the active catalog — exactly what
// `translate(context, source)` does in Python.

macro_rules! translator_map {
    ($(#[$meta:meta])* $fn_name:ident, $ctx:expr, { $($key:literal => $source:literal),* $(,)? }) => {
        $(#[$meta])*
        pub fn $fn_name(c: &Catalog, key: &str) -> Option<String> {
            match key {
                $($key => Some(c.tr($ctx, $source).to_owned()),)*
                _ => None,
            }
        }
    };
}

translator_map!(
    /// `Translator.CDN_SERVER_PROVIDER`
    cdn_server_provider, "CDN_SERVER_PROVIDER", {
    "ALIYUN" => "Aliyun",
    "TENCENT" => "Tencent Cloud",
    "HUAWEI" => "Huawei Cloud",
    "CUSTOM" => "Custom",
});

translator_map!(
    /// `Translator.VIDEO_QUALITY`
    video_quality, "VIDEO_QUALITY", {
    "auto" => "Auto",
    "8K" => "8K UHD",
    "DOLBY_VISION" => "Dolby Vision",
    "HDR" => "HDR True Color",
    "4K_SDR" => "4K SDR Enhanced",
    "4K" => "4K UHD",
    "1080P60" => "1080P 60fps",
    "1080P+" => "1080P High Bitrate",
    "AI" => "AI Upscale",
    "1080P" => "1080P",
    "720P" => "720P",
    "480P" => "480P",
    "360P" => "360P",
});

translator_map!(
    /// `Translator.AUDIO_QUALITY`
    audio_quality, "AUDIO_QUALITY", {
    "auto" => "Auto",
    "HI_RES" => "Hi-Res Audio",
    "DOLBY_ATMOS" => "Dolby Atmos",
    "192K" => "192 kbps",
    "132K" => "132 kbps",
    "64K" => "64 kbps",
});

translator_map!(
    /// `Translator.VIDEO_CODEC`
    video_codec, "VIDEO_CODEC", {
    "auto" => "Auto",
    "AVC/H.264" => "AVC/H.264",
    "HEVC/H.265" => "HEVC/H.265",
    "AV1" => "AV1",
});

translator_map!(
    /// `Translator.EPISODE_TYPE`
    episode_type, "EPISODE_TYPE", {
    "USER_UPLOADS" => "User Uploads",
    "INTERACTIVE_VIDEO" => "Interactive Video",
    "ANIME" => "Anime",
    "MOVIE" => "Movies",
    "DOCUMENTARY" => "Documentaries",
    "CHN_ANIME" => "Chinese Animation",
    "TV" => "TV Dramas",
    "VARIETY" => "Variety Shows",
    "COURSE" => "Courses",
    "MALL_COURSE" => "Mall Courses",
    "WEEKLY" => "Weekly Picks",
    "COLLECTION_LIST" => "Collection",
    "FAVORITES" => "Favorites",
    "PROFILE" => "Profile",
    "WATCH_LATER" => "Watch Later",
    "HISTORY" => "History",
    "AUDIO" => "Music",
});

translator_map!(
    /// `Translator.DANMAKU_SPEED`
    danmaku_speed, "DANMAKU_SPEED", {
    "VERY_SLOW" => "Very Slow",
    "SLOW" => "Slow",
    "NORMAL" => "Normal",
    "FAST" => "Fast",
    "VERY_FAST" => "Very Fast",
});

translator_map!(
    /// `Translator.DANMAKU_DENSITY`
    danmaku_density, "DANMAKU_DENSITY", {
    "NORMAL" => "Normal",
    "HIGH" => "High",
    "OVERLAP" => "Overlap",
});

translator_map!(
    /// `Translator.SUBTITLES_ALIGNMENT`
    subtitles_alignment, "SUBTITLES_ALIGNMENT", {
    "BOTTOM_LEFT" => "Bottom Left",
    "BOTTOM_CENTER" => "Bottom Center",
    "BOTTOM_RIGHT" => "Bottom Right",
    "MIDDLE_LEFT" => "Middle Left",
    "MIDDLE_CENTER" => "Middle Center",
    "MIDDLE_RIGHT" => "Middle Right",
    "TOP_LEFT" => "Top Left",
    "TOP_CENTER" => "Top Center",
    "TOP_RIGHT" => "Top Right",
});

translator_map!(
    /// `Translator.TIP_MESSAGES` (service-scope subset used by this crate:
    /// download-completion notifications)
    tip_messages, "TIP_MESSAGES", {
    "QUEUED" => "Queued...",
    "PARSING" => "Parsing...",
    "WBI_KEY_WAITING" => "Waiting for the signature key...",
    "PAUSED" => "Paused",
    "FFMPEG_QUEUED" => "Queued for FFmpeg...",
    "MERGING" => "Merging...",
    "MERGING_WITH_PROGRESS" => "Merging... {progress}%",
    "COMPLETED" => "Completed",
    "CONVERTING" => "Converting...",
    "CONVERTING_WITH_PROGRESS" => "Converting... {progress}%",
    "ALREADY_LATEST_VERSION" => "You are already using the latest version",
    "DOWNLOAD_COMPLETED" => "Download completed",
    "DOWNLOAD_COMPLETED_DETAIL" => "All download tasks have been completed.",
    "EXPIRED" => "Expired",
    "ADDITIONAL_FILES" => "Additional Files",
    "DOWNLOADING_DANMAKU" => "Downloading Danmaku...",
    "DOWNLOADING_SUBTITLES" => "Downloading Subtitles...",
    "DOWNLOADING_COVER" => "Downloading Cover...",
    "PARSING_CHAPTER" => "Parsing Chapters...",
    "SCRAPING_METADATA" => "Scraping Metadata...",
    "PARSING_INTERACTIVE_VIDEO_NODE" => "Parsing node: {title}",
    "PARSING_PAGE" => "Parsing page {page}, total {total_page} pages, progress: {progress}%",
    "PARSING_LINK" => "Parsing link {link}, total {total_links} links, progress: {progress}%",
    "VIDEO_CODEC_FALLBACK" => "This video has no {requested}",
    "VIDEO_CODEC_FALLBACK_MESSAGE" => "This video has no {requested}, using {actual}",
    "ADDED_TO_DOWNLOAD_QUEUE" => "Added to download queue",
});

/// `Translator.TERMS_OF_USE()` — HTML text shown in the terms gate.
pub fn terms_of_use(c: &Catalog) -> String {
    // source string lives in the generated catalog under TERMS_OF_USE
    let source = crate::i18n_data::CATALOG
        .iter()
        .find(|(ctx, _, _, _)| *ctx == "TERMS_OF_USE")
        .map(|(_, s, _, _)| *s)
        .expect("TERMS_OF_USE entry exists");
    c.tr("TERMS_OF_USE", source).to_owned()
}

/// `Translator.PRIVACY_POLICY()` — HTML text shown in the privacy dialog.
pub fn privacy_policy(c: &Catalog) -> String {
    let source = crate::i18n_data::CATALOG
        .iter()
        .find(|(ctx, _, _, _)| *ctx == "PRIVACY_POLICY")
        .map(|(_, s, _, _)| *s)
        .expect("PRIVACY_POLICY entry exists");
    c.tr("PRIVACY_POLICY", source).to_owned()
}
