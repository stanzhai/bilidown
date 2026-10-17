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
    // Cache reads finish before the first await so the sqlite handle never
    // rides across a yield — the pipeline future must stay `Send`.
    if let Some(bytes) = covers.query_cover(&id)? {
        return Ok(bytes);
    }
    let bytes = fetch_cover_bytes(client, cover_url, suffix).await?;
    covers.add_cover(&id, &bytes)?;
    Ok(bytes)
}

/// Network half: the decorated `@.{suffix}` URL.
async fn fetch_cover_bytes(
    client: &HttpClient,
    cover_url: &str,
    suffix: &str,
) -> anyhow::Result<Vec<u8>> {
    Ok(client
        .inner()
        .get(format!("{cover_url}@.{suffix}"))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}

/// `CoverParser.parse`: write the cover image next to the video.
pub async fn parse(
    client: &HttpClient,
    cover_db: &std::path::Path,
    task: &mut TaskInfo,
    store: &ConfigStore,
) -> anyhow::Result<()> {
    let suffix = resolve(task, store, "cover_type")
        .as_str()
        .unwrap_or("jpg")
        .to_owned();
    // The sqlite handle opens and closes inside this block; the network
    // retry below never holds it across an await (pipeline futures must
    // be `Send`).
    let mut contents = None;
    {
        let covers = CoverDatabase::open(cover_db)?;
        let id = format!("{}@orig.{}", calc_cover_id(&task.Episode.cover), suffix);
        if let Some(bytes) = covers.query_cover(&id)? {
            contents = Some(bytes);
        }
    }
    let contents = match contents {
        Some(bytes) => bytes,
        None => {
            let mut fetched = None;
            // 最多重试 2 次网络失败，3 次都失败则放弃
            for _ in 0..3 {
                match fetch_cover_bytes(client, &task.Episode.cover, &suffix).await {
                    Ok(bytes) => {
                        fetched = Some(bytes);
                        break;
                    }
                    Err(e) if is_transport_error(&e) => continue,
                    Err(e) => return Err(e),
                }
            }
            let fetched = fetched.expect("transport errors re-raise on the third attempt");
            // Re-open just long enough to store the cache entry.
            CoverDatabase::open(cover_db)?.add_cover(
                &format!("{}@orig.{}", calc_cover_id(&task.Episode.cover), suffix),
                &fetched,
            )?;
            fetched
        }
    };
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
