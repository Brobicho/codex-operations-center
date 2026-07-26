use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Datelike, Utc};
use serde_json::Value;

use crate::codex::{ThreadRuntime, ThreadStatus, ThreadSummary};
use crate::events::EventRecord;

const TAIL_BYTES: u64 = 16 * 1024 * 1024;
const ACTIVITY_TAIL_BYTES: u64 = 256 * 1024;
const MAX_ACTIVITY_SESSIONS: usize = 12;
const RECENT_WINDOW: Duration = Duration::from_secs(10 * 60);
const UNCONFIRMED_RUNNING_WINDOW: Duration = Duration::from_secs(5 * 60);
const OPEN_ACTIVITY_WINDOW: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Lifecycle {
    Running,
    Complete,
    #[default]
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
        let observed = read_runtime(path).unwrap_or_default();
        thread.runtime = observed.metrics;
        if matches!(
            thread.status,
            ThreadStatus::Active { .. } | ThreadStatus::SystemError
        ) {
            continue;
        }
        thread.status = match (is_open, observed.lifecycle, age) {
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

#[derive(Default)]
struct ObservedRuntime {
    lifecycle: Lifecycle,
    metrics: ThreadRuntime,
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

fn read_runtime(path: &Path) -> std::io::Result<ObservedRuntime> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let start = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = String::new();
    file.read_to_string(&mut tail)?;

    let mut observed = ObservedRuntime::default();
    let mut turn_running = false;
    for line in tail.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) == Some("turn_context") {
            observed.metrics.model = value
                .pointer("/payload/model")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            observed.metrics.reasoning_effort = value
                .pointer("/payload/effort")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
        }
        if value.get("type").and_then(Value::as_str) == Some("event_msg") {
            match value
                .pointer("/payload/type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "task_started" => {
                    observed.lifecycle = Lifecycle::Running;
                    turn_running = true;
                    observed.metrics.activity_started_at = value
                        .pointer("/payload/started_at")
                        .and_then(Value::as_i64)
                        .or_else(|| event_timestamp(&value));
                    observed.metrics.actions_this_turn = 0;
                    observed.metrics.last_action = Some("Analyse la demande".to_owned());
                }
                "task_complete" | "turn_aborted" => {
                    observed.lifecycle = Lifecycle::Complete;
                    turn_running = false;
                    observed.metrics.activity_started_at = None;
                    observed.metrics.last_turn_duration_ms = value
                        .pointer("/payload/duration_ms")
                        .and_then(Value::as_u64);
                    observed.metrics.time_to_first_token_ms = value
                        .pointer("/payload/time_to_first_token_ms")
                        .and_then(Value::as_u64);
                    observed.metrics.last_action = Some(
                        if value.pointer("/payload/type").and_then(Value::as_str)
                            == Some("turn_aborted")
                        {
                            "Activité interrompue".to_owned()
                        } else {
                            "Tâche terminée".to_owned()
                        },
                    );
                }
                "token_count" => {
                    observed.metrics.context_tokens = value
                        .pointer("/payload/info/last_token_usage/total_tokens")
                        .and_then(Value::as_u64);
                    observed.metrics.context_window = value
                        .pointer("/payload/info/model_context_window")
                        .and_then(Value::as_u64);
                    observed.metrics.reasoning_tokens = value
                        .pointer("/payload/info/last_token_usage/reasoning_output_tokens")
                        .and_then(Value::as_u64);
                }
                _ => {}
            }
        }
        if turn_running
            && let Some((_, _, summary, _)) = activity_summary(&value)
            && !matches!(
                value.pointer("/payload/type").and_then(Value::as_str),
                Some("task_started" | "task_complete" | "turn_aborted")
            )
        {
            observed.metrics.actions_this_turn =
                observed.metrics.actions_this_turn.saturating_add(1);
            observed.metrics.last_action = Some(summary);
        }
    }
    Ok(observed)
}

fn event_timestamp(value: &Value) -> Option<i64> {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
        .map(|timestamp| timestamp.timestamp())
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
        let Some((event, tool_name, summary, payload)) = activity_summary(&value) else {
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
            event,
            tool_name,
            summary,
            payload,
        });
    }
    if events.len() > limit {
        events.drain(..events.len() - limit);
    }
    Ok(events)
}

fn activity_summary(value: &Value) -> Option<(String, Option<String>, String, Value)> {
    match value.get("type").and_then(Value::as_str)? {
        "event_msg" => match value.pointer("/payload/type").and_then(Value::as_str)? {
            "task_started" => owned_activity("TaskStarted", None, "Commence une nouvelle tâche"),
            "task_complete" => owned_activity("TaskComplete", None, "Termine sa tâche"),
            "turn_aborted" => owned_activity("TurnAborted", None, "Interrompt la tâche"),
            "patch_apply_end" => Some((
                "PatchApplied".to_owned(),
                Some("apply_patch".to_owned()),
                changed_files_summary(value),
                serde_json::json!({ "files": changed_files(value) }),
            )),
            "context_compacted" => Some((
                "ContextCompacted".to_owned(),
                None,
                "Optimise son contexte de travail".to_owned(),
                Value::Null,
            )),
            _ => None,
        },
        "response_item"
            if value.pointer("/payload/type").and_then(Value::as_str)
                == Some("custom_tool_call") =>
        {
            let tool = value.pointer("/payload/name").and_then(Value::as_str)?;
            let command = custom_call_command(value);
            let summary = match tool {
                "exec" | "exec_command" => command
                    .as_deref()
                    .map(summarize_command)
                    .unwrap_or_else(|| "Lance un script shell".to_owned()),
                "apply_patch" => "Prépare une modification de fichiers".to_owned(),
                "imagegen" | "image_gen" => "Crée un élément visuel".to_owned(),
                "web" | "web__run" => "Consulte une source en ligne".to_owned(),
                _ if tool.starts_with("mcp__") => "Consulte un service connecté".to_owned(),
                _ => return None,
            };
            let payload = command
                .map(|command| serde_json::json!({ "command": command }))
                .unwrap_or(Value::Null);
            Some((
                "ToolStarted".to_owned(),
                Some(tool.to_owned()),
                summary,
                payload,
            ))
        }
        _ => None,
    }
}

fn owned_activity(
    event: &str,
    tool: Option<&str>,
    summary: &str,
) -> Option<(String, Option<String>, String, Value)> {
    Some((
        event.to_owned(),
        tool.map(ToOwned::to_owned),
        summary.to_owned(),
        Value::Null,
    ))
}

fn changed_files(value: &Value) -> Vec<String> {
    let mut files = value
        .pointer("/payload/changes")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|changes| changes.keys().cloned())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn changed_files_summary(value: &Value) -> String {
    let mut files = changed_files(value)
        .iter()
        .filter_map(|path| Path::new(path).file_name()?.to_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    let extra = files.len().saturating_sub(3);
    files.truncate(3);
    if files.is_empty() {
        "Modifie des fichiers".to_owned()
    } else if extra == 0 {
        format!("Modifie {}", files.join(", "))
    } else {
        format!("Modifie {} (+{extra})", files.join(", "))
    }
}

fn custom_call_command(value: &Value) -> Option<String> {
    for pointer in ["/payload/input", "/payload/arguments"] {
        let Some(input) = value.pointer(pointer).and_then(Value::as_str) else {
            continue;
        };
        if let Ok(parsed) = serde_json::from_str::<Value>(input)
            && let Some(command) = parsed
                .get("cmd")
                .or_else(|| parsed.get("command"))
                .and_then(Value::as_str)
        {
            return Some(command.to_owned());
        }
        for key in ["cmd", "command"] {
            let needle = format!("\"{key}\":");
            let Some(rest) = input.split_once(&needle).map(|(_, rest)| rest.trim_start()) else {
                continue;
            };
            if let Some(Ok(command)) = serde_json::Deserializer::from_str(rest)
                .into_iter::<String>()
                .next()
            {
                return Some(command);
            }
        }
    }
    None
}

pub(crate) fn summarize_command(command: &str) -> String {
    let first = command
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let lower = first.to_lowercase();
    let safe_prefixes = [
        "cargo ",
        "git status",
        "git diff",
        "git log",
        "git show",
        "npm test",
        "npm run",
        "pnpm test",
        "pnpm run",
        "pytest",
        "php artisan test",
    ];
    if safe_prefixes.iter().any(|prefix| lower.starts_with(prefix)) {
        return format!("Commande · {}", truncate_detail(first, 76));
    }
    let programs = command
        .split(['\n', ';', '|'])
        .flat_map(|part| part.split("&&"))
        .filter_map(|part| {
            let mut words = part.split_whitespace();
            let mut word = words.next()?;
            while matches!(word, "if" | "then" | "do" | "done" | "fi" | "for" | "while") {
                word = words.next()?;
            }
            let word = word.rsplit('/').next().unwrap_or(word);
            word.chars()
                .all(|character| character.is_ascii_alphanumeric() || "_-.$".contains(character))
                .then(|| word.trim_start_matches('$').to_owned())
        })
        .filter(|word| !word.is_empty() && !word.contains('='))
        .take(5)
        .collect::<Vec<_>>();
    if programs.is_empty() {
        "Lance un script shell".to_owned()
    } else {
        format!("Shell · {}", programs.join(" → "))
    }
}

fn truncate_detail(value: &str, width: usize) -> String {
    let flattened = value.replace(['\n', '\r', '\t'], " ");
    if flattened.chars().count() <= width {
        flattened
    } else {
        format!(
            "{}…",
            flattened
                .chars()
                .take(width.saturating_sub(1))
                .collect::<String>()
        )
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
        assert_eq!(
            read_runtime(file.path()).unwrap().lifecycle,
            Lifecycle::Running
        );
    }

    #[test]
    fn runtime_metrics_are_read_from_codex_events() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-07-26T20:00:00Z","type":"turn_context","payload":{{"model":"gpt-5.6-sol","effort":"high"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-07-26T20:00:01Z","type":"event_msg","payload":{{"type":"task_started","started_at":1785096001}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-07-26T20:00:02Z","type":"response_item","payload":{{"type":"custom_tool_call","name":"exec","input":"{{\"cmd\":\"cargo test\"}}"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-07-26T20:00:03Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"total_tokens":120000,"reasoning_output_tokens":840}},"model_context_window":258400}}}}}}"#
        )
        .unwrap();

        let observed = read_runtime(file.path()).unwrap();
        assert_eq!(observed.lifecycle, Lifecycle::Running);
        assert_eq!(observed.metrics.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(observed.metrics.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(observed.metrics.activity_started_at, Some(1785096001));
        assert_eq!(observed.metrics.actions_this_turn, 1);
        assert_eq!(observed.metrics.context_tokens, Some(120000));
        assert_eq!(observed.metrics.context_window, Some(258400));
        assert_eq!(observed.metrics.reasoning_tokens, Some(840));
        assert_eq!(
            observed.metrics.last_action.as_deref(),
            Some("Commande · cargo test")
        );
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
            runtime: ThreadRuntime::default(),
        };
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-07-26T19:00:00Z","type":"response_item","payload":{{"type":"custom_tool_call","name":"exec","arguments":"cat super-secret.txt"}}}}"#
        )
        .unwrap();

        let events = read_activity(file.path(), &thread, 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "Lance un script shell");
        assert_eq!(events[0].payload, Value::Null);
        assert!(
            !serde_json::to_string(&events[0])
                .unwrap()
                .contains("super-secret")
        );
    }

    #[test]
    fn activity_adapter_describes_safe_commands_and_changed_files() {
        let command = serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "custom_tool_call",
                "name": "exec",
                "input": "const r = await tools.exec_command({\"cmd\":\"cargo test --locked\",\"workdir\":\"/work/project\"});"
            }
        });
        let command_activity = activity_summary(&command).unwrap();
        assert_eq!(command_activity.2, "Commande · cargo test --locked");
        assert_eq!(
            command_activity
                .3
                .pointer("/command")
                .and_then(Value::as_str),
            Some("cargo test --locked")
        );

        let patch = serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "patch_apply_end",
                "changes": {
                    "/work/project/src/app.rs": {},
                    "/work/project/src/ui.rs": {}
                }
            }
        });
        let patch_activity = activity_summary(&patch).unwrap();
        assert_eq!(patch_activity.2, "Modifie app.rs, ui.rs");
        assert_eq!(
            patch_activity.3.pointer("/files/0").and_then(Value::as_str),
            Some("/work/project/src/app.rs")
        );
    }
}
