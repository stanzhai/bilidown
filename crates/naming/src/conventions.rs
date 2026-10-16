//! The 12 naming conventions and their preview sample data (Python
//! `util/common/data/naming_convention.py`).

use std::collections::HashMap;

use crate::format::Value;

/// Naming convention types (Python `ConventionType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConventionType {
    Normal = 11,
    Part = 12,
    Collection = 13,
    InteractiveVideo = 14,
    Bangumi = 20,
    Cheese = 30,
    Favorite = 40,
    Space = 50,
    History = 60,
    WatchLater = 70,
    Weekly = 80,
    Audio = 90,
}

impl ConventionType {
    pub const ALL: [ConventionType; 12] = [
        ConventionType::Normal,
        ConventionType::Part,
        ConventionType::Collection,
        ConventionType::InteractiveVideo,
        ConventionType::Bangumi,
        ConventionType::Cheese,
        ConventionType::Favorite,
        ConventionType::Space,
        ConventionType::History,
        ConventionType::WatchLater,
        ConventionType::Weekly,
        ConventionType::Audio,
    ];

    pub fn id(&self) -> i64 {
        *self as i64
    }
}

/// Variable data type shown in the editor (Python `VariableType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VariableType {
    Text = 0,
    Datetime = 1,
    Number = 2,
}

/// Preview sample shape (Python `SampleShape`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleShape {
    Single,
    Multi,
    Collection,
}

impl SampleShape {
    pub const ALL: [SampleShape; 3] = [SampleShape::Single, SampleShape::Multi, SampleShape::Collection];
}

/// Entry shapes each type can actually meet.
pub fn supported_shapes(type_: ConventionType) -> &'static [SampleShape] {
    match type_ {
        ConventionType::Normal
        | ConventionType::InteractiveVideo
        | ConventionType::Bangumi
        | ConventionType::Cheese
        | ConventionType::Weekly
        | ConventionType::Audio => &[SampleShape::Single],
        ConventionType::Part => &[SampleShape::Multi],
        ConventionType::Collection => &[SampleShape::Single, SampleShape::Collection],
        ConventionType::Favorite
        | ConventionType::Space
        | ConventionType::History
        | ConventionType::WatchLater => &[SampleShape::Single, SampleShape::Multi, SampleShape::Collection],
    }
}

/// Media kinds mixed into source-type lists; they use their own rules, not
/// the source's (see `formatter::get_type_id_from_attribute`).
pub const MIXED_ENTRY_TYPES: &[(ConventionType, &[ConventionType])] = &[
    (ConventionType::Favorite, &[ConventionType::Bangumi]),
    (ConventionType::History, &[ConventionType::Bangumi, ConventionType::Cheese]),
    (ConventionType::WatchLater, &[ConventionType::Bangumi]),
    (ConventionType::Space, &[ConventionType::Cheese]),
];

/// Variable data types; only these are worth numeric formatting (`{number:02d}`).
pub fn variable_type(name: &str) -> VariableType {
    match name {
        "pub_time" | "create_time" | "fav_time" | "last_watched_time" => VariableType::Datetime,
        "number" | "season_number" | "episode_number" | "p" => VariableType::Number,
        _ => VariableType::Text,
    }
}

/// Numeric variables: "unset" sentinel is `0` (not `""`), so `{p:02d}` /
/// `{aid:05d}` never raise on sample data (the root cause of issue #461's
/// false "invalid rule" verdicts).
const NUMERIC_VARIABLES: &[&str] = &[
    "pub_ts", "create_ts", "fav_ts", "last_watched_ts", "number",
    "uploader_uid", "aid", "cid", "ep_id", "season_id",
    "course_id", "lesson_id", "item_id", "section_id",
    "season_number", "episode_number", "p",
    "favorites_id", "favorites_owner_id", "space_owner_id",
];

const SAMPLE_TIMESTAMP: i64 = 1772841600;

/// `_GENERIC_DESCRIPTION`: same variable, different meaning per type; the
/// `_FOR_XXX` description is only accurate in its own type's primary list.
const GENERIC_DESCRIPTION: &[(&str, &str)] = &[
    ("leaf_title", "LEAF_TITLE_GENERIC"),
    ("parent_title", "PARENT_TITLE_GENERIC"),
    ("p", "PART_NUMBER_GENERIC"),
    ("section_title", "SECTION_TITLE_GENERIC"),
    ("series_title", "SERIES_TITLE_GENERIC"),
    ("episode_title", "EPISODE_TITLE_GENERIC"),
];

/// Type whose `collection_title` is membership info, not shape info: it
/// stays filled under every shape.
const SHAPE_KEEPS_COLLECTION_TYPES: &[ConventionType] = &[ConventionType::Collection];

fn shape_overrides(shape: SampleShape) -> Vec<(&'static str, Value)> {
    use SampleShape::*;
    match shape {
        Single => vec![
            ("parent_title", Value::Text(String::new())),
            ("p", Value::Int(0)),
            ("collection_title", Value::Text(String::new())),
            ("section_title", Value::Text(String::new())),
        ],
        Multi => vec![
            ("parent_title", Value::Text("【KEY社20周年音乐专辑】Key BEST SELECTION".to_string())),
            ("p", Value::Int(4)),
            ("leaf_title", Value::Text("04 アルカテイル".to_string())),
            ("collection_title", Value::Text(String::new())),
            ("section_title", Value::Text(String::new())),
        ],
        Collection => vec![
            ("collection_title", Value::Text("艾尔登法环白金攻略".to_string())),
            ("section_title", Value::Text("DLC黄金树幽影".to_string())),
            ("parent_title", Value::Text("全收集、全流程、全剧情攻略".to_string())),
            ("p", Value::Int(3)),
            ("leaf_title", Value::Text("03【墓地平原-西+艾拉克河】".to_string())),
        ],
    }
}

/// Per-(type, shape) patches over the generic shape overrides.
fn shape_overrides_by_type(type_: ConventionType, shape: SampleShape) -> Vec<(&'static str, Value)> {
    if type_ == ConventionType::Collection && shape == SampleShape::Single {
        return vec![("leaf_title", Value::Text("游戏科学新作《黑神话：钟馗》先导预告".to_string()))];
    }
    Vec::new()
}

/// One variable list entry.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableEntry {
    pub name: &'static str,
    pub variable: &'static str,
    pub description: &'static str,
    pub example: Value,
}

fn t(s: &str) -> Value {
    Value::Text(s.to_string())
}

fn base_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "pub_time", variable: "{pub_time:%Y-%m-%d_%H-%M-%S}", description: "PUB_TIME", example: t("2026-03-07_12-00-00") },
        VariableEntry { name: "pub_ts", variable: "{pub_ts}", description: "PUB_TS", example: t("1772841600") },
        VariableEntry { name: "create_time", variable: "{create_time:%Y-%m-%d_%H-%M-%S}", description: "CREATE_TIME", example: t("2026-03-07_12-00-00") },
        VariableEntry { name: "create_ts", variable: "{create_ts}", description: "CREATE_TS", example: t("1772841600") },
        VariableEntry { name: "number", variable: "{number}", description: "NUMBER", example: Value::Int(1) },
        VariableEntry { name: "uploader", variable: "{uploader}", description: "UPLOADER", example: t("UP主昵称") },
        VariableEntry { name: "uploader_uid", variable: "{uploader_uid}", description: "UPLOADER_UID", example: t("12345678") },
        VariableEntry { name: "video_quality", variable: "{video_quality}", description: "VIDEO_QUALITY", example: t("1080P") },
        VariableEntry { name: "audio_quality", variable: "{audio_quality}", description: "AUDIO_QUALITY", example: t("192K") },
        VariableEntry { name: "video_codec", variable: "{video_codec}", description: "VIDEO_CODEC", example: t("HEVC") },
    ]
}

fn normal_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "aid", variable: "{aid}", description: "AID", example: t("115056436060087") },
        VariableEntry { name: "bvid", variable: "{bvid}", description: "BVID", example: t("BV1sHePzWEbG") },
        VariableEntry { name: "cid", variable: "{cid}", description: "CID", example: t("31809079869") },
        VariableEntry { name: "leaf_title", variable: "{leaf_title}", description: "LEAF_TITLE_FOR_NORMAL", example: t("游戏科学新作《黑神话：钟馗》先导预告") },
    ]
}

fn part_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "parent_title", variable: "{parent_title}", description: "PARENT_TITLE_FOR_PART", example: t("【KEY社20周年音乐专辑】Key BEST SELECTION") },
        VariableEntry { name: "p", variable: "{p}", description: "PART_NUMBER_FOR_PART", example: Value::Int(4) },
        VariableEntry { name: "leaf_title", variable: "{leaf_title}", description: "LEAF_TITLE_FOR_PART", example: t("04 アルカテイル") },
        VariableEntry { name: "aid", variable: "{aid}", description: "AID", example: t("634233546") },
        VariableEntry { name: "bvid", variable: "{bvid}", description: "BVID", example: t("BV1Rb4y1t7gc") },
        VariableEntry { name: "cid", variable: "{cid}", description: "CID", example: t("442845594") },
    ]
}

fn collection_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "collection_title", variable: "{collection_title}", description: "COLLECTION_TITLE", example: t("艾尔登法环白金攻略") },
        VariableEntry { name: "section_title", variable: "{section_title}", description: "SECTION_TITLE_FOR_COLLECTION", example: t("DLC黄金树幽影") },
        VariableEntry { name: "parent_title", variable: "{parent_title}", description: "PARENT_TITLE_FOR_COLLECTION", example: t("全收集、全流程、全剧情攻略") },
        VariableEntry { name: "leaf_title", variable: "{leaf_title}", description: "LEAF_TITLE_FOR_COLLECTION", example: t("03【墓地平原-西+艾拉克河】") },
        VariableEntry { name: "p", variable: "{p}", description: "PART_NUMBER_FOR_COLLECTION", example: Value::Int(3) },
        VariableEntry { name: "aid", variable: "{aid}", description: "AID", example: t("1555921697") },
        VariableEntry { name: "bvid", variable: "{bvid}", description: "BVID", example: t("BV1r1421r78r") },
        VariableEntry { name: "cid", variable: "{cid}", description: "CID", example: t("1599644073") },
    ]
}

fn interactive_video_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "leaf_title", variable: "{leaf_title}", description: "LEAF_TITLE_FOR_INTERACTIVE_VIDEO", example: t("序幕") },
        VariableEntry { name: "parent_title", variable: "{parent_title}", description: "PARENT_TITLE_FOR_INTERACTIVE_VIDEO", example: t("【互动视频】你能逃出这个房间吗？") },
    ]
}

fn bangumi_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "series_title", variable: "{series_title}", description: "SERIES_TITLE_FOR_BANGUMI", example: t("轻音少女") },
        VariableEntry { name: "season_title", variable: "{season_title}", description: "SEASON_TITLE", example: t("轻音少女 第二季") },
        VariableEntry { name: "section_title", variable: "{section_title}", description: "SECTION_TITLE_FOR_BANGUMI_CHEESE", example: t("正片") },
        VariableEntry { name: "episode_title", variable: "{episode_title}", description: "EPISODE_TITLE", example: t("第18话 主角！") },
        VariableEntry { name: "season_number", variable: "{season_number}", description: "SEASON_NUMBER", example: Value::Int(2) },
        VariableEntry { name: "episode_number", variable: "{episode_number}", description: "EPISODE_NUMBER", example: Value::Int(18) },
        VariableEntry { name: "aid", variable: "{aid}", description: "AID", example: t("849267924") },
        VariableEntry { name: "bvid", variable: "{bvid}", description: "BVID", example: t("BV1NL4y1i7L2") },
        VariableEntry { name: "cid", variable: "{cid}", description: "CID", example: t("91560482") },
        VariableEntry { name: "ep_id", variable: "{ep_id}", description: "EP_ID", example: t("21296") },
        VariableEntry { name: "season_id", variable: "{season_id}", description: "SEASON_ID", example: t("1173") },
    ]
}

fn cheese_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "series_title", variable: "{series_title}", description: "SERIES_TITLE_FOR_CHEESE", example: t("【618限时价】清华梁爽：0-N1日语精讲高级班") },
        VariableEntry { name: "section_title", variable: "{section_title}", description: "SECTION_TITLE_FOR_BANGUMI_CHEESE", example: t("先导片") },
        VariableEntry { name: "episode_title", variable: "{episode_title}", description: "EPISODE_TITLE", example: t("【先导片】清华梁爽带你重新定义日语学习，教你说好、更能考好日语！") },
        VariableEntry { name: "aid", variable: "{aid}", description: "AID", example: t("315163535") },
        VariableEntry { name: "cid", variable: "{cid}", description: "CID", example: t("1639897333") },
        VariableEntry { name: "ep_id", variable: "{ep_id}", description: "EP_ID", example: t("158662") },
        VariableEntry { name: "season_id", variable: "{season_id}", description: "SEASON_ID", example: t("4016") },
        VariableEntry { name: "course_id", variable: "{course_id}", description: "COURSE_ID", example: t("1000625147") },
        VariableEntry { name: "lesson_id", variable: "{lesson_id}", description: "LESSON_ID", example: t("180281190609920") },
        VariableEntry { name: "item_id", variable: "{item_id}", description: "ITEM_ID", example: t("10302975") },
        VariableEntry { name: "section_id", variable: "{section_id}", description: "SECTION_ID", example: t("180281190650881") },
    ]
}

fn favorite_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "parent_title", variable: "{parent_title}", description: "PARENT_TITLE_FOR_FAVORITE", example: t("【KEY社20周年音乐专辑】Key BEST SELECTION") },
        VariableEntry { name: "p", variable: "{p}", description: "PART_NUMBER_FOR_COLLECTION", example: Value::Int(3) },
        VariableEntry { name: "favorites_name", variable: "{favorites_name}", description: "FAVORITES_NAME", example: t("默认收藏夹") },
        VariableEntry { name: "favorites_id", variable: "{favorites_id}", description: "FAVORITES_ID", example: t("12345678") },
        VariableEntry { name: "favorites_owner", variable: "{favorites_owner}", description: "FAVORITES_OWNER", example: t("用户昵称") },
        VariableEntry { name: "favorites_owner_id", variable: "{favorites_owner_id}", description: "FAVORITES_OWNER_ID", example: t("12345678") },
        VariableEntry { name: "fav_time", variable: "{fav_time:%Y-%m-%d_%H-%M-%S}", description: "FAV_TIME", example: t("2026-03-07_12-00-00") },
        VariableEntry { name: "fav_ts", variable: "{fav_ts}", description: "FAV_TS", example: t("1772841600") },
    ]
}

fn space_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "space_owner", variable: "{space_owner}", description: "SPACE_OWNER", example: t("用户昵称") },
        VariableEntry { name: "space_owner_id", variable: "{space_owner_id}", description: "SPACE_OWNER_ID", example: t("12345678") },
    ]
}

fn history_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "source_title", variable: "{source_title}", description: "SOURCE_TITLE", example: t("历史记录") },
        VariableEntry { name: "parent_title", variable: "{parent_title}", description: "PARENT_TITLE_FOR_HISTORY", example: t("【KEY社20周年音乐专辑】Key BEST SELECTION") },
        VariableEntry { name: "leaf_title", variable: "{leaf_title}", description: "LEAF_TITLE_FOR_NORMAL", example: t("游戏科学新作《黑神话：钟馗》先导预告") },
        VariableEntry { name: "last_watched_time", variable: "{last_watched_time:%Y-%m-%d_%H-%M-%S}", description: "LAST_WATCHED_TIME", example: t("2026-03-07_12-00-00") },
        VariableEntry { name: "last_watched_ts", variable: "{last_watched_ts}", description: "LAST_WATCHED_TS", example: t("1772841600") },
    ]
}

fn watch_later_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "source_title", variable: "{source_title}", description: "SOURCE_TITLE", example: t("稍后再看") },
        VariableEntry { name: "parent_title", variable: "{parent_title}", description: "PARENT_TITLE_FOR_WATCH_LATER", example: t("【KEY社20周年音乐专辑】Key BEST SELECTION") },
        VariableEntry { name: "leaf_title", variable: "{leaf_title}", description: "LEAF_TITLE_FOR_NORMAL", example: t("游戏科学新作《黑神话：钟馗》先导预告") },
        VariableEntry { name: "fav_time", variable: "{fav_time:%Y-%m-%d_%H-%M-%S}", description: "FAV_TIME", example: t("2026-03-07_12-00-00") },
        VariableEntry { name: "fav_ts", variable: "{fav_ts}", description: "FAV_TS", example: t("1772841600") },
    ]
}

fn weekly_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "source_title", variable: "{source_title}", description: "SOURCE_TITLE", example: t("第377期(0612更新)") },
        VariableEntry { name: "leaf_title", variable: "{leaf_title}", description: "LEAF_TITLE_FOR_NORMAL", example: t("游戏科学新作《黑神话：钟馗》先导预告") },
    ]
}

fn audio_variables() -> Vec<VariableEntry> {
    vec![
        VariableEntry { name: "pub_time", variable: "{pub_time:%Y-%m-%d_%H-%M-%S}", description: "PUB_TIME", example: t("2026-03-07_12-00-00") },
        VariableEntry { name: "pub_ts", variable: "{pub_ts}", description: "PUB_TS", example: t("1772841600") },
        VariableEntry { name: "create_time", variable: "{create_time:%Y-%m-%d_%H-%M-%S}", description: "CREATE_TIME", example: t("2026-03-07_12-00-00") },
        VariableEntry { name: "create_ts", variable: "{create_ts}", description: "CREATE_TS", example: t("1772841600") },
        VariableEntry { name: "number", variable: "{number}", description: "NUMBER", example: t("1") },
        VariableEntry { name: "leaf_title", variable: "{leaf_title}", description: "LEAF_TITLE_FOR_AUDIO", example: t("歌曲名称") },
        VariableEntry { name: "uploader", variable: "{uploader}", description: "UPLOADER_FOR_AUDIO", example: t("歌手") },
        VariableEntry { name: "source_title", variable: "{source_title}", description: "SOURCE_TITLE", example: t("歌单名称") },
        VariableEntry { name: "audio_quality", variable: "{audio_quality}", description: "AUDIO_QUALITY", example: t("192K") },
    ]
}

fn primary_variables(type_: ConventionType) -> Vec<VariableEntry> {
    match type_ {
        ConventionType::History => history_variables(),
        ConventionType::WatchLater => watch_later_variables(),
        ConventionType::Weekly => weekly_variables(),
        ConventionType::Audio => audio_variables(),
        _ => {
            let base = base_variables();
            match type_ {
                ConventionType::Normal => [base, normal_variables()].concat(),
                ConventionType::Part => [base, part_variables()].concat(),
                ConventionType::Collection => [base, collection_variables()].concat(),
                ConventionType::InteractiveVideo => [base, interactive_video_variables()].concat(),
                ConventionType::Bangumi => [base, bangumi_variables()].concat(),
                ConventionType::Cheese => [base, cheese_variables()].concat(),
                ConventionType::Favorite => [base, normal_variables(), favorite_variables()].concat(),
                ConventionType::Space => [base, normal_variables(), space_variables()].concat(),
                _ => unreachable!(),
            }
        }
    }
}

/// All variables deduped by name, ordered by first appearance across the
/// primary lists in `convention_type_map` order.
pub fn all_variables() -> Vec<VariableEntry> {
    let mut entries: Vec<VariableEntry> = Vec::new();
    let mut known: Vec<&str> = Vec::new();

    for type_ in ConventionType::ALL {
        for entry in primary_variables(type_) {
            if !known.contains(&entry.name) {
                known.push(entry.name);
                entries.push(entry);
            }
        }
    }

    entries
}

/// Decorated list entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DecoratedEntry {
    pub name: &'static str,
    pub variable: &'static str,
    pub description: &'static str,
    pub example: Value,
    pub group: &'static str,
    pub type_: VariableType,
}

/// Variable list factory (Python `VariableListFactory`).
#[derive(Debug, Clone, Default)]
pub struct VariableListFactory;

impl VariableListFactory {
    pub fn new() -> Self {
        Self
    }

    /// The type's variable list; `full` appends every other variable after
    /// the recommended ones (the key space is constant — per-type lists are
    /// just UI "recommended" grouping; feeding a trimmed list to the
    /// formatter would make the editor reject rules the runtime accepts).
    pub fn build(&self, type_: ConventionType, full: bool) -> Vec<DecoratedEntry> {
        let primary: Vec<DecoratedEntry> = primary_variables(type_)
            .into_iter()
            .map(|e| self.decorate(e, "PRIMARY", false))
            .collect();

        if !full {
            return primary;
        }

        let known: Vec<&str> = primary.iter().map(|e| e.name).collect();

        let more: Vec<DecoratedEntry> = all_variables()
            .into_iter()
            .filter(|e| !known.contains(&e.name))
            .map(|e| self.decorate(e, "MORE", true))
            .collect();

        [primary, more].concat()
    }

    /// Preview variable data with the runtime's exact key space.
    pub fn build_variable_data(&self, type_: ConventionType, shape: SampleShape) -> HashMap<String, Value> {
        let mut data: HashMap<String, Value> = all_variables()
            .into_iter()
            .map(|e| (e.name.to_string(), unset_value(e.name)))
            .collect();

        for entry in primary_variables(type_) {
            data.insert(entry.name.to_string(), entry.example);
        }

        let keeps_collection = SHAPE_KEEPS_COLLECTION_TYPES.contains(&type_);
        for (name, value) in shape_overrides(shape) {
            if name == "collection_title" && keeps_collection {
                continue;
            }
            data.insert(name.to_string(), value.clone());
        }

        for (name, value) in shape_overrides_by_type(type_, shape) {
            data.insert(name.to_string(), value.clone());
        }

        // Time variables must be datetimes, or {pub_time:%Y-%m-%d} raises.
        let sample_time = crate::formatter::time_from_timestamp(SAMPLE_TIMESTAMP);
        for name in ["pub_time", "create_time", "fav_time", "last_watched_time"] {
            data.insert(name.to_string(), sample_time.clone());
        }

        data
    }

    fn decorate(&self, entry: VariableEntry, group: &'static str, generic: bool) -> DecoratedEntry {
        let description = if generic {
            GENERIC_DESCRIPTION
                .iter()
                .find(|(name, _)| *name == entry.name)
                .map(|(_, desc)| *desc)
                .unwrap_or(entry.description)
        } else {
            entry.description
        };

        DecoratedEntry {
            name: entry.name,
            variable: entry.variable,
            description,
            example: entry.example,
            group,
            type_: variable_type(entry.name),
        }
    }
}

fn unset_value(name: &str) -> Value {
    if NUMERIC_VARIABLES.contains(&name) {
        Value::Int(0)
    } else {
        Value::Text(String::new())
    }
}

/// English variable descriptions (Python `Translator.VARIABLE_DESCRIPTION`
/// fallbacks) — used by tests to pin description keys.
pub const VARIABLE_DESCRIPTION: &[(&str, &str)] = &[
    ("PUB_TIME", "Video publish time"),
    ("PUB_TS", "Video publish timestamp"),
    ("CREATE_TIME", "Download task creation time"),
    ("CREATE_TS", "Download task creation timestamp"),
    ("FAV_TIME", "Favorite time"),
    ("FAV_TS", "Favorite timestamp"),
    ("LAST_WATCHED_TIME", "Last watched time"),
    ("LAST_WATCHED_TS", "Last watched timestamp"),
    ("NUMBER", "Sequence number"),
    ("UPLOADER", "Uploader name"),
    ("UPLOADER_FOR_AUDIO", "Author of the song"),
    ("UPLOADER_UID", "Uploader id"),
    ("VIDEO_QUALITY", "Video quality"),
    ("AUDIO_QUALITY", "Audio quality"),
    ("VIDEO_CODEC", "Video codec"),
    ("AID", "av number"),
    ("BVID", "BV number"),
    ("CID", "cid"),
    ("EP_ID", "Episode id"),
    ("SEASON_ID", "Season id"),
    ("LEAF_TITLE_GENERIC", "Item title"),
    ("PARENT_TITLE_GENERIC", "Parent title"),
    ("PART_NUMBER_GENERIC", "Part number"),
    ("SECTION_TITLE_GENERIC", "Section title (empty if not divided into sections)"),
    ("SERIES_TITLE_GENERIC", "Series title"),
    ("EPISODE_TITLE_GENERIC", "Episode title"),
    ("LEAF_TITLE_FOR_NORMAL", "Full video title"),
    ("LEAF_TITLE_FOR_PART", "Current part's title"),
    ("LEAF_TITLE_FOR_COLLECTION", "Content title (video title for single videos, part title for multi-part)"),
    ("LEAF_TITLE_FOR_INTERACTIVE_VIDEO", "Node title"),
    ("LEAF_TITLE_FOR_AUDIO", "Song title"),
    ("PARENT_TITLE_FOR_PART", "Multi-part video main title"),
    ("PARENT_TITLE_FOR_COLLECTION", "Main title if video has multiple parts; otherwise empty"),
    ("PARENT_TITLE_FOR_INTERACTIVE_VIDEO", "Interactive video title"),
    ("PARENT_TITLE_FOR_HISTORY", "Title of the video; only set for multi-part or collection entries"),
    ("PARENT_TITLE_FOR_WATCH_LATER", "Title of the video; only set for multi-part or collection entries"),
    ("PARENT_TITLE_FOR_FAVORITE", "Multi-part video main title (if exists)"),
    ("SOURCE_TITLE", "Name of the source list"),
    ("PART_NUMBER_FOR_PART", "Part number"),
    ("PART_NUMBER_FOR_COLLECTION", "Part number (only for multi-part videos)"),
    ("COLLECTION_TITLE", "Collection title"),
    ("SECTION_TITLE_FOR_COLLECTION", "Section title (empty if not divided into sections)"),
    ("SECTION_TITLE_FOR_BANGUMI_CHEESE", "Section title"),
    ("SERIES_TITLE_FOR_BANGUMI", "Series title"),
    ("SERIES_TITLE_FOR_CHEESE", "Courses title"),
    ("COURSE_ID", "Mall courses id"),
    ("LESSON_ID", "Mall lesson id"),
    ("ITEM_ID", "Mall item id"),
    ("SECTION_ID", "Mall section id"),
    ("SEASON_TITLE", "Season title"),
    ("EPISODE_TITLE", "Episode title"),
    ("SEASON_NUMBER", "Season number"),
    ("EPISODE_NUMBER", "Episode number"),
    ("FAVORITES_NAME", "Favorites name"),
    ("FAVORITES_ID", "Favorites id"),
    ("FAVORITES_OWNER", "Favorites owner"),
    ("FAVORITES_OWNER_ID", "Favorites owner id"),
    ("SPACE_OWNER", "User name"),
    ("SPACE_OWNER_ID", "User id"),
];
