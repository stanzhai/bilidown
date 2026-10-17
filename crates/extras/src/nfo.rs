//! `MetadataParser` + `MetadataNFO` (`additional/metadata.py`,
//! `additional/file/metadata_nfo.py`): NFO sidecars, the JSON alternative
//! and the tvshow poster.

use std::fs;
use std::path::PathBuf;

use chrono::{Datelike, TimeZone};
use serde_json::{Map, Value};

use config::ConfigStore;
use database::TaskInfo;
use network::HttpClient;

use crate::options::resolve;
use crate::python_truthy;
use crate::writer::{self, Contents};
use crate::Labels;

pub const VIDEO_BIT: i64 = 1 << 0;
pub const BANGUMI_BIT: i64 = 1 << 1;
pub const CHEESE_BIT: i64 = 1 << 2;
pub const LESSON_BIT: i64 = 1 << 18;

pub const TAG_API: &str = "https://api.bilibili.com/x/web-interface/view/detail/tag";

const VIDEO_BASE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<movie>
    <title>{title}</title>
    <plot>{plot}</plot>
    <runtime>{runtime}</runtime>
    <premiered>{premiered}</premiered>
    <year>{year}</year>
    <actor>
        <name>{uploader}</name>
        <role>UP主</role>
        <profile>https://space.bilibili.com/{uploader_uid}</profile>
        <thumb>{uploader_face}</thumb>
    </actor>
    {tag}
    <thumb>{thumb}</thumb>
    <uniqueid type="bvid">{bvid}</uniqueid>
</movie>
"#;

const TVSHOW_BASE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tvshow>
    <title>{title}</title>
    <plot>{plot}</plot>
    <premiered>{premiered}</premiered>
    <year>{year}</year>
    <studio>Bilibili</studio>
    <director>{director}</director>
    {genre}
    {country}
    {rating}
    {status}
    <thumb aspect="poster">{thumb}</thumb>
    <uniqueid type="season_id">{season_id}</uniqueid>
</tvshow>
"#;

const EPISODE_BASE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<episodedetails>
    <title>{title}</title>
    <plot>{plot}</plot>
    <runtime>{runtime}</runtime>
    <premiered>{premiered}</premiered>
    <year>{year}</year>
    <studio>Bilibili</studio>
    <episode>{episode}</episode>
    <director>{director}</director>
    {genre}
    {country}
    <thumb>{thumb}</thumb>
    <uniqueid type="ep_id">{ep_id}</uniqueid>
</episodedetails>
"#;

/// `Time.from_timestamp` + `strftime("%Y-%m-%d")`: normalize millisecond
/// timestamps, convert in the local zone, fall back to epoch arithmetic
/// outside the platform's range, zero-pad the year to four digits.
pub fn date_from_timestamp(timestamp: i64) -> String {
    let seconds = if timestamp.unsigned_abs() >= 10_000_000_000 {
        timestamp as f64 / 1000.0
    } else {
        timestamp as f64
    };
    let naive = chrono::Local
        .timestamp_opt(seconds as i64, 0)
        .single()
        .map(|dt| dt.naive_local())
        .unwrap_or_else(|| {
            chrono::DateTime::<chrono::Utc>::UNIX_EPOCH.naive_utc()
                + chrono::Duration::seconds(seconds as i64)
        });
    format!(
        "{:04}-{:02}-{:02}",
        naive.year(),
        naive.month(),
        naive.day()
    )
}

fn year_from_timestamp(timestamp: i64) -> String {
    date_from_timestamp(timestamp)
        .split('-')
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// `math.ceil(duration / 60)` over positive durations.
fn runtime_minutes(duration: i64) -> i64 {
    (duration as f64 / 60.0).ceil() as i64
}

fn joined_tags(items: &[String], tag: &str) -> String {
    items
        .iter()
        .map(|t| format!("<{tag}>{t}</{tag}>"))
        .collect::<Vec<_>>()
        .join("\n    ")
}

/// The dedented Bilibili rating block; empty when the rating is falsy.
fn rating_block(rating: f64, votes: i64) -> String {
    if !python_truthy(&Value::from(rating)) {
        return String::new();
    }
    let rating = crate::py_float(rating);
    format!(
        "<ratings>\n        <rating default=\"true\" max=\"10\" name=\"Bilibili\">\n            <value>{rating}</value>\n            <votes>{votes}</votes>\n        </rating>\n    </ratings>\n    <rating>{rating}</rating>"
    )
}

fn status_block(new_ep_status: bool) -> &'static str {
    if new_ep_status {
        "<status>Ongoing</status>"
    } else {
        "<status>Ended</status>"
    }
}

/// `MetadataNFO.generate` pure half: the NFO documents to write.
#[derive(Debug, Clone)]
pub struct NfoDocument {
    pub contents: String,
    pub name: String,
    pub qualifier: Vec<String>,
}

pub fn generate(task: &TaskInfo, metadata_qualifier: &str) -> Vec<NfoDocument> {
    let mut list = Vec::new();
    let attr = task.Episode.attribute;
    let e = &task.Episode;

    if attr & VIDEO_BIT != 0 {
        list.push(NfoDocument {
            contents: VIDEO_BASE
                .replace("{title}", &task.Basic.show_title)
                .replace("{plot}", &e.description)
                .replace("{runtime}", &runtime_minutes(e.duration).to_string())
                .replace("{premiered}", &date_from_timestamp(e.pubtime))
                .replace("{year}", &year_from_timestamp(e.pubtime))
                .replace("{uploader_uid}", &e.uploader_uid.to_string())
                .replace("{uploader_face}", &e.uploader_face)
                .replace("{uploader}", &e.uploader)
                .replace("{tag}", &joined_tags(&e.tags, "tag"))
                .replace("{thumb}", &e.cover)
                .replace("{bvid}", &e.bvid),
            name: task.File.name.clone(),
            qualifier: vec![metadata_qualifier.to_owned()],
        });
    }

    if attr & (BANGUMI_BIT | CHEESE_BIT | LESSON_BIT) != 0 {
        // 确保 tvshow.nfo 不重复生成
        if !tvshow_exists(task) {
            list.push(NfoDocument {
                contents: TVSHOW_BASE
                    .replace("{title}", &e.season_title)
                    .replace("{plot}", &e.description)
                    .replace("{premiered}", &date_from_timestamp(e.premiered))
                    .replace("{year}", &year_from_timestamp(e.premiered))
                    .replace("{director}", &e.uploader)
                    .replace("{genre}", &joined_tags(&e.styles, "genre"))
                    .replace("{country}", &joined_tags(&e.areas, "country"))
                    .replace("{rating}", &rating_block(e.rating, e.rating_votes))
                    .replace("{status}", status_block(e.new_ep_status))
                    .replace("{thumb}", &e.poster)
                    .replace("{season_id}", &e.season_id.to_string()),
                name: "tvshow".to_owned(),
                qualifier: Vec::new(),
            });
        }
        list.push(NfoDocument {
            contents: EPISODE_BASE
                .replace("{title}", &e.episode_title)
                .replace("{plot}", &e.episode_plot)
                .replace("{runtime}", &runtime_minutes(e.duration).to_string())
                .replace("{premiered}", &date_from_timestamp(e.pubtime))
                .replace("{year}", &year_from_timestamp(e.pubtime))
                .replace("{episode}", &e.episode_number.to_string())
                .replace("{director}", &e.uploader)
                .replace("{genre}", &joined_tags(&e.styles, "genre"))
                .replace("{country}", &joined_tags(&e.areas, "country"))
                .replace("{thumb}", &e.cover)
                .replace("{ep_id}", &e.ep_id.to_string()),
            name: task.File.name.clone(),
            qualifier: Vec::new(),
        });
    }
    list
}

fn tvshow_exists(task: &TaskInfo) -> bool {
    base_dir(task).join("tvshow.nfo").exists()
}

/// Swap the endpoint's host for a test `base` (parsers::list::swap_host).
pub fn swap_host(api: &str, base: &str) -> String {
    if base.is_empty() {
        return api.to_owned();
    }
    let path = api
        .strip_prefix("https://")
        .and_then(|rest| rest.find('/').map(|i| &rest[i..]))
        .unwrap_or(api);
    format!("{base}{path}")
}

fn base_dir(task: &TaskInfo) -> PathBuf {
    PathBuf::from(&task.File.download_path).join(&task.File.folder)
}

/// `_to_json`: the Episode section as a pretty sidecar, python-falsy fields
/// dropped (`{k: v for k, v in asdict(...).items() if v not in
/// [None, "", [], {}, 0]`), insertion order = field order.
pub fn episode_json(task: &TaskInfo) -> String {
    let full: Value =
        serde_json::to_value(&task.Episode).expect("EpisodeInfo is JSON-serializable");
    let mut filtered = Map::new();
    if let Value::Object(map) = full {
        for (k, v) in map {
            if python_truthy(&v) {
                filtered.insert(k, v);
            }
        }
    }
    serde_json::to_string_pretty(&Value::Object(filtered))
        .expect("episode data is JSON-serializable")
}

/// `MetadataParser.parse`: NFO or JSON per config, plus the tag fetch that
/// only video-type items need and the tvshow poster.
pub async fn parse(
    client: &HttpClient,
    base: &str,
    task: &mut TaskInfo,
    store: &ConfigStore,
    labels: &Labels,
) -> anyhow::Result<()> {
    let metadata_type = resolve(task, store, "metadata_type")
        .as_str()
        .unwrap_or("nfo")
        .to_owned();
    match metadata_type.as_str() {
        "nfo" => {
            if task.Episode.attribute & VIDEO_BIT != 0 {
                get_video_tags(
                    client,
                    base,
                    &task.Episode.bvid.clone(),
                    &mut task.Episode.tags,
                )
                .await?;
            }
            let docs = generate(task, &labels.metadata_qualifier);
            for doc in docs {
                // 去除 contents 中多余空行
                let contents = doc
                    .contents
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                writer::write(
                    task,
                    Contents::Text(&contents),
                    "nfo",
                    Some(&doc.name),
                    &doc.qualifier.iter().map(String::as_str).collect::<Vec<_>>(),
                )?;
            }
            save_poster(client, task).await?;
        }
        _ => {
            let contents = episode_json(task);
            let name = task.File.name.clone();
            writer::write(
                task,
                Contents::Text(&contents),
                "json",
                Some(&name),
                &[&labels.metadata_qualifier],
            )?;
        }
    }
    Ok(())
}

async fn get_video_tags(
    client: &HttpClient,
    base: &str,
    bvid: &str,
    tags: &mut Vec<String>,
) -> anyhow::Result<()> {
    let url = format!("{}?bvid={bvid}", swap_host(TAG_API, base));
    let response: Value = client.get_json(&url, &[]).await?;
    *tags = response
        .get("data")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|t| {
                    t.get("tag_name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(())
}

/// `_save_poster`: fetch `poster.jpg` once per folder when a poster URL is
/// set; an existing file is never overwritten.
pub async fn save_poster(client: &HttpClient, task: &mut TaskInfo) -> anyhow::Result<()> {
    let path = base_dir(task).join("poster.jpg");
    if path.exists() || task.Episode.poster.is_empty() {
        return Ok(());
    }
    let bytes = client
        .inner()
        .get(&task.Episode.poster)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();
    writer::write(task, Contents::Bytes(&bytes), "jpg", Some("poster"), &[])?;
    let _ = fs::write(&path, &bytes);
    Ok(())
}
