//! Audio parsing (au singles, am menus) — port of
//! `src/util/parse/parser/audio.py` + `src/util/parse/episode/audio.py`.
//!
//! `au` fetches `song/info`; `am` fetches `song/of-menu` (the track list)
//! then `menu/info` (the menu title, written next to the list as
//! `menu_title`). The episode layer turns one response into a root node
//! plus one `AUDIO_BIT` child per track. All endpoints are plain-signed
//! (`NO_WBI_PARSER_TYPES`).

use serde_json::Value;

use crate::tree::{Attribute, Number, TreeItem};

pub const SONG_INFO_API: &str = "https://www.bilibili.com/audio/music-service-c/web/song/info";
pub const SONG_OF_MENU_API: &str =
    "https://www.bilibili.com/audio/music-service-c/web/song/of-menu";
pub const MENU_INFO_API: &str = "https://www.bilibili.com/audio/music-service-c/web/menu/info";

/// Page size for `song/of-menu` (`get_audio_menu_list`); menus longer than
/// 100 tracks are not paginated by the Python build either.
pub const MENU_PAGE_SIZE: i64 = 100;
/// Origin stripped when swapping hosts for tests (`base` prefix).
const ORIGIN_WWW: &str = "https://www.bilibili.com";

/// Errors surfaced by the audio parser.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("network: {0}")]
    Network(#[from] network::HttpError),
    /// `check_response`: non-zero business code.
    #[error("{0}")]
    Api(String),
    #[error("invalid audio link")]
    InvalidLink,
}

/// Result of one audio parse: the response `data` object with `menu_title`
/// folded in (for menus) plus the category key ("audio").
#[derive(Debug, Clone)]
pub struct AudioInfo {
    pub data: Value,
}

/// Fetch the `data` payload of an au/am link, expanding menus to their
/// tracks (`AudioParser.parse`). `url` is the bilibili audio link; host is
/// swapped for `base` in tests.
pub async fn parse_audio(
    client: &network::HttpClient,
    base: &str,
    url: &str,
) -> Result<AudioInfo, AudioError> {
    // Python strips the "audio" path segment before matching am|au.
    let stripped = url.replace("audio", "");
    let (is_menu, sid) = audio_sid(&stripped).ok_or(AudioError::InvalidLink)?;

    if is_menu {
        let mut data = get_song_of_menu(client, base, sid).await?;
        let menu_title = get_menu_info(client, base, sid).await?;
        data["menu_title"] = Value::String(menu_title);
        Ok(AudioInfo { data })
    } else {
        let data = get_song_info(client, base, sid).await?;
        Ok(AudioInfo { data })
    }
}

/// Lift `(is_menu, sid)` off a stripped audio url — Python
/// `find_str(r"am([0-9]+)")` then `find_str(r"au([0-9]+)")`: first match
/// wins, menu pattern preferred.
fn audio_sid(url: &str) -> Option<(bool, i64)> {
    static MENU: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"am([0-9]+)").unwrap());
    static SINGLE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"au([0-9]+)").unwrap());

    if let Some(c) = MENU.captures(url) {
        let sid = c.get(1)?.as_str().parse().ok()?;
        return Some((true, sid));
    }
    let sid = SINGLE.captures(url)?.get(1)?.as_str().parse().ok()?;
    Some((false, sid))
}

async fn get_song_info(
    client: &network::HttpClient,
    base: &str,
    sid: i64,
) -> Result<Value, AudioError> {
    let response: Value = client
        .get_json(
            &format!("{base}{}", SONG_INFO_API.trim_start_matches(ORIGIN_WWW)),
            &[("sid".to_owned(), sid.to_string())],
        )
        .await?;
    check_response(&response)?;
    Ok(response["data"].clone())
}

async fn get_song_of_menu(
    client: &network::HttpClient,
    base: &str,
    sid: i64,
) -> Result<Value, AudioError> {
    let response: Value = client
        .get_json(
            &format!("{base}{}", SONG_OF_MENU_API.trim_start_matches(ORIGIN_WWW)),
            &[
                ("sid".to_owned(), sid.to_string()),
                ("pn".to_owned(), "1".to_owned()),
                ("ps".to_owned(), MENU_PAGE_SIZE.to_string()),
            ],
        )
        .await?;
    check_response(&response)?;
    Ok(response["data"].clone())
}

async fn get_menu_info(
    client: &network::HttpClient,
    base: &str,
    sid: i64,
) -> Result<String, AudioError> {
    let response: Value = client
        .get_json(
            &format!("{base}{}", MENU_INFO_API.trim_start_matches(ORIGIN_WWW)),
            &[("sid".to_owned(), sid.to_string())],
        )
        .await?;
    check_response(&response)?;
    Ok(response["data"]["title"].as_str().unwrap_or_default().to_owned())
}

fn check_response(response: &Value) -> Result<(), AudioError> {
    if response.get("code").and_then(Value::as_i64).unwrap_or(-1) != 0 {
        return Err(AudioError::Api(
            response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
                .to_owned(),
        ));
    }
    Ok(())
}

/// Category label the parse list shows for audio roots
/// (`Translator.EPISODE_TYPE("AUDIO")`).
pub const AUDIO_CATEGORY: &str = "音频";

/// Build the audio episode tree (`AudioEpisodeParser`).
///
/// `info_data` is the `data` object of the song response: a single-song
/// object (no `data` key — normalized to a one-element list) or a menu
/// object whose `data` array is the track list, with `menu_title` as a
/// sibling key. Returns the root node; every track is an
/// [`Attribute::AUDIO_BIT`] child numbered from 1, its `source_title`
/// carrying the menu name (audio entries are never re-parsed, so the menu
/// is just the "source list name", not a structural parent).
pub fn build_audio_tree(info_data: &Value) -> TreeItem {
    let menu_title = info_data
        .get("menu_title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let tracks: Vec<&Value> = match info_data.get("data") {
        Some(Value::Array(list)) => list.iter().collect(),
        // Single-song response: the payload itself is the one track.
        Some(_) | None => vec![info_data],
    };

    let mut root = TreeItem::root(
        Number::Text(AUDIO_CATEGORY.to_owned()),
        &menu_title,
    );
    root.set_attribute(Attribute::TREE_NODE_BIT);

    for (index, track) in tracks.iter().enumerate() {
        let sid = track["statistic"]["sid"].as_i64().unwrap_or(0);
        let mut item = TreeItem::new(
            Number::Int(index as i64 + 1),
            track["title"].as_str().unwrap_or_default(),
            track["cover"].as_str().unwrap_or_default(),
            track["duration"].as_i64().unwrap_or(0),
            sid,
            track["passtime"].as_i64().unwrap_or(0),
            &format!("https://www.bilibili.com/audio/au{sid}"),
            track["author"].as_str().unwrap_or_default(),
            [(
                "source_title".to_owned(),
                Value::String(menu_title.clone()),
            )]
            .into_iter()
            .collect(),
        );
        item.set_attribute(Attribute::AUDIO_BIT);
        root.add_child(item);
    }

    root
}
