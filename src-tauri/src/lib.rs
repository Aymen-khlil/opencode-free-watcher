mod monitor;
mod store;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItemBuilder};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use monitor::DOCS_ANCHOR_URL;
use store::{State, Store};

pub struct AppCtx {
    state: Mutex<State>,
    running: AtomicBool,
}

#[derive(serde::Serialize, Clone)]
pub struct CheckOutcome {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub updated: Vec<String>,
    pub failed: bool,
    pub first_run_baseline: bool,
}

fn names(models: &[monitor::FreeModel]) -> Vec<String> {
    models.iter().map(|m| m.name.clone()).collect()
}

fn trim_names(mut parts: Vec<String>, label: &str) -> Option<String> {
    if parts.is_empty() {
        return None;
    }
    parts.sort_by_key(|a| a.to_lowercase());
    let shown = if parts.len() > 3 {
        format!("{} (+{} more)", parts[..3].join(", "), parts.len() - 3)
    } else {
        parts.join(", ")
    };
    Some(format!("{label}: {shown}"))
}

pub async fn run_check(app: &tauri::AppHandle) -> CheckOutcome {
    let ctx = app.state::<AppCtx>();
    if ctx.running.swap(true, Ordering::SeqCst) {
        return CheckOutcome {
            added: vec![],
            removed: vec![],
            updated: vec![],
            failed: false,
            first_run_baseline: false,
        };
    }
    let result = run_check_inner(app).await;
    ctx.running.store(false, Ordering::SeqCst);
    result
}

async fn run_check_inner(app: &tauri::AppHandle) -> CheckOutcome {
    let ctx = app.state::<AppCtx>();
    let notify_on_update = { ctx.state.lock().map(|s| s.settings.notify_on_update).unwrap_or(true) };

    let outcome = match monitor::fetch_free_models().await {
        Err(err) => {
            {
                let mut st = ctx.state.lock().expect("state lock");
                st.push_history("error", format!("Check failed: {err}"), vec![]);
                st.last_checked = store::now_secs();
                Store::save(app, &st);
            }
            let _ = app.emit("state-changed", ());
            CheckOutcome {
                added: vec![],
                removed: vec![],
                updated: vec![],
                failed: true,
                first_run_baseline: false,
            }
        }
        Ok(live) => {
            let mut notifications: Vec<String> = Vec::new();
            let mut outcome = CheckOutcome {
                added: vec![],
                removed: vec![],
                updated: vec![],
                failed: false,
                first_run_baseline: false,
            };
            {
                let mut st = ctx.state.lock().expect("state lock");
                let was_baseline = st.models.is_empty();
                if was_baseline {
                    st.push_history(
                        "baseline",
                        format!("First baseline saved: {} free models", live.len()),
                        live.iter().map(|m| m.id.clone()).collect(),
                    );
                    st.models = live.clone();
                    st.last_checked = store::now_secs();
                    outcome.first_run_baseline = true;
                } else {
                    let diff = monitor::diff_models(&st.models, &live);
                    let diff = if notify_on_update {
                        diff
                    } else {
                        monitor::DiffResult {
                            added: diff.added,
                            removed: diff.removed,
                            updated: Vec::<monitor::FreeModel>::new(),
                        }
                    };
                    if !diff.added.is_empty() {
                        outcome.added = diff.added.iter().map(|m| m.id.clone()).collect();
                        let ids: Vec<String> = diff.added.iter().map(|m| m.id.clone()).collect();
                        if let Some(msg) = trim_names(names(&diff.added), "New free model available") {
                            st.push_history("added", msg.clone(), ids.clone());
                            notifications.push(msg);
                        }
                    }
                    if !diff.removed.is_empty() {
                        outcome.removed = diff.removed.iter().map(|m| m.id.clone()).collect();
                        let ids: Vec<String> = diff.removed.iter().map(|m| m.id.clone()).collect();
                        if let Some(msg) = trim_names(names(&diff.removed), "No longer free") {
                            st.push_history("removed", msg.clone(), ids.clone());
                            notifications.push(msg);
                        }
                    }
                    if !diff.updated.is_empty() {
                        outcome.updated = diff.updated.iter().map(|m| m.id.clone()).collect();
                        let ids: Vec<String> = diff.updated.iter().map(|m| m.id.clone()).collect();
                        if let Some(msg) = trim_names(names(&diff.updated), "Free-model info changed") {
                            st.push_history("updated", msg.clone(), ids.clone());
                            notifications.push(msg);
                        }
                    }
                    let meaningful = !diff.added.is_empty() || !diff.removed.is_empty() || !diff.updated.is_empty();
                    if meaningful {
                        st.models = live.clone();
                    }
                    st.last_checked = store::now_secs();
                }
                Store::save(app, &st);
            }
            if !notifications.is_empty() {
                let body = notifications.join("\n");
                let _ = app.notification().builder().title("OpenCode Free Models").body(&body).show();
                let _ = app.emit("state-changed", ());
            }
            outcome
        }
    };
    outcome
}

fn toggle_main(app: &tauri::AppHandle) -> Option<()> {
    let win = app.get_webview_window("main")?;
    let visible = win.is_visible().unwrap_or(false);
    if visible {
        let focused = win.is_focused().unwrap_or(false);
        if focused {
            let _ = win.hide();
        } else {
            let _ = win.set_focus();
        }
    } else {
        let _ = win.show();
        let _ = win.set_focus();
    }
    Some(())
}

#[tauri::command]
fn get_state(ctx: tauri::State<'_, AppCtx>) -> State {
    ctx.state.lock().expect("state lock").clone()
}

#[tauri::command(async)]
async fn check_now(app: tauri::AppHandle) -> CheckOutcome {
    run_check(&app).await
}

#[tauri::command]
fn set_settings(ctx: tauri::State<'_, AppCtx>, app: tauri::AppHandle, settings: store::Settings) -> State {
    let state = {
        let mut st = ctx.state.lock().expect("state lock");
        st.settings = settings;
        Store::save(&app, &st);
        st.clone()
    };
    let _ = app.emit("state-changed", ());
    state
}

#[tauri::command]
fn set_start_with_windows(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    let res = if enabled { autostart.enable() } else { autostart.disable() };
    res.map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            get_state,
            check_now,
            set_settings,
            set_start_with_windows
        ])
        .setup(|app| {
            let state = Store::load(&app.handle());
            let first_run = state.models.is_empty();
            app.manage(AppCtx {
                state: Mutex::new(state),
                running: AtomicBool::new(false),
            });

            let show_i = MenuItemBuilder::with_id("show", "Show models").build(app)?;
            let check_i = MenuItemBuilder::with_id("check", "Check now").build(app)?;
            let open_i = MenuItemBuilder::with_id("open", "Open models page").build(app)?;
            let quit_i = MenuItemBuilder::with_id("quit", "Exit").build(app)?;
            let menu = Menu::with_items(app, &[&show_i, &check_i, &open_i, &quit_i])?;

            let handle = app.handle().clone();
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().cloned().expect("window icon"))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("OpenCode Free Models")
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "show" => {
                        toggle_main(app);
                    }
                    "check" => {
                        let h = app.clone();
                        tauri::async_runtime::spawn(async move {
                            run_check(&h).await;
                        });
                    }
                    "open" => {
                        let _ = app.opener().open_url(DOCS_ANCHOR_URL, None::<&str>);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(move |_tray, event| {
                    if let TrayIconEvent::DoubleClick {
                        button: MouseButton::Left,
                        ..
                    } = event
                    {
                        toggle_main(&handle);
                    }
                })
                .build(app)?;

            let handle2 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let outcome = run_check(&handle2).await;
                if outcome.first_run_baseline {
                    toggle_main(&handle2);
                }
            });

            let handle3 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let mins = {
                        let ctx = handle3.state::<AppCtx>();
                        ctx.state
                            .lock()
                            .map(|s| s.settings.interval_minutes.max(5))
                            .unwrap_or(60)
                    };
                    tokio::time::sleep(std::time::Duration::from_secs(mins * 60)).await;
                    run_check(&handle3).await;
                }
            });

            if first_run {
                let _ = app.notification().builder().title("OpenCode Free Models").body("Running in the background. Find the widget in your system tray.").show();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

