use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::paths;

const EVENTS: &[&str] = &[
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "Stop",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallManifest {
    hooks_path: String,
    hook_command: String,
}

pub fn install() -> Result<()> {
    let executable = std::env::current_exe().context("unable to locate codex-ops")?;
    let command = format!("{} emit", shell_quote(&executable.to_string_lossy()));
    let path = paths::hooks_path()?;
    install_into(&path, &command)?;

    let manifest_path = paths::manifest_path()?;
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(
        &manifest_path,
        &serde_json::to_vec_pretty(&InstallManifest {
            hooks_path: path.display().to_string(),
            hook_command: command,
        })?,
    )?;

    println!("Codex integration installed in {}", path.display());
    println!("Open `/hooks` in Codex once to review and trust the integration.");
    Ok(())
}

pub fn uninstall(purge: bool) -> Result<()> {
    let manifest_path = paths::manifest_path()?;
    if manifest_path.exists() {
        let manifest: InstallManifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        remove_from(Path::new(&manifest.hooks_path), &manifest.hook_command)?;
        fs::remove_file(&manifest_path)?;
        println!("Codex integration removed.");
    } else {
        println!("No Codex Operations Center integration was registered.");
    }

    if purge {
        let data = paths::data_dir()?;
        if data.exists() {
            fs::remove_dir_all(&data)
                .with_context(|| format!("unable to remove {}", data.display()))?;
            println!("Local Codex Operations Center data removed.");
        }
    }
    Ok(())
}

fn install_into(path: &Path, command: &str) -> Result<()> {
    let mut root = load_object(path)?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .context("the existing `hooks` value is not an object")?;

    for event in EVENTS {
        let groups = hooks
            .entry((*event).to_owned())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .with_context(|| format!("existing {event} hooks are not an array"))?;
        let already_present = groups.iter().any(|group| group_contains(group, command));
        if !already_present {
            groups.push(json!({
                "hooks": [{
                    "type": "command",
                    "command": command,
                    "timeout": 3
                }]
            }));
        }
    }
    atomic_write(path, &serde_json::to_vec_pretty(&root)?)
}

fn remove_from(path: &Path, command: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut root = load_object(path)?;
    if let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) {
        for groups in hooks.values_mut() {
            if let Some(groups) = groups.as_array_mut() {
                for group in groups.iter_mut() {
                    if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                        handlers.retain(|handler| {
                            handler.get("command").and_then(Value::as_str) != Some(command)
                        });
                    }
                }
                groups.retain(|group| {
                    group
                        .get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(|handlers| !handlers.is_empty())
                });
            }
        }
        hooks.retain(|_, groups| groups.as_array().is_none_or(|groups| !groups.is_empty()));
    }
    atomic_write(path, &serde_json::to_vec_pretty(&root)?)
}

fn group_contains(group: &Value, command: &str) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|handlers| {
            handlers
                .iter()
                .any(|handler| handler.get("command").and_then(Value::as_str) == Some(command))
        })
}

fn load_object(path: &Path) -> Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let bytes = fs::read(path).with_context(|| format!("unable to read {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} does not contain valid JSON", path.display()))?;
    value
        .as_object()
        .cloned()
        .context("hooks configuration must be a JSON object")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("codex-ops.tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_remove_preserves_unrelated_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.json");
        fs::write(
            &path,
            br#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"keep-me"}]}]}}"#,
        )
        .unwrap();

        install_into(&path, "'/tmp/codex-ops' emit").unwrap();
        install_into(&path, "'/tmp/codex-ops' emit").unwrap();
        remove_from(&path, "'/tmp/codex-ops' emit").unwrap();

        let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let stop = value.pointer("/hooks/Stop").unwrap().as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0].pointer("/hooks/0/command").unwrap(), "keep-me");
    }
}
