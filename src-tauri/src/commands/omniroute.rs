use reqwest::Client;
use serde::Serialize;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const OMNIROUTE_URL: &str = "http://127.0.0.1:20128";
const OMNIROUTE_HOSTS: [&str; 2] = ["http://127.0.0.1:20128", "http://localhost:20128"];
const OMNIROUTE_COMMAND: &str = "omniroute";

#[derive(Debug, Serialize, Clone)]
pub struct OmniRouteHealth {
    pub healthy: bool,
    pub status: String,
    pub port: u16,
}

async fn health() -> OmniRouteHealth {
    let client = match Client::builder().timeout(Duration::from_secs(2)).build() {
        Ok(client) => client,
        Err(_) => return OmniRouteHealth { healthy: false, status: "Offline".into(), port: 20128 },
    };
    let mut healthy = false;
    for host in OMNIROUTE_HOSTS {
        match client.get(format!("{host}/v1/models")).send().await {
            Ok(response) if response.status().as_u16() < 500 => {
                healthy = true;
                break;
            }
            Ok(_) | Err(_) => {}
        }
    }
    OmniRouteHealth { healthy, status: if healthy { "Live".into() } else { "Offline".into() }, port: 20128 }
}

#[tauri::command]
pub async fn check_omniroute_health() -> OmniRouteHealth { health().await }

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

#[tauri::command]
pub async fn restart_omniroute() -> Result<OmniRouteHealth, String> {
    #[cfg(windows)]
    {
        let query = std::process::Command::new("cmd").args(["/C", "netstat -ano -p tcp"]).output()
            .map_err(|e| format!("Unable to inspect port 20128: {e}"))?;
        let text = String::from_utf8_lossy(&query.stdout);
        let listening = text.lines().any(|line| {
            line.contains("LISTENING") && line.split_whitespace().any(|endpoint| {
                endpoint.ends_with(":20128") || endpoint == "[::]:20128"
            })
        });
        if listening {
            return Ok(health().await);
        }
        let mut command = Command::new("omniroute");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console(&mut command);
        command.spawn().map_err(|e| format!("Unable to launch OmniRoute: {e}"))?;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    Ok(health().await)
}

pub fn stop_omniroute() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let ps_cmd = "Get-CimInstance Win32_Process -Filter \"Name='node.exe'\" | Where-Object { $_.CommandLine -like '*omniroute*' } | ForEach-Object { taskkill /F /T /PID $_.ProcessId 2>$null }";
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", ps_cmd])
            .creation_flags(0x0800_0000)
            .output();
    }
}