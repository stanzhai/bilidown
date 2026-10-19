//! App bootstrap (ticket 31): the post-first-frame startup pass Python
//! runs in `bootstrap_startup_tasks`, plus the download driver loop that
//! feeds [`crate::download_pipeline::run_task`].
//!
//! - One process-wide tokio runtime (`RUNTIME`); gpui's `cx.spawn`
//!   executor is not a tokio context and every network/downloader future
//!   in this workspace needs one. Bridge with `RUNTIME.spawn(...)`.
//! - MCP server: gated `Mcp/mcp_enabled`, failure logs and continues
//!   (Python `start_mcp_server` contract).
//! - Download driver: scans the downloading table on an interval,
//!   admissions queued tasks (concurrency from
//!   `Download/download_parallel`), runs each through the full
//!   parse→download→extras→merge pipeline, and fires the
//!   download-complete notification when the active count hits zero.

use std::sync::Arc;
use std::time::Duration;

use config::ConfigStore;
use parking_lot::Mutex;
use tasks::status::download_status;
use tasks::TaskManager;

/// The app's tokio runtime. gpui's `cx.spawn` executor is not a tokio
/// context and every network/downloader future in this workspace needs
/// one; bridge with `RUNTIME.spawn(...)`.
pub static RUNTIME: std::sync::LazyLock<tokio::runtime::Runtime> = std::sync::LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime builds")
});

pub fn runtime() -> &'static tokio::runtime::Runtime {
    &RUNTIME
}

/// Enter [`RUNTIME`] for the remainder of the current async block.
///
/// gpui's `cx.spawn` executor is not a tokio context, so any reqwest
/// future awaited directly in one panics ("no reactor running") the
/// moment it arms its timeout timer. Wrap the async block: the entered
/// guard makes `Handle::current()` resolve on the caller's thread, so
/// timers/spawns inside the block register with our runtime.
///
/// ```ignore
/// cx.spawn(async move |shell, cx| {
///     let _rt = crate::bootstrap::tokio_context();
///     network_call().await
/// })
/// ```
pub fn tokio_context() -> tokio::runtime::EnterGuard<'static> {
    runtime().enter()
}

/// Python `start_mcp_server` (post-first-frame, gated, non-fatal).
pub fn start_mcp_server(config: Arc<Mutex<ConfigStore>>, manager: Arc<TaskManager>) {
    let enabled = config
        .lock()
        .get("MCP", "mcp_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !enabled {
        return;
    }
    runtime().spawn(async move {
        if let Err(error) = mcp::start(config, manager).await {
            // Port-in-use or bad config must never take the app down.
            eprintln!("MCP server start failed: {error:#}");
        }
    });
}

/// Spawn the download driver loop for one manager. Called once from app
/// startup; the task ends when the runtime drops.
pub fn spawn_download_driver(config: Arc<ConfigStore>, manager: Arc<TaskManager>) {
    runtime().spawn(driver_loop(config, manager));
}

async fn driver_loop(config: Arc<ConfigStore>, manager: Arc<TaskManager>) {
    let client = network::HttpClient::new(network::ClientOptions {
        user_agent: config
            .get("Advanced", "user_agent")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| config::DEFAULT_USER_AGENT.to_owned()),
        proxy: network::ProxyConfig::none(),
        cookies_from_persistence: Vec::new(),
        signer: None,
    })
    .expect("driver client builds");
    // Ticket 34: playurl calls in the pipeline are cookie-gated; seed
    // the persisted visitor + login sets.
    let state = config
        .path()
        .parent()
        .map(auth::session::config_path)
        .and_then(|p| auth::AuthState::from_config_file(&p))
        .unwrap_or_default();
    auth::seed_client_cookies(&client, &state);
    let apis = parsers::playurl::PlayurlApis::default();
    let install_root = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_default();

    // Tasks currently owned by a spawned pipeline run: their engine slots
    // exist, so the admission scan must not double-start them.
    let running: Arc<Mutex<std::collections::HashSet<String>>> =
        Arc::new(Mutex::new(std::collections::HashSet::new()));

    loop {
        // Scan → admit → run admitted tasks concurrently; the interval
        // backstops external task creation (MCP tools, another window)
        // and re-adopts rows stranded in an active status with no
        // pipeline owner (create/auto_manage flips QUEUED→DOWNLOADING
        // without spawning anything; app restart does the same to
        // previously running tasks).
        if let Ok(tasks) = manager.query(false, None) {
            let limit = config
                .get("Download", "download_parallel")
                .and_then(|v| v.as_i64())
                .unwrap_or(3)
                .clamp(1, 10) as usize;

            let mut admitted: Vec<database::TaskInfo> = Vec::new();
            {
                let mut running = running.lock();
                let active = running.len();
                for task in tasks {
                    if admitted.len() + active >= limit {
                        break;
                    }
                    let startable = task.Download.status == download_status::QUEUED
                        || (matches!(
                            task.Download.status,
                            download_status::PARSING | download_status::DOWNLOADING
                        ) && !running.contains(&task.Basic.task_id));
                    if startable && running.insert(task.Basic.task_id.clone()) {
                        admitted.push(task);
                    }
                }
            }

            for task in admitted {
                let id = task.Basic.task_id.clone();
                let config = config.clone();
                let manager = manager.clone();
                let client = client.clone();
                let apis = apis.clone();
                let install_root = install_root.clone();
                let running = running.clone();
                tokio::spawn(async move {
                    let outcome = crate::download_pipeline::run_task(
                        &manager,
                        &config,
                        &client,
                        &apis,
                        install_root,
                        task,
                    )
                    .await;
                    running.lock().remove(&id);
                    // All-active-done notification (`list_view.py:140`).
                    let active = manager
                        .query(false, None)
                        .map(|list| {
                            list.iter()
                                .filter(|t| {
                                    !matches!(
                                        t.Download.status,
                                        download_status::COMPLETED
                                            | download_status::PAUSED
                                            | download_status::FAILED
                                            | download_status::FFMPEG_FAILED
                                    )
                                })
                                .count()
                        })
                        .unwrap_or(1);
                    if services::notify::should_notify(&config, active) {
                        let n = services::notify::download_completed(
                            services::i18n::Language::from_config(
                                &config
                                    .get("Interface", "language")
                                    .and_then(|v| v.as_str().map(str::to_owned))
                                    .unwrap_or_else(|| "Auto".into()),
                            )
                            .resolve(),
                        );
                        // ponytail: notification goes to stderr until a
                        // platform notifier lands; the decision logic is
                        // the tested part.
                        eprintln!("[notify] {}: {}", n.title, n.message);
                    }
                    if !outcome.message.is_empty() {
                        eprintln!(
                            "[pipeline] task {} failed ({}): {}",
                            outcome.task_id, outcome.status, outcome.message
                        );
                    }
                });
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
