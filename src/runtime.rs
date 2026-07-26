use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Datelike, Utc};
use serde_json::Value;

use crate::codex::{ThreadStatus, ThreadSummary};
use crate::events::EventRecord;

const TAIL_BYTES: u64 = 4 * 1024 * 1024;
const ACTIVITY_TAIL_BYTES: u64 = 1024 * 1024;
const MAX_ACTIVITY_SESSIONS: usize = 24;
const RECENT_WINDOW: Duration = Duration::from_secs(10 * 60);
const UNCONFIRMED_RUNNING_WINDOW: Duration = Duration::from_secs(5 * 60);
const OPEN_ACTIVITY_WINDOW: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lifecycle {
    Running,
    Complete,
    Unknown,
}

pub fn apply_observed_states(threads: &mut [ThreadSummary]) {
    let Ok(session_root) = crate::paths::codex_home().map(|path| path.join("sessions")) else {
        return;
    };
    let paths = locate_session_files(&session_root, threads);
    let open_paths = open_codex_session_files();
    let now = SystemTime::now();

    for thread in threads {
        if matches!(
            thread.status,
            ThreadStatus::Active { .. } | ThreadStatus::SystemError
        ) {
            continue;
        }
        let Some(path) = paths.get(&thread.id) else {
            continue;
        };
        let age = path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .unwrap_or(Duration::MAX);
        let is_open = open_paths.contains(path);
        if !is_open && age > RECENT_WINDOW {
            continue;
        }
        let lifecycle = read_lifecycle(path).unwrap_or(Lifecycle::Unknown);
        thread.status = match (is_open, lifecycle, age) {
            (true, Lifecycle::Running, _) => ThreadStatus::ObservedRunning,
            (true, Lifecycle::Unknown, age) if age <= OPEN_ACTIVITY_WINDOW => {
                ThreadStatus::ObservedRunning
            }
            (true, _, _) => ThreadStatus::ObservedOpen,
            (false, Lifecycle::Running, age) if age <= UNCONFIRMED_RUNNING_WINDOW => {
                ThreadStatus::ObservedRunning
            }
            (false, _, age) if age <= RECENT_WINDOW => ThreadStatus::RecentlyActive,
            _ => thread.status.clone(),
        };
    }
}

/// Builds a privacy-preserving activity feed from Codex's local rollout files.
///
/// This is a compatibility adapter for Codex surfaces which do not emit hooks.
/// It deliberately retains only event metadata: prompt contents, assistant
/// messages, command arguments and tool outputs never leave the rollout file.
pub fn observed_events(threads: &[ThreadSummary], per_session: usize) -> Vec<EventRecord> {
    let Ok(session_root) = crate::paths::codex_home().map(|path| path.join("sessions")) else {
        return Vec::new();
    };
    let paths = locate_session_files(&session_root, threads);
    let mut events = Vec::new();

    for thread in threads.iter().take(MAX_ACTIVITY_SESSIONS) {
        let Some(path) = paths.get(&thread.id) else {
            continue;
        };
        if let Ok(mut observed) = read_activity(path, thread, per_session) {
            events.append(&mut observed);
        }
    }
    events
}

fn locate_session_files(root: &Path, threads: &[ThreadSummary]) -> HashMap<String, PathBuf> {
    let mut ids_by_directory = HashMap::<PathBuf, HashSet<String>>::new();
    for thread in threads {
        let Some(created) = chrono::DateTime::from_timestamp(thread.created_at, 0) else {
            continue;
        };
        let created = created.with_timezone(&Utc);
        let directory = root
            .join(format!("{:04}", created.year()))
            .join(format!("{:02}", created.month()))
            .join(format!("{:02}", created.day()));
        ids_by_directory
            .entry(directory)
            .or_default()
            .insert(thread.id.clone());
    }

    let mut result = HashMap::new();
    for (directory, ids) in ids_by_directory {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            for id in &ids {
                if name.ends_with(&format!("-{id}.jsonl")) {
                    result.insert(id.clone(), path.clone());
                    break;
                }
            }
        }
    }
    result
}

fn read_lifecycle(path: &Path) -> std::io::Result<Lifecycle> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = String::new();
    file.read_to_string(&mut tail)?;

    for line in tail.lines().rev() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        match value
            .pointer("/payload/type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "task_started" => return Ok(Lifecycle::Running),
            "task_complete" | "turn_aborted" => return Ok(Lifecycle::Complete),
            _ => {}
        }
    }
    Ok(Lifecycle::Unknown)
}

fn read_activity(
    path: &Path,
    thread: &ThreadSummary,
    limit: usize,
) -> std::io::Result<Vec<EventRecord>> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(ACTIVITY_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = String::new();
    file.read_to_string(&mut tail)?;

    let mut events = Vec::new();
    for line in tail.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(timestamp) = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
            .map(|timestamp| timestamp.with_timezone(&Utc))
        else {
            continue;
        };
        let Some((event, tool_name, summary)) = activity_summary(&value) else {
            continue;
        };
        events.push(EventRecord {
            received_at: timestamp,
            session_id: if thread.session_id.is_empty() {
                thread.id.clone()
            } else {
                thread.session_id.clone()
            },
            turn_id: None,
            cwd: thread.cwd.clone(),
            model: None,
            event: event.to_owned(),
            tool_name: tool_name.map(ToOwned::to_owned),
            summary: summary.to_owned(),
            payload: Value::Null,
        });
    }
    if events.len() > limit {
        events.drain(..events.len() - limit);
    }
    Ok(events)
}

fn activity_summary(value: &Value) -> Option<(&'static str, Option<&str>, &'static str)> {
    match value.get("type").and_then(Value::as_str)? {
        "event_msg" => match value.pointer("/payload/type").and_then(Value::as_str)? {
            "task_started" => Some(("TaskStarted", None, "A commencé une nouvelle tâche")),
            "task_complete" => Some(("TaskComplete", None, "A terminé sa tâche")),
            "turn_aborted" => Some(("TurnAborted", None, "La tâche a été interrompue")),
            "patch_apply_end" => Some((
                "PatchApplied",
                Some("apply_patch"),
                "A modifié des fichiers",
            )),
            "context_compacted" => Some((
                "ContextCompacted",
                None,
                "A optimisé son contexte de travail",
            )),
            _ => None,
        },
        "response_item"
            if value.pointer("/payload/type").and_then(Value::as_str)
                == Some("custom_tool_call") =>
        {
            let tool = value.pointer("/payload/name").and_then(Value::as_str)?;
            let summary = match tool {
                "exec" | "exec_command" => "Exécute une commande",
                "apply_patch" => "Modifie des fichiers",
                "imagegen" | "image_gen" => "Crée un élément visuel",
                "web" | "web__run" => "Consulte une source en ligne",
                _ if tool.starts_with("mcp__") => "Consulte un service connecté",
                _ => return None,
            };
            Some(("ToolStarted", Some(tool), summary))
        }
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn open_codex_session_files() -> HashSet<PathBuf> {
    let mut result = HashSet::new();
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return result;
    };
    for process in processes.flatten() {
        if !process
            .file_name()
            .to_string_lossy()
            .chars()
            .all(|character| character.is_ascii_digit())
        {
            continue;
        }
        let process_path = process.path();
        let is_codex = std::fs::read_link(process_path.join("exe"))
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_lowercase())
            })
            .is_some_and(|name| name.starts_with("codex"));
        if !is_codex {
            continue;
        }
        let Ok(descriptors) = std::fs::read_dir(process_path.join("fd")) else {
            continue;
        };
        for descriptor in descriptors.flatten() {
            let Ok(path) = std::fs::read_link(descriptor.path()) else {
                continue;
            };
            if path.extension().and_then(|value| value.to_str()) == Some("jsonl")
                && path.components().any(|part| part.as_os_str() == "sessions")
            {
                result.insert(path);
            }
        }
    }
    result
}

#[cfg(not(target_os = "linux"))]
fn open_codex_session_files() -> HashSet<PathBuf> {
    HashSet::new()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn latest_lifecycle_event_wins() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","payload":{{"type":"task_complete"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","payload":{{"type":"task_started"}}}}"#
        )
        .unwrap();
        assert_eq!(read_lifecycle(file.path()).unwrap(), Lifecycle::Running);
    }

    #[test]
    fn activity_adapter_never_copies_sensitive_payloads() {
        let thread = ThreadSummary {
            id: "thread-1".to_owned(),
            session_id: "session-1".to_owned(),
            cwd: "/work/private".to_owned(),
            preview: "secret prompt".to_owned(),
            name: None,
            model_provider: "openai".to_owned(),
            created_at: 0,
            updated_at: 0,
            parent_thread_id: None,
            agent_nickname: None,
            agent_role: None,
            status: ThreadStatus::Idle,
        };
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-07-26T19:00:00Z","type":"response_item","payload":{{"type":"custom_tool_call","name":"exec","arguments":"cat super-secret.txt"}}}}"#
        )
        .unwrap();

        let events = read_activity(file.path(), &thread, 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "Exécute une commande");
        assert_eq!(events[0].payload, Value::Null);
        assert!(
            !serde_json::to_string(&events[0])
                .unwrap()
                .contains("super-secret")
        );
    }
}
