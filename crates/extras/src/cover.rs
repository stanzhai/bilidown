//! `CoverParser` (`additional/cover.py`): cover export at original quality,
//! using the thumbnail database as a persistent fetch cache.

use database::{CoverDatabase, TaskInfo};
use md5::{Digest, Md5};
use network::HttpClient;

use crate::options::resolve;
use crate::writer::{self, Contents};
use config::ConfigStore;

/// `CoverManager._calc_cover_id`: the md5 hex digest of the URL.
fn calc_cover_id(cover_url: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(cover_url.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// `CoverParser._get_cover_contents`: original-quality bytes — the cache
/// first (key `{md5}@orig.{suffix}`), otherwise the `@.{suffix}` decorated
/// URL, whose bytes are cached for next time.
pub async fn get_cover_contents(
    client: &HttpClient,
    covers: &CoverDatabase,
    cover_url: &str,
    suffix: &str,
) -> anyhow::Result<Vec<u8>> {
    let id = format!("{}@orig.{}", calc_cover_id(cover_url), suffix);
    if let Some(bytes) = covers.query_cover(&id)? {
        return Ok(bytes);
    }
    let bytes = client
        .inner()
        .get(format!("{cover_url}@.{suffix}"))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();
    covers.add_cover(&id, &bytes)?;
    Ok(bytes)
}

/// `CoverParser.parse`: write the cover image next to the video.
pub async fn parse(
    client: &HttpClient,
    covers: &CoverDatabase,
    task: &mut TaskInfo,
    store: &ConfigStore,
) -> anyhow::Result<()> {
    let suffix = resolve(task, store, "cover_type")
        .as_str()
        .unwrap_or("jpg")
        .to_owned();
    let mut contents = None;
    // 最多重试 2 次网络失败，3 次都失败则放弃
    for _ in 0..3 {
        match get_cover_contents(client, covers, &task.Episode.cover, &suffix).await {
            Ok(bytes) => {
                contents = Some(bytes);
                break;
            }
            Err(e) if is_transport_error(&e) => continue,
            Err(e) => return Err(e),
        }
    }
    let contents = contents.expect("transport errors re-raise on the third attempt");
    let name = task.File.name.clone();
    writer::write(task, Contents::Bytes(&contents), &suffix, Some(&name), &[])?;
    Ok(())
}

/// Python retries only on `RequestException` (connection/timeout), not on
/// HTTP error statuses — those raise straight through.
fn is_transport_error(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<network::HttpError>(),
        Some(network::HttpError::Transport(_))
    )
}
