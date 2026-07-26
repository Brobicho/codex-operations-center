use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub id: String,
    pub session_id: String,
    pub cwd: String,
    pub preview: String,
    pub name: Option<String>,
    pub model_provider: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub parent_thread_id: Option<String>,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
    pub status: ThreadStatus,
    #[serde(default)]
    pub runtime: ThreadRuntime,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRuntime {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub activity_started_at: Option<i64>,
    pub last_action: Option<String>,
    pub actions_this_turn: u32,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub last_turn_duration_ms: Option<u64>,
    pub time_to_first_token_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ThreadStatus {
    NotLoaded,
    Idle,
    SystemError,
    Active {
        active_flags: Vec<String>,
    },
    #[serde(skip)]
    RecentlyActive,
    #[serde(skip)]
    ObservedRunning,
    #[serde(skip)]
    ObservedOpen,
    #[serde(skip)]
    NeedsAttention,
}

pub fn list_threads(limit: u32) -> Result<Vec<ThreadSummary>> {
    let mut client = AppServer::start()?;
    let result = client.request(
        2,
        "thread/list",
        json!({
            "limit": limit,
            "sortKey": "recency_at",
            "sortDirection": "desc",
            "modelProviders": []
        }),
    )?;
    let data = result
        .get("data")
        .cloned()
        .context("thread/list response did not include data")?;
    let mut threads: Vec<ThreadSummary> =
        serde_json::from_value(data).context("unable to decode Codex threads")?;
    crate::runtime::apply_observed_states(&mut threads);
    Ok(threads)
}

struct AppServer {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl AppServer {
    fn start() -> Result<Self> {
        let mut child = Command::new("codex")
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("unable to start `codex app-server`")?;
        let input = child.stdin.take().context("app-server stdin unavailable")?;
        let output = child
            .stdout
            .take()
            .context("app-server stdout unavailable")?;
        let mut client = Self {
            child,
            input,
            output: BufReader::new(output),
        };
        client.request(
            0,
            "initialize",
            json!({
                "clientInfo": {
                    "name": "codex_operations_center",
                    "title": "Codex Operations Center",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )?;
        client.notify("initialized", json!({}))?;
        Ok(client)
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        self.send(json!({ "method": method, "id": id, "params": params }))?;
        let mut line = String::new();
        loop {
            line.clear();
            if self.output.read_line(&mut line)? == 0 {
                bail!("codex app-server closed the connection");
            }
            let message: Value = serde_json::from_str(&line)
                .with_context(|| format!("invalid app-server response: {line}"))?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                bail!("app-server {method} failed: {error}");
            }
            return message
                .get("result")
                .cloned()
                .context("app-server response did not include result");
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(json!({ "method": method, "params": params }))
    }

    fn send(&mut self, message: Value) -> Result<()> {
        writeln!(self.input, "{}", serde_json::to_string(&message)?)?;
        self.input.flush()?;
        Ok(())
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
