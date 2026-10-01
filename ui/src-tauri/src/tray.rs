//! Tray icon and its menu.

use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::App;

pub struct TrayItems {
    enabled: CheckMenuItem<tauri::Wry>,
    noise: CheckMenuItem<tauri::Wry>,
    status: MenuItem<tauri::Wry>,
}

fn german() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .is_some_and(|v| v.to_lowercase().starts_with("de"))
}

fn tr(de: &'static str, en: &'static str) -> &'static str {
    if german() {
        de
    } else {
        en
    }
}

pub fn build(app: &AppHandle, shared: &Arc<App>) -> tauri::Result<()> {
    let status = MenuItem::with_id(
        app,
        "status",
        tr("Sordino startet…", "Sordino is starting…"),
        false,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(
        app,
        "open",
        tr("Sordino öffnen", "Open Sordino"),
        true,
        None::<&str>,
    )?;
    let enabled = CheckMenuItem::with_id(
        app,
        "enabled",
        tr("Sordino Mic aktiv", "Sordino Mic active"),
        true,
        true,
        None::<&str>,
    )?;
    let noise = CheckMenuItem::with_id(
        app,
        "noise",
        tr("Rauschunterdrückung", "Noise suppression"),
        true,
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        tr("Sordino beenden", "Quit Sordino"),
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &enabled,
            &noise,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    *shared.tray.lock().unwrap() = Some(TrayItems {
        enabled: enabled.clone(),
        noise: noise.clone(),
        status,
    });

    let icon = Image::from_bytes(include_bytes!("../icons/128x128.png"))?;
    TrayIconBuilder::with_id("sordino")
        .icon(icon)
        .tooltip("Sordino")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let Some(shared) = app.try_state::<Arc<App>>() else { return };
            let shared = shared.inner().clone();
            match event.id().as_ref() {
                "open" => crate::show_main_window(app),
                "enabled" => toggle(&shared, |s| serde_json::json!({ "enabled": !s["settings"]["enabled"].as_bool().unwrap_or(true) })),
                "noise" => toggle(&shared, |s| serde_json::json!({ "noise": { "enabled": !s["settings"]["noise"]["enabled"].as_bool().unwrap_or(true) } })),
                "quit" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move { crate::quit_everything(&app, &shared).await });
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

fn toggle(shared: &Arc<App>, patch: impl Fn(&serde_json::Value) -> serde_json::Value) {
    let Some(state) = shared.last.lock().unwrap().clone() else {
        return;
    };
    let patch = patch(&state).to_string();
    let shared = shared.clone();
    tauri::async_runtime::spawn(async move {
        let _ = shared.bus.call("Apply", &(patch,)).await;
    });
}

/// Bring the menu in line with the daemon's state.
pub fn refresh(_app: &AppHandle, shared: &Arc<App>) {
    let state = shared.last.lock().unwrap().clone();
    let guard = shared.tray.lock().unwrap();
    let Some(items) = guard.as_ref() else { return };
    let Some(state) = state else {
        let _ = items
            .status
            .set_text(tr("Sordino läuft nicht", "Sordino is not running"));
        let _ = items.enabled.set_enabled(false);
        let _ = items.noise.set_enabled(false);
        return;
    };
    let _ = items.enabled.set_enabled(true);
    let _ = items.noise.set_enabled(true);
    let _ = items
        .enabled
        .set_checked(state["settings"]["enabled"].as_bool().unwrap_or(true));
    let _ = items.noise.set_checked(
        state["settings"]["noise"]["enabled"]
            .as_bool()
            .unwrap_or(true),
    );
    let text = match state["status"].as_str().unwrap_or("") {
        "running" => tr("Sordino Mic aktiv", "Sordino Mic active"),
        "off" => tr("Pausiert", "Paused"),
        "mic_missing" => tr("Mikrofon getrennt", "Microphone disconnected"),
        "no_pipewire" => tr("Audiosystem nicht erreichbar", "Audio system not reachable"),
        "error" => tr("Fehler, Neustart läuft", "Error, retrying"),
        _ => tr("Startet…", "Starting…"),
    };
    let _ = items.status.set_text(text);
}
