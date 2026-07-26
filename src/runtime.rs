use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{Datelike, Utc};
use serde_json::Value;

use crate::codex::{ThreadStatus, ThreadSummary};

const TAIL_BYTES: u64 = 4 * 1024 * 1024;
const RECENT_WINDOW: Duration = Duration::from_secs(10 * 60);
const UNCONFIRMED_RUNNING_WINDOW: Duration = Duration::from_secs(5 * 60);

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
            (true, _, _) => ThreadStatus::ObservedOpen,
            (false, Lifecycle::Running, age) if age <= UNCONFIRMED_RUNNING_WINDOW => {
                ThreadStatus::ObservedRunning
            }
            (false, _, age) if age <= RECENT_WINDOW => ThreadStatus::RecentlyActive,
            _ => thread.status.clone(),
        };
    }
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
}
