//! Task tools (`src/util/mcp/tools/task.py`): list_tasks,
//! get_task_status, get_login_status.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use tasks::status::download_status as status;

use crate::protocol::{ToolOutput, ToolSchema};
use crate::state::McpState;
use crate::tools;

/// Python `DEFAULT_TASK_LIMIT` / `MAX_TASK_LIMIT`: completed tasks are a
/// long-lived pile, callers almost always want the active + recent ones.
const DEFAULT_TASK_LIMIT: usize = 30;
const MAX_TASK_LIMIT: usize = 200;

fn clamp_limit(value: Option<&Value>) -> usize {
    // Bools are ints in Python but rejected here as in Python's
    // `isinstance(value, bool)` guard.
    match value {
        Some(Value::Number(n)) if n.is_i64() => {
            n.as_i64().unwrap_or(DEFAULT_TASK_LIMIT as i64).clamp(1, MAX_TASK_LIMIT as i64) as usize
        }
        _ => DEFAULT_TASK_LIMIT,
    }
}

/// Unfinished first, each group most-recent first; completed sort by
/// completed_time, unfinished by created_time — the same column the two
/// SQL tables ORDER BY, so the truncated set and its order match the DB.
fn list_sort_key(task: &database::TaskInfo) -> (bool, i64) {
    let completed_time = task.Basic.completed_time;
    if completed_time > 0 {
        (true, -completed_time)
    } else {
        (false, -task.Basic.created_time)
    }
}

/// Output file name (with extension), empty when unknowable. Prefers
/// `relative_files[0]` (the post-merge safe_rename name); while still
/// downloading, falls back to name + an expected extension.
fn output_file_name(task: &database::TaskInfo) -> String {
    if task.Download.status == status::COMPLETED && !task.File.relative_files.is_empty() {
        return task.File.relative_files[0].clone();
    }
    if task.File.name.is_empty() {
        return String::new();
    }
    let ext = [&task.File.merge_file_ext, &task.File.video_file_ext, &task.File.audio_file_ext]
        .into_iter()
        .find(|ext| !ext.is_empty());
    match ext {
        Some(ext) => format!("{}.{}", task.File.name, ext),
        None => task.File.name.clone(),
    }
}

fn status_name(value: i64) -> String {
    let name = match value {
        status::QUEUED => "queued",
        status::PARSING => "parsing",
        status::DOWNLOADING => "downloading",
        status::PAUSED => "paused",
        status::COMPLETED => "completed",
        status::FFMPEG_QUEUED => "ffmpeg_queued",
        status::MERGING => "merging",
        status::CONVERTING => "converting",
        status::ADDITIONAL_PROCESSING => "additional_processing",
        status::FAILED => "failed",
        status::FFMPEG_FAILED => "ffmpeg_failed",
        _ => return format!("unknown({value})"),
    };
    name.to_owned()
}

/// Join + normalize `download_path/folder/name` so a "." folder and the
/// mixed separators never reach the model.
fn normalized_output_path(task: &database::TaskInfo) -> String {
    let name = output_file_name(task);
    if name.is_empty() {
        return String::new();
    }
    let parts: Vec<&str> = [task.File.download_path.as_str(), task.File.folder.as_str(), name.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    parts.join("/")
}

fn task_to_dict(task: &database::TaskInfo, verbose: bool) -> Value {
    let mut data = json!({
        "task_id": task.Basic.task_id,
        "title": task.Basic.show_title,
        "status": status_name(task.Download.status),
        "progress": task.Download.progress,
    });

    if task.Download.total_size > 0 {
        data["total_size"] = json!(task.Download.total_size);
        data["downloaded_size"] = json!(task.Download.downloaded_size);
    }
    if task.Download.speed > 0 {
        data["speed"] = json!(task.Download.speed);
    }

    if verbose {
        data["created_time"] = json!(task.Basic.created_time);
        data["completed_time"] = json!(task.Basic.completed_time);

        let path = normalized_output_path(task);
        if !path.is_empty() {
            data["file_path"] = json!(path);
        }
        if !task.Episode.video_quality.is_empty() {
            data["video_quality"] = json!(task.Episode.video_quality);
        }
        if !task.Episode.audio_quality.is_empty() {
            data["audio_quality"] = json!(task.Episode.audio_quality);
        }
    }

    data
}

fn tool_list_tasks(state: &McpState, arguments: &Map<String, Value>) -> ToolOutput {
    let state_arg = arguments.get("state").and_then(Value::as_str).unwrap_or("downloading");
    if !matches!(state_arg, "downloading" | "completed" | "all") {
        return ToolOutput::error(
            "The 'state' argument must be one of: downloading, completed, all.",
        );
    }

    let limit = clamp_limit(arguments.get("limit"));

    // limit only pushes down to the completed table: that one accumulates
    // for years while the unfinished table is the active working set whose
    // oldest rows are exactly the in-flight downloads.
    let mut tasks = Vec::new();
    let mut total = 0i64;
    let manager = &state.manager;

    if matches!(state_arg, "downloading" | "all") {
        tasks.extend(manager.query(false, None).unwrap_or_default());
        total += manager.count(false).unwrap_or(0);
    }
    if matches!(state_arg, "completed" | "all") {
        tasks.extend(manager.query(true, Some(limit as i64)).unwrap_or_default());
        total += manager.count(true).unwrap_or(0);
    }

    tasks.sort_by_key(list_sort_key);
    tasks.truncate(limit);

    let dicts: Vec<Value> = tasks.iter().map(|t| task_to_dict(t, false)).collect();
    let returned = dicts.len();
    let summary = if total > returned as i64 {
        format!("{returned} of {total} task(s); use a larger 'limit' to see more.")
    } else {
        format!("{total} task(s).")
    };

    ToolOutput::with_structured(
        summary,
        json!({"total": total, "returned": returned, "tasks": dicts}),
    )
}

fn tool_get_task_status(state: &McpState, arguments: &Map<String, Value>) -> ToolOutput {
    let task_id = tools::str_arg(arguments, "task_id").unwrap_or_default().trim();
    if task_id.is_empty() {
        return ToolOutput::error("The 'task_id' argument is required.");
    }

    match state.manager.query_by_id(task_id) {
        Ok(Some(task)) => ToolOutput::with_structured("", task_to_dict(&task, true)),
        _ => ToolOutput::error(format!(
            "No task found with id '{task_id}'. Use list_tasks to see current tasks."
        )),
    }
}

fn tool_get_login_status(state: &McpState, _arguments: &Map<String, Value>) -> ToolOutput {
    let auth = tools::auth_state(state);
    // The headless server has no GUI runtime auth object; uname is not
    // persisted separately from the cookie jar.
    let info = json!({
        "logged_in": auth.is_login,
        "username": "",
        "uid": auth.dede_user_id,
        "session_expired": false,
    });

    let summary = if !auth.is_login {
        "Not logged in. Members-only content and high quality streams are unavailable.".to_owned()
    } else {
        "Logged in.".to_owned()
    };

    ToolOutput::with_structured(summary, info)
}

pub fn task_schemas() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "list_tasks",
            title: "List Download Tasks",
            description: concat!(
                "List download tasks with their current status and progress. Covers every task ",
                "in the application, including ones the user created. Unfinished tasks come ",
                "first, then the most recently created; the result is truncated to 'limit'."
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "state": {
                        "type": "string",
                        "enum": ["downloading", "completed", "all"],
                        "description": "Which tasks to list. Defaults to 'downloading'.",
                    },
                    "limit": {
                        "type": "integer",
                        "description": concat!(
                            "Maximum number of tasks to return (1-200",
                            ", default 30). The response reports the true total."
                        ),
                        "minimum": 1,
                        "maximum": MAX_TASK_LIMIT,
                    },
                },
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "get_task_status",
            title: "Get Download Task Status",
            description: "Get detailed status for a single download task, including its output file path.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "The task_id returned by list_tasks or create_download.",
                    },
                },
                "required": ["task_id"],
                "additionalProperties": false,
            }),
        },
        ToolSchema {
            name: "get_login_status",
            title: "Get Login Status",
            description: concat!(
                "Check whether the user is signed in to Bilibili. Members-only content and the ",
                "highest quality streams require a valid login."
            ),
            input_schema: json!({"type": "object", "additionalProperties": false}),
        },
    ]
}

pub fn dispatch(state: Arc<McpState>, name: &str, arguments: &Map<String, Value>) -> ToolOutput {
    match name {
        "list_tasks" => tool_list_tasks(&state, arguments),
        "get_task_status" => tool_get_task_status(&state, arguments),
        "get_login_status" => tool_get_login_status(&state, arguments),
        _ => ToolOutput::error(format!("Unknown task tool: {name}")),
    }
}
