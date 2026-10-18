//! Bilibili URL classification — which of the URL families a link targets.
//!
//! The dispatch table is **order-sensitive**: `space.bilibili.com/{mid}/lists`
//! and `/favlist` both also match the catch-all space rule, so only their
//! earlier position distinguishes them. Likewise `/list/ml{id}` is a favorites
//! folder while `/list/{id}` is a collection. Reordering the table (even to
//! "tidy it up") swallows every non-plain-space type; in the UI that shows up
//! as "parsed the uploader's profile instead of the favorites folder".
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use url::Url;

/// Which URL family a link targets. Variant names mirror the dispatch-table
/// keys of the Python app; `key()` returns that wire name (the
/// `USER_UPLOADS`-style category names come from the parser layer, not from
/// the URL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParserType {
    /// av/BV uploads (incl. multi-P and collections).
    Video,
    /// Bangumi/movie/documentary/TV/variety (ep/ss/md).
    Bangumi,
    /// Cheese course (课程).
    Cheese,
    /// Mall lesson (会员购商城课程).
    Lesson,
    /// Collection (合集/系列 list).
    List,
    /// Favorites folder (收藏夹).
    Favlist,
    /// Uploader space (个人空间).
    Space,
    /// Weekly popular (每周必看).
    Popular,
    /// Festival page (活动页) — real type known only after expansion.
    Festival,
    /// b23.tv / bili2233.cn short link — real type known only after expansion.
    B23,
    /// Internal pseudo-protocol `bilidown://watch_later`.
    WatchLater,
    /// Internal pseudo-protocol `bilidown://history`.
    History,
    /// Audio `au…` and audio menu `am…`.
    Audio,
}

impl ParserType {
    /// Dispatch-table key used across the app.
    pub fn key(self) -> &'static str {
        match self {
            ParserType::Video => "video",
            ParserType::Bangumi => "bangumi",
            ParserType::Cheese => "cheese",
            ParserType::Lesson => "lesson",
            ParserType::List => "list",
            ParserType::Favlist => "favlist",
            ParserType::Space => "space",
            ParserType::Popular => "popular",
            ParserType::Festival => "festival",
            ParserType::B23 => "b23",
            ParserType::WatchLater => "watch_later",
            ParserType::History => "history",
            ParserType::Audio => "audio",
        }
    }
}

/// Raw ids lifted off the link, before any request is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawIds {
    /// `av123`, `BV1xx…`, `/video/BV…` — av number and/or BV id string.
    Video { av: Option<u64>, bv: Option<String> },
    /// `ep…`/`ss…`/`md…` id with its family.
    Bangumi { kind: BangumiId, id: u64 },
    /// `cheese/play/(ss|ep)…`.
    Cheese { kind: CheeseId, id: u64 },
    /// mall lesson query ids (`courseId`/`lessonId`/`itemId`).
    Lesson {
        course_id: u64,
        lesson_id: u64,
        item_id: u64,
    },
    /// Collection id from the path; `?type=season|series` marks the id's
    /// family on `space/{mid}/lists` links, play-page links carry
    /// `sid`/`oid`/`bvid` extras instead.
    List {
        id: u64,
        season: Option<u64>,
        series: Option<u64>,
        sid: Option<u64>,
        oid: Option<u64>,
        bvid: Option<String>,
    },
    /// Favorites folder: `ml…` media id (or `fid=` query), plus the owner
    /// mid for personal-space favlist links.
    Favlist {
        media_id: Option<u64>,
        owner_mid: Option<u64>,
    },
    /// Space owner uid.
    Space { mid: u64 },
    /// Weekly popular issue number, when `num=` is given.
    Popular { number: Option<u64> },
    /// Short link — real ids live behind the redirect.
    ShortLink,
    /// Watch-later list; no ids.
    WatchLater,
    /// Parse history; no ids.
    History,
    /// `au…` (single audio) or `am…` (audio menu).
    Audio { kind: AudioId, id: u64 },
}

/// Which id family a bangumi link carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BangumiId {
    /// `ep` episode id.
    Ep,
    /// `ss` season id.
    Ss,
    /// `md` media id.
    Md,
}

/// Which id family a cheese link carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheeseId {
    /// `ss` season id.
    Ss,
    /// `ep` episode id.
    Ep,
}

/// Which id family an audio link carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioId {
    /// `au` single audio.
    Au,
    /// `am` audio menu.
    Am,
}

/// One row of the order-sensitive dispatch table: regex plus the
/// id-extractor for the family it recognizes.
struct Row {
    pattern: &'static str,
    parser_type: ParserType,
    ids: fn(&str) -> RawIds,
}

/// The table, ported verbatim from `url_pattern.py`. Order matters;
/// see the module docs.
static ROWS: &[Row] = &[
    Row {
        pattern: r"bilibili\.com/video/([a-zA-Z0-9]+)",
        parser_type: ParserType::Video,
        ids: ids_video,
    },
    Row {
        pattern: r"bilibili\.com/bangumi/(play|media)/(ss\d+|ep\d+|md\d+)",
        parser_type: ParserType::Bangumi,
        ids: ids_bangumi,
    },
    Row {
        pattern: r"bilibili\.com/cheese/play/(ss\d+|ep\d+)",
        parser_type: ParserType::Cheese,
        ids: ids_cheese,
    },
    Row {
        pattern: r"mall\.bilibili\.com/lesson/play",
        parser_type: ParserType::Lesson,
        ids: ids_lesson,
    },
    // live.bilibili.com/{room} is deliberately absent here too
    Row {
        pattern: r"space\.bilibili\.com/(\d+)/lists",
        parser_type: ParserType::List,
        ids: ids_list,
    },
    Row {
        pattern: r"space\.bilibili\.com/(\d+)/favlist",
        parser_type: ParserType::Favlist,
        ids: ids_favlist,
    },
    Row {
        pattern: r"www\.bilibili\.com/list/ml(\d+)",
        parser_type: ParserType::Favlist,
        ids: ids_favlist,
    },
    Row {
        pattern: r"space\.bilibili\.com/(\d+)",
        parser_type: ParserType::Space,
        ids: ids_space,
    },
    Row {
        pattern: r"www\.bilibili\.com/medialist/play/(\d+)",
        parser_type: ParserType::Space,
        ids: ids_space,
    },
    Row {
        pattern: r"bilibili\.com/list/(\d+)",
        parser_type: ParserType::List,
        ids: ids_list,
    },
    Row {
        pattern: r"bilibili\.com/v/popular",
        parser_type: ParserType::Popular,
        ids: ids_popular,
    },
    Row {
        pattern: r"bilidown://watch_later",
        parser_type: ParserType::WatchLater,
        ids: |_| RawIds::WatchLater,
    },
    Row {
        pattern: r"bilidown://history",
        parser_type: ParserType::History,
        ids: |_| RawIds::History,
    },
    Row {
        pattern: r"bilibili\.com/festival",
        parser_type: ParserType::Festival,
        ids: |_| RawIds::ShortLink,
    },
    Row {
        pattern: r"(b23\.tv|bili2233\.cn)",
        parser_type: ParserType::B23,
        ids: |_| RawIds::ShortLink,
    },
    // 裸 ID bare ids
    Row {
        pattern: r"(?:BV|bv|AV|av)(\w+)",
        parser_type: ParserType::Video,
        ids: ids_video,
    },
    Row {
        pattern: r"(ep[0-9]+|ss[0-9]+)|md[0-9]+",
        parser_type: ParserType::Bangumi,
        ids: ids_bangumi,
    },
    Row {
        pattern: r"(am[0-9]+)|(au[0-9]+)",
        parser_type: ParserType::Audio,
        ids: ids_audio,
    },
];

static COMPILED: LazyLock<Vec<(Regex, &'static Row)>> = LazyLock::new(|| {
    ROWS.iter()
        .map(|row| {
            (
                Regex::new(row.pattern).expect("url pattern must compile"),
                row,
            )
        })
        .collect()
});

/// Classified link: its Parser Type plus the raw ids lifted off it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub parser_type: ParserType,
    pub ids: RawIds,
}

/// Match a URL to its Parser Type and lift its raw ids; first table row wins.
///
/// Query strings are carried through untouched — `?keyword=…` must not
/// change dispatch. Unknown links return `None`.
pub fn classify(url: &str) -> Option<Classified> {
    for (regex, row) in COMPILED.iter() {
        if regex.is_match(url) {
            return Some(Classified {
                parser_type: row.parser_type,
                ids: (row.ids)(url),
            });
        }
    }
    None
}

/// Parser Type alone (`search_url`-style dispatch).
pub fn parser_type(url: &str) -> Option<ParserType> {
    COMPILED
        .iter()
        .find(|(regex, _)| regex.is_match(url))
        .map(|(_, row)| row.parser_type)
}

fn ids_video(url: &str) -> RawIds {
    static BV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:BV|bv)\w+").unwrap());
    static AV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:av|AV)([0-9]+)").unwrap());
    let bv = BV.find(url).map(|m| m.as_str().to_owned());
    let av = AV
        .captures(url)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok());
    RawIds::Video { av, bv }
}

fn ids_bangumi(url: &str) -> RawIds {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(ep|ss|md)([0-9]+)").unwrap());
    if let Some(caps) = RE.captures(url) {
        let kind = match &caps[1] {
            "ep" => BangumiId::Ep,
            "ss" => BangumiId::Ss,
            _ => BangumiId::Md,
        };
        if let Ok(id) = caps[2].parse() {
            return RawIds::Bangumi { kind, id };
        }
    }
    RawIds::Bangumi {
        kind: BangumiId::Ep,
        id: 0,
    }
}

fn ids_cheese(url: &str) -> RawIds {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:ss|ep)([0-9]+)").unwrap());
    if let Some(caps) = RE.captures(url) {
        let kind = if url.contains("/ss") {
            CheeseId::Ss
        } else {
            CheeseId::Ep
        };
        if let Ok(id) = caps[1].parse() {
            return RawIds::Cheese { kind, id };
        }
    }
    RawIds::Cheese {
        kind: CheeseId::Ss,
        id: 0,
    }
}

fn ids_lesson(url: &str) -> RawIds {
    let pairs = query_pairs(url);
    let get = |name: &str| pairs.get(name).and_then(|v| v.parse().ok()).unwrap_or(0);
    RawIds::Lesson {
        course_id: get("courseId"),
        lesson_id: get("lessonId"),
        item_id: get("itemId"),
    }
}

fn ids_list(url: &str) -> RawIds {
    // Only the path is searched for the collection id, so query params
    // (oid/bvid/sid) don't interfere with it.
    static PATH_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/([0-9]+)").unwrap());
    let id = PATH_ID
        .captures(&url_path(url))
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u64>().ok())
        .unwrap_or(0);

    let pairs = query_pairs(url);
    let by_param = |name: &str| pairs.get(name).and_then(|v| v.parse().ok());
    // `space/{mid}/lists?type=season|series` marks the id's family
    let (season, series) = match pairs.get("type").map(String::as_str) {
        Some("season") => (Some(id), None),
        Some("series") => (None, Some(id)),
        _ => (None, None),
    };

    RawIds::List {
        id,
        season,
        series,
        sid: by_param("sid"),
        oid: by_param("oid"),
        bvid: pairs.get("bvid").cloned(),
    }
}

fn ids_favlist(url: &str) -> RawIds {
    static ML: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"ml(\d+)").unwrap());
    static FID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"fid=(\d+)").unwrap());
    let media_id = ML
        .captures(url)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .or_else(|| {
            FID.captures(url)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse().ok())
        });
    RawIds::Favlist {
        media_id,
        owner_mid: first_number_in_path(url),
    }
}

fn ids_space(url: &str) -> RawIds {
    RawIds::Space {
        mid: first_number_in_path(url).unwrap_or(0),
    }
}

fn ids_popular(url: &str) -> RawIds {
    RawIds::Popular {
        number: query_pairs(url).get("num").and_then(|v| v.parse().ok()),
    }
}

fn ids_audio(url: &str) -> RawIds {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(am|au)([0-9]+)").unwrap());
    if let Some(caps) = RE.captures(url) {
        let kind = if &caps[1] == "am" {
            AudioId::Am
        } else {
            AudioId::Au
        };
        if let Ok(id) = caps[2].parse() {
            return RawIds::Audio { kind, id };
        }
    }
    RawIds::Audio {
        kind: AudioId::Au,
        id: 0,
    }
}

fn first_number_in_path(url: &str) -> Option<u64> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/([0-9]+)").unwrap());
    RE.captures(&url_path(url))
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

fn url_path(url: &str) -> String {
    Url::parse(url)
        .map(|u| u.path().to_owned())
        .unwrap_or_else(|_| url.to_owned())
}

fn query_pairs(url: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(parsed) = Url::parse(url) {
        for (k, v) in parsed.query_pairs() {
            map.insert(k.into_owned(), v.into_owned());
        }
    }
    map
}
