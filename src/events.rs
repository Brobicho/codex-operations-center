use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths;
use crate::{codex::ThreadSummary, runtime};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRecord {
    pub received_at: DateTime<Utc>,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub cwd: String,
    pub model: Option<String>,
    pub event: String,
    pub tool_name: Option<String>,
    pub summary: String,
    pub payload: Value,
}

impl EventRecord {
    pub fn from_hook(payload: Value) -> Result<Self> {
        let object = payload
            .as_object()
            .context("hook payload must be a JSON object")?;
        let string = |name: &str| {
            object
                .get(name)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        };
        let event = string("hook_event_name").context("missing hook_event_name")?;
        let session_id = string("session_id").context("missing session_id")?;
        let cwd = string("cwd").unwrap_or_else(|| "Unknown project".to_owned());
        let tool_name = string("tool_name");
        let summary = human_summary(&event, tool_name.as_deref(), &payload);

        Ok(Self {
            received_at: Utc::now(),
            session_id,
            turn_id: string("turn_id"),
            cwd,
            model: string("model"),
            event,
            tool_name,
            summary,
            payload,
        })
    }
}

pub fn ingest_stdin() -> Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("unable to read hook payload")?;
    if input.trim().is_empty() {
        bail!("empty hook payload");
    }
    let payload: Value = serde_json::from_str(&input).context("invalid hook JSON")?;
    append(&EventRecord::from_hook(payload)?)
}

pub fn append(event: &EventRecord) -> Result<()> {
    let path = paths::events_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("unable to create {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("unable to open {}", path.display()))?;
    file.lock_exclusive()
        .context("unable to lock event store")?;
    let result = writeln!(file, "{}", serde_json::to_string(event)?);
    let _ = FileExt::unlock(&file);
    result.context("unable to append Codex event")
}

pub fn recent(limit: usize) -> Result<Vec<EventRecord>> {
    let path = paths::events_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(&path)?;
    let mut events: Vec<_> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    if events.len() > limit {
        events.drain(..events.len() - limit);
    }
    Ok(events)
}

pub fn recent_for_threads(threads: &[ThreadSummary], limit: usize) -> Result<Vec<EventRecord>> {
    let mut events = recent(limit)?;
    events.extend(runtime::observed_events(threads, 18));
    events.sort_by_key(|event| event.received_at);
    events.dedup_by(|left, right| {
        left.received_at == right.received_at
            && left.session_id == right.session_id
            && left.event == right.event
            && left.tool_name == right.tool_name
    });
    if events.len() > limit {
        events.drain(..events.len() - limit);
    }
    Ok(events)
}

fn human_summary(event: &str, tool: Option<&str>, payload: &Value) -> String {
    match event {
        "SessionStart" => "La session vient de démarrer".to_owned(),
        "SessionEnd" => "La session est terminée".to_owned(),
        "UserPromptSubmit" => "Une nouvelle demande a été reçue".to_owned(),
        "PermissionRequest" => "Attend votre autorisation".to_owned(),
        "SubagentStart" => "Une partie du travail a été déléguée".to_owned(),
        "SubagentStop" => "Un sous-agent vient de rendre son résultat".to_owned(),
        "PreCompact" => "Réorganise le contexte de la conversation".to_owned(),
        "PostCompact" => "Le contexte de la conversation a été réorganisé".to_owned(),
        "Stop" => "A terminé sa réponse".to_owned(),
        "PreToolUse" => tool_start_summary(tool, payload),
        "PostToolUse" => tool_finished_summary(tool, payload),
        other => format!("Événement Codex : {other}"),
    }
}

fn tool_start_summary(tool: Option<&str>, payload: &Value) -> String {
    match tool.unwrap_or("outil") {
        "Bash" => command_summary(payload),
        "apply_patch" | "Edit" | "Write" => "Modifie des fichiers".to_owned(),
        "Agent" | "spawn_agent" => "Délègue une partie du travail".to_owned(),
        "update_plan" => "Met à jour son plan de travail".to_owned(),
        name if name.starts_with("mcp__") => "Consulte un service connecté".to_owned(),
        name => format!("Utilise {name}"),
    }
}

fn tool_finished_summary(tool: Option<&str>, payload: &Value) -> String {
    let failed = payload
        .pointer("/tool_response/is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || payload
            .pointer("/tool_response/exit_code")
            .and_then(Value::as_i64)
            .is_some_and(|code| code != 0);
    if failed {
        "Une action vient d’échouer".to_owned()
    } else {
        match tool.unwrap_or("outil") {
            "Bash" => "La commande est terminée".to_owned(),
            "apply_patch" | "Edit" | "Write" => "Les fichiers ont été modifiés".to_owned(),
            name => format!("{name} a terminé"),
        }
    }
}

fn command_summary(payload: &Value) -> String {
    let command = payload
        .pointer("/tool_input/command")
        .or_else(|| payload.pointer("/tool_input/cmd"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_lowercase();
    if [
        "cargo test",
        "pytest",
        "npm test",
        "pnpm test",
        "php artisan test",
    ]
    .iter()
    .any(|needle| command.contains(needle))
    {
        "Exécute les tests".to_owned()
    } else if command.contains("git diff") || command.contains("git status") {
        "Vérifie les changements Git".to_owned()
    } else if command.contains("git commit") {
        "Enregistre les changements dans Git".to_owned()
    } else if command.contains("rg ") || command.contains("grep ") || command.contains("find ") {
        "Recherche dans le projet".to_owned()
    } else if command.contains("build") || command.contains("compile") {
        "Compile le projet".to_owned()
    } else {
        "Exécute une commande".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn translates_test_command() {
        let payload = json!({
            "session_id": "session-1",
            "cwd": "/work/project",
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": "cargo test --all" }
        });
        let event = EventRecord::from_hook(payload).unwrap();
        assert_eq!(event.summary, "Exécute les tests");
    }

    #[test]
    fn rejects_payload_without_session() {
        let payload = json!({ "hook_event_name": "Stop", "cwd": "/tmp" });
        assert!(EventRecord::from_hook(payload).is_err());
    }
}
