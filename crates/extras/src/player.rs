//! `PlayerInfoParser` (`additional/player.py`): the web player info endpoint
//! shared by subtitle and chapter parsing.

use network::HttpClient;
use serde_json::Value;

use database::TaskInfo;

pub const PLAYER_INFO_API: &str = "https://api.bilibili.com/x/player/wbi/v2";

/// The anti-bot fingerprints `PlayerInfoParser.get_data` pins.
pub fn player_params(task: &TaskInfo) -> Vec<(String, String)> {
    let mut params = vec![
        ("cid".to_owned(), task.Episode.cid.to_string()),
        ("dm_img_list".to_owned(), "[]".to_owned()),
        (
            "dm_img_str".to_owned(),
            "V2ViR0wgMS4wIChPcGVuR0wgRVMgMi4wIENocm9taXVtKQ".to_owned(),
        ),
        (
            "dm_cover_img_str".to_owned(),
            "QU5HTEUgKE5WSURJQSwgTlZJRElBIEdlRm9yY2UgUlRYIDQwNjAgTGFwdG9wIEdQVSAoMHgwMDAwMjhFMCkgRGlyZWN0M0QxMSB2c181XzAgcHNfNV8wLCBEM0QxMSlHb29nbGUgSW5jLiAoTlZJRElBKQ".to_owned(),
        ),
        (
            "dm_img_inter".to_owned(),
            r#"{"ds":[],"wh":[5231,6067,75],"of":[475,950,475]}"#.to_owned(),
        ),
    ];
    // Courses have no bvid; fall back to aid for those.
    if !task.Episode.bvid.is_empty() {
        params.push(("bvid".to_owned(), task.Episode.bvid.clone()));
    } else {
        params.push(("aid".to_owned(), task.Episode.aid.to_string()));
    }
    params
}

/// Fetches the player info payload, returning `response["data"]` (an empty
/// object when absent). Callers pass the signed global client; the wbi
/// signer attached there signs api.bilibili.com automatically.
pub async fn get_data(client: &HttpClient, base: &str, task: &TaskInfo) -> anyhow::Result<Value> {
    let url = format!("{base}/x/player/wbi/v2");
    let body = client.get(&url, &player_params(task)).await?;
    let response: Value = serde_json::from_str(&body)?;
    Ok(response.get("data").cloned().unwrap_or_else(|| Value::Object(Default::default())))
}

