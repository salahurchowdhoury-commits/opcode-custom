use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::path::PathBuf;
use std::process::{Command, Stdio};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

/// Type of Claude installation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InstallationType {
    System,
    Custom,
}

/// Represents a Claude installation with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeInstallation {
    pub path: String,
    pub version: Option<String>,
    pub source: String,
    pub installation_type: InstallationType,
}

/// Finds the configured or best available Claude binary.
pub fn find_claude_binary(app_handle: &AppHandle) -> Result<String, String> {
    info!("Searching for claude binary...");
    if let Ok(app_data_dir) = app_handle.path().app_data_dir() {
        let db_path = app_data_dir.join("agents.db");
        if db_path.exists() {
            if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                if let Ok(stored_path) = conn.query_row(
                    "SELECT value FROM app_settings WHERE key = 'claude_binary_path'",
                    [],
                    |row| row.get::<_, String>(0),
                ) {
                    let path = PathBuf::from(&stored_path);
                    if path.is_file() {
                        return Ok(stored_path);
                    }
                    warn!("Stored claude path no longer exists: {}", stored_path);
                }
            }
        }
    }

    select_best_installation(discover_claude_installations())
        .map(|installation| installation.path)
        .ok_or_else(|| "Claude Code not found. Please ensure it is installed and available in PATH.".to_string())
}

/// Discovers all available Claude installations.
pub fn discover_claude_installations() -> Vec<ClaudeInstallation> {
    let mut installations = Vec::new();
    if let Some(path) = command_path("claude") {
        installations.push(ClaudeInstallation {
            version: get_claude_version(&path).ok().flatten(),
            path,
            source: "PATH".to_string(),
            installation_type: InstallationType::System,
        });
    }

    let mut candidates = Vec::new();
    if let Ok(home) = std::env::var("USERPROFILE") {
        candidates.push((format!(r"{home}\.local\bin\claude.exe"), "local-bin".to_string()));
        candidates.push((format!(r"{home}\.claude\local\claude.exe"), "claude-local".to_string()));
    }
    for (path, source) in candidates {
        let path_buf = PathBuf::from(&path);
        if path_buf.is_file() && !installations.iter().any(|item| item.path == path) {
            installations.push(ClaudeInstallation {
                version: get_claude_version(&path).ok().flatten(),
                path,
                source,
                installation_type: InstallationType::System,
            });
        }
    }
    installations
}

fn command_path(program: &str) -> Option<String> {
    let output = if cfg!(windows) {
        { let mut c = Command::new("where"); #[cfg(target_os = "windows")] c.creation_flags(0x0800_0000); c }.arg(program).output().ok()?
    } else {
        Command::new("which").arg(program).output().ok()?
    };
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn get_claude_version(path: &str) -> Result<Option<String>, String> {
    let output = { let mut c = Command::new(path); #[cfg(target_os = "windows")] c.creation_flags(0x0800_0000); c }
        .arg("--version")
        .output()
        .map_err(|error| error.to_string())?;
    Ok(if output.status.success() {
        extract_version_from_output(&output.stdout)
    } else {
        None
    })
}

fn extract_version_from_output(stdout: &[u8]) -> Option<String> {
    let value = String::from_utf8_lossy(stdout);
    let bytes = value.as_bytes();
    for start in 0..bytes.len() {
        if bytes[start].is_ascii_digit() {
            let mut end = start;
            let mut dots = 0;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
                if bytes[end] == b'.' { dots += 1; }
                end += 1;
            }
            if dots >= 2 {
                return Some(value[start..end].to_string());
            }
        }
    }
    None
}

fn select_best_installation(installations: Vec<ClaudeInstallation>) -> Option<ClaudeInstallation> {
    installations.into_iter().max_by(|a, b| match (&a.version, &b.version) {
        (Some(left), Some(right)) => compare_versions(left, right),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => source_preference(&b.source).cmp(&source_preference(&a.source)),
    })
}

fn source_preference(source: &str) -> u8 {
    match source {
        "PATH" => 1,
        "local-bin" => 2,
        "claude-local" => 3,
        _ => 10,
    }
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let left_parts = left.split('.').map(|part| part.parse::<u32>().unwrap_or(0));
    let right_parts = right.split('.').map(|part| part.parse::<u32>().unwrap_or(0));
    left_parts
        .zip(right_parts)
        .find_map(|(a, b)| match a.cmp(&b) { Ordering::Equal => None, order => Some(order) })
        .unwrap_or(Ordering::Equal)
}

/// Creates a standard command with inherited environment and proxy settings.
pub fn create_command_with_env(program: &str) -> Command {
    let mut command = Command::new(program);
    for (key, value) in std::env::vars() {
        if matches!(key.as_str(), "PATH" | "HOME" | "USERPROFILE" | "USER" | "SHELL" | "LANG" | "LC_ALL" | "NODE_PATH" | "NVM_DIR" | "NVM_BIN" | "HTTP_PROXY" | "HTTPS_PROXY" | "NO_PROXY" | "ALL_PROXY") || key.starts_with("LC_") {
            command.env(key, value);
        }
    }
    command
}

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:20128";

fn claude_auth_token() -> String {
    if let Ok(value) = std::env::var("ANTHROPIC_AUTH_TOKEN") {
        if !value.trim().is_empty() {
            return value;
        }
    }

    if let Some(home) = dirs::home_dir() {
        let settings_path = home.join(".claude").join("settings.json");
        if let Ok(content) = std::fs::read_to_string(settings_path) {
            if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(value) = settings
                    .get("env")
                    .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
                    .and_then(serde_json::Value::as_str)
                {
                    if !value.trim().is_empty() {
                        return value.to_string();
                    }
                }
            }
        }
    }

    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_default()
}

/// Resolves a Claude project identifier or malformed workspace path to a native path.
///
/// Claude stores project directories using a lossy hyphen encoding. On Windows,
/// `D--ai-1` represents a path on drive D, but the final hyphen may be either a
/// separator, a literal hyphen, or a space. Prefer an existing directory so
/// paths such as `D:\\ai 1` win over the incorrect `D:\\ai\\1` candidate.
pub fn resolve_workspace_path(value: &str) -> String {
    if value.trim().is_empty() {
        return value.to_string();
    }

    let normalized = value.replace('/', "\\");
    if std::path::Path::new(&normalized).is_dir() {
        return normalized;
    }

    #[cfg(windows)]
    {
        let encoded = value.trim();
        let (prefix, remainder) = if encoded.len() >= 3
            && encoded.as_bytes()[0].is_ascii_alphabetic()
            && encoded.as_bytes()[1] == b'-'
            && encoded.as_bytes()[2] == b'-'
        {
            (Some(format!("{}:\\", &encoded[0..1])), &encoded[3..])
        } else {
            (None, encoded.strip_prefix('-').unwrap_or(encoded))
        };

        let tokens: Vec<&str> = remainder.split('-').filter(|part| !part.is_empty()).collect();
        let mut candidates = Vec::new();
        build_encoded_path_candidates(&tokens, 0, String::new(), &mut candidates);

        for candidate in candidates {
            let path = match &prefix {
                Some(drive) => format!("{drive}{candidate}"),
                None => format!("\\{candidate}"),
            };
            if std::path::Path::new(&path).is_dir() {
                return path;
            }
        }

        if let Some(drive) = prefix {
            return format!("{drive}{}", remainder.replace('-', "\\"));
        }
        return format!("\\{}", remainder.replace('-', "\\"));
    }

    #[cfg(not(windows))]
    {
        if value.starts_with('-') {
            return format!("/{}", value.trim_start_matches('-').replace('-', "/"));
        }
        normalized
    }
}

#[cfg(windows)]
fn build_encoded_path_candidates(tokens: &[&str], index: usize, current: String, output: &mut Vec<String>) {
    if index == tokens.len() {
        output.push(current);
        return;
    }

    let token = tokens[index];
    let separator = if current.is_empty() { String::new() } else { "\\".to_string() };
    build_encoded_path_candidates(tokens, index + 1, format!("{current}{separator}{token}"), output);

    if !current.is_empty() {
        build_encoded_path_candidates(tokens, index + 1, format!("{current} {token}"), output);
        build_encoded_path_candidates(tokens, index + 1, format!("{current}-{token}"), output);
    }
}

/// Encodes an absolute workspace path into the directory name Claude uses
/// under `~/.claude/projects`. Claude maps every non-alphanumeric character of
/// the path (drive colon, separators and spaces) to a hyphen, which is lossy:
/// Different paths can collide after this lossy encoding. The UI derives its own
/// project id the same way, so this keeps the launcher and the poller in agreement.
pub fn encode_project_dir_name(path: &str) -> String {
    path.chars()
        .map(|character| if character.is_ascii_alphanumeric() { character } else { '-' })
        .collect()
}

#[derive(Debug, Clone)]
pub struct ClaudeSpawnOptions {
    pub prompt: String,
    pub cwd: String,
    pub model: Option<String>,
    pub session_id: Option<String>,
}

fn mapped_model(model: Option<&str>) -> Option<String> {
    model.map(|value| {
        let lower = value.to_ascii_lowercase();
        if lower == "4s" || lower.contains("claude-sonnet-4") { "sonnet".to_string() } else { value.to_string() }
    })
}

#[cfg(windows)]
fn hide_console(command: &mut TokioCommand) {
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut TokioCommand) {}

/// Spawns Claude with stdin closed and concurrently streams stdout/stderr.
pub async fn spawn_and_stream(app: AppHandle, options: ClaudeSpawnOptions) -> Result<Option<String>, String> {
    let binary = find_claude_binary(&app)?;
    let auth_token = claude_auth_token();
    let mut command = TokioCommand::new(binary);
    command
        .arg("-p").arg(&options.prompt)
        .arg("--output-format").arg("stream-json")
        .arg("--verbose")
        .arg("--dangerously-skip-permissions")
        .current_dir(resolve_workspace_path(&options.cwd))
        .env("ANTHROPIC_BASE_URL", DEFAULT_BASE_URL)
        .env("ANTHROPIC_AUTH_TOKEN", &auth_token)
        .env("ANTHROPIC_API_KEY", &auth_token)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("DISABLE_AUTOUPDATER", "1")
        .env("DISABLE_TELEMETRY", "1")
        .env("DISABLE_ERROR_REPORTING", "1")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(model) = mapped_model(options.model.as_deref()) { command.arg("--model").arg(model); }
    if let Some(session_id) = options.session_id.as_deref() { command.arg("--resume").arg(session_id); }
    hide_console(&mut command);

    let mut child = command.spawn().map_err(|error| format!("Unable to start claude: {error}"))?;
    let stdout = child.stdout.take().ok_or_else(|| "Claude stdout was not captured".to_string())?;
    let stderr = child.stderr.take().ok_or_else(|| "Claude stderr was not captured".to_string())?;

    // Keep the live child in the shared state so Stop and tab cleanup can kill
    // the exact process started by this launcher. The polling waiter below
    // deliberately does not hold this mutex while waiting, allowing cancel to
    // acquire it and terminate the child without a deadlock.
    let process_state = app.state::<crate::commands::claude::ClaudeProcessState>();
    let process_state_arc = process_state.current_process.clone();
    {
        let mut current_process = process_state_arc.lock().await;
        if let Some(mut existing_child) = current_process.take() {
            let _ = existing_child.kill().await;
        }
        *current_process = Some(child);
    }

    let (init_tx, init_rx) = tokio::sync::oneshot::channel::<String>();
    let init_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(init_tx)));
    let session_id_holder = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let stdout_app = app.clone();
    let stdout_init_tx = init_tx.clone();
    let stdout_session_id = session_id_holder.clone();
    let stderr_app = app.clone();
    let stderr_session_id = session_id_holder.clone();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? {
            if let Ok(message) = serde_json::from_str::<serde_json::Value>(&line) {
                if message.get("type").and_then(|value| value.as_str()) == Some("system")
                    && message.get("subtype").and_then(|value| value.as_str()) == Some("init")
                {
                    if let Some(session_id) = message.get("session_id").and_then(|value| value.as_str()) {
                        *stdout_session_id.lock().unwrap() = Some(session_id.to_string());
                        if let Some(sender) = stdout_init_tx.lock().unwrap().take() {
                            let _ = sender.send(session_id.to_string());
                        }
                    }
                }
            }
            if let Some(session_id) = stdout_session_id.lock().unwrap().clone() {
                let _ = stdout_app.emit(&format!("claude-output:{session_id}"), &line);
            }
            let _ = stdout_app.emit("claude-output", &line);
            let _ = stdout_app.emit("claude://event", serde_json::json!({"kind":"stdout","line":line}));
        }
        Ok::<(), String>(())
    });
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? {
            let session_id = stderr_session_id.lock().unwrap().clone();
            if let Some(session_id) = session_id {
                let _ = stderr_app.emit(&format!("claude-error:{session_id}"), &line);
            }
            let _ = stderr_app.emit("claude-error", &line);
            let _ = stderr_app.emit("claude://event", serde_json::json!({"kind":"stderr","line":line}));
        }
        Ok::<(), String>(())
    });

    // Keep waiting and cleanup in the background. The command returns as soon
    // as Claude's init record identifies the session, rather than after the
    // whole prompt has finished.
    let waiter_state = process_state_arc.clone();
    let waiter_app = app.clone();
    tokio::spawn(async move {
        let status = loop {
            let mut current_process = waiter_state.lock().await;
            let status = match current_process.as_mut() {
                Some(child) => child.try_wait(),
                None => return,
            };
            drop(current_process);
            match status {
                Ok(Some(status)) => break status,
                Ok(None) => tokio::time::sleep(tokio::time::Duration::from_millis(50)).await,
                Err(error) => {
                    log::error!("Claude process failed: {error}");
                    return;
                }
            }
        };
        let _ = stdout_task.await;
        let _ = stderr_task.await;
        let code = status.code().unwrap_or(-1);
        let _ = waiter_app.emit("claude://event", serde_json::json!({"kind":"exit","code":code}));
        let mut current_process = waiter_state.lock().await;
        current_process.take();
    });

    // Return the real Claude session id immediately after the init line. If
    // Claude exits before emitting init, the caller still receives None.
    Ok(init_rx.await.ok())
}


