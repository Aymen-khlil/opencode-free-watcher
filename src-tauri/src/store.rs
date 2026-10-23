use serde::{Deserialize, Serialize};

use crate::monitor::FreeModel;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub interval_minutes: u64,
    pub notify_on_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            interval_minutes: 60,
            notify_on_update: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub ts: u64,
    pub kind: String, // "added" | "removed" | "updated" | "error" | "baseline"
    pub message: String,
    pub model_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub settings: Settings,
    pub last_checked: u64,
    pub models: Vec<FreeModel>,
    pub history: Vec<HistoryEvent>,
}

impl State {
    pub fn push_history(&mut self, kind: &str, message: String, model_ids: Vec<String>) {
        self.history.insert(
            0,
            HistoryEvent {
                ts: now_secs(),
                kind: kind.to_string(),
                message,
                model_ids,
            },
        );
        if self.history.len() > 200 {
            self.history.truncate(200);
        }
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct Store;

impl Store {
    pub fn file_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
        tauri::Manager::path(app)
            .app_data_dir()
            .ok()
            .map(|dir| dir.join("state.json"))
    }

    pub fn load(app: &tauri::AppHandle) -> State {
        let Some(path) = Self::file_path(app) else {
            return State::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                eprintln!("[watcher] corrupt state.json, starting fresh: {e}");
                State::default()
            }),
            Err(_) => State::default(),
        }
    }

    pub fn save(app: &tauri::AppHandle, state: &State) {
        let Some(path) = Self::file_path(app) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = serde_json::to_string_pretty(state).unwrap_or_else(|_| "{}".into());
        if let Err(e) = std::fs::write(&path, text) {
            eprintln!("[watcher] failed to save state: {e}");
        }
    }
}
