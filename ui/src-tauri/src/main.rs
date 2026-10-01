//! Sordino desktop app: a thin Tauri shell around the daemon's D-Bus API.
//!
//! All audio logic lives in `sordinod`. This process only shows state, forwards commands, owns the
//! tray icon and the autostart entry.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod bus;
mod tray;

use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::{AppHandle, Manager, State, WindowEvent};

use bus::Bus;

/// Shared app state.
pub struct App {
    pub bus: Bus,
    /// Last state received from the daemon, used by the tray and the close handler.
    pub last: Mutex<Option<Value>>,
    pub tray: Mutex<Option<tray::TrayItems>>,
}

type Shared<'a> = State<'a, Arc<App>>;

#[tauri::command]
async fn get_state(app: Shared<'_>) -> Result<Value, String> {
    let state = app.bus.get_state().await?;
    *app.last.lock().unwrap() = Some(state.clone());
    Ok(state)
}

#[tauri::command]
async fn apply(app: Shared<'_>, patch: String) -> Result<(), String> {
    app.bus.call("Apply", &(patch,)).await
}

#[tauri::command]
async fn set_profile(app: Shared<'_>, card: u32, index: i32) -> Result<(), String> {
    app.bus.call("SetProfile", &(card, index)).await
}

#[tauri::command]
async fn set_monitor(app: Shared<'_>, on: bool) -> Result<(), String> {
    app.bus.call("SetMonitor", &(on,)).await
}

#[tauri::command]
async fn set_ab_original(app: Shared<'_>, on: bool) -> Result<(), String> {
    app.bus.call("SetAbOriginal", &(on,)).await
}

#[tauri::command]
async fn set_watching(app: Shared<'_>, on: bool) -> Result<(), String> {
    app.bus.call("SetWatching", &(on,)).await
}

#[tauri::command]
async fn start_daemon(app: Shared<'_>) -> Result<(), String> {
    app.bus.start_daemon().await
}

#[tauri::command]
fn get_autostart() -> autostart::Status {
    autostart::status()
}

/// Start the window app (hidden, tray only) at login.
#[tauri::command]
async fn set_autostart(app: Shared<'_>, on: bool) -> Result<(), String> {
    if autostart::is_flatpak() {
        app.bus.request_background(on).await
    } else {
        autostart::set_app(on)
    }
}

/// Start the daemon at login, so Sordino Mic exists even if the window was never opened.
#[tauri::command]
fn set_daemon_autostart(on: bool) -> Result<(), String> {
    autostart::set_daemon(on)
}

#[tauri::command]
async fn set_default_device(app: Shared<'_>, kind: String, name: String) -> Result<(), String> {
    app.bus.call("SetDefaultDevice", &(kind, name)).await
}

/// Open the project page in the default browser. The URL is fixed on purpose: the UI cannot
/// make this command open anything else.
#[tauri::command]
fn open_repo() -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg("https://github.com/BxnnyG/sordino")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn quit_app(app: AppHandle, shared: Shared<'_>) -> Result<(), String> {
    quit_everything(&app, &shared).await;
    Ok(())
}

/// Stop the daemon as well, then exit.
pub async fn quit_everything(app: &AppHandle, shared: &Arc<App>) {
    let _ = shared.bus.call("Quit", &()).await;
    app.exit(0);
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    if let Some(shared) = app.try_state::<Arc<App>>() {
        let shared = shared.inner().clone();
        tauri::async_runtime::spawn(async move {
            let _ = shared.bus.call("SetWatching", &(true,)).await;
        });
    }
}

/// Is there a tray that can bring the window back? Without one (e.g. GNOME without the
/// AppIndicator extension) hiding the window would make Sordino unreachable.
fn tray_available(shared: &Arc<App>) -> bool {
    tauri::async_runtime::block_on(shared.bus.name_has_owner("org.kde.StatusNotifierWatcher"))
}

fn run_in_background(app: &AppHandle) -> bool {
    app.try_state::<Arc<App>>()
        .and_then(|s| {
            s.last.lock().ok().and_then(|l| {
                l.as_ref()
                    .and_then(|v| v["settings"]["run_in_background"].as_bool())
            })
        })
        .unwrap_or(true)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // WebKitGTK's DMABUF renderer aborts with a Wayland protocol error on several GPU/driver
    // combinations (notably NVIDIA). The software path is plenty fast for this UI.
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    let hidden = std::env::args().any(|a| a == "--hidden");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
        .setup(move |app| {
            let bus = tauri::async_runtime::block_on(Bus::connect())
                .map_err(|e| format!("cannot reach the session bus: {e}"))?;
            let shared = Arc::new(App {
                bus,
                last: Mutex::new(None),
                tray: Mutex::new(None),
            });
            app.manage(shared.clone());
            // The tray is a convenience: if it cannot be built, Sordino still works without it.
            if let Err(e) = tray::build(app.handle(), &shared) {
                log::warn!("no tray icon: {e}");
            }
            bus::spawn_watcher(app.handle().clone(), shared.clone());
            // Start hidden (autostart) only if a tray can bring the window back.
            if hidden && tray_available(&shared) {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                let can_hide = app
                    .try_state::<Arc<App>>()
                    .is_some_and(|s| tray_available(s.inner()));
                if run_in_background(&app) && can_hide {
                    api.prevent_close();
                    let _ = window.hide();
                    if let Some(shared) = app.try_state::<Arc<App>>() {
                        let shared = shared.inner().clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = shared.bus.call("SetMonitor", &(false,)).await;
                            let _ = shared.bus.call("SetWatching", &(false,)).await;
                        });
                    }
                } else if !run_in_background(&app) {
                    // The user asked for "stop when closed": stop the daemon as well.
                    api.prevent_close();
                    if let Some(shared) = app.try_state::<Arc<App>>() {
                        let shared = shared.inner().clone();
                        tauri::async_runtime::spawn(
                            async move { quit_everything(&app, &shared).await },
                        );
                    }
                } else if let Some(shared) = app.try_state::<Arc<App>>() {
                    // Background mode but no tray to come back from: close the window normally.
                    // The daemon keeps running as a service, the app can be started again.
                    let shared = shared.inner().clone();
                    tauri::async_runtime::block_on(async move {
                        let _ = shared.bus.call("SetMonitor", &(false,)).await;
                        let _ = shared.bus.call("SetWatching", &(false,)).await;
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            apply,
            set_profile,
            set_monitor,
            set_ab_original,
            set_watching,
            start_daemon,
            get_autostart,
            set_autostart,
            set_daemon_autostart,
            set_default_device,
            open_repo,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sordino");
}
