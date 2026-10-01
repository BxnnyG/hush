//! Starting Sordino at login.
//!
//! Two independent things can start automatically:
//! * the **daemon** (`sordinod`, keeps "Sordino Mic" available for other apps even if the window
//!   was never opened), preferably as a systemd user service, otherwise through an XDG autostart
//!   entry;
//! * the **app** (`sordino --hidden`: tray icon, window stays closed).
//!
//! "At login" is the earliest sensible point: PipeWire and the audio devices belong to the user
//! session. (Starting before login would need `loginctl enable-linger`, which we do not do.)

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

const APP_ENTRY: &str = "io.github.bxnnyg.Sordino.desktop";
const DAEMON_ENTRY: &str = "io.github.bxnnyg.Sordino.Daemon.desktop";
const UNIT: &str = "sordinod.service";

#[derive(Serialize)]
pub struct Status {
    /// The app starts (hidden, in the tray) at login.
    pub app: bool,
    /// The daemon starts at login.
    pub daemon: bool,
    /// Running inside a Flatpak sandbox: only the app can autostart, through the Background portal.
    pub flatpak: bool,
}

pub fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

fn autostart_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart"))
}

fn entry_path(name: &str) -> Option<PathBuf> {
    autostart_dir().map(|d| d.join(name))
}

fn systemctl(args: &[&str]) -> Option<std::process::Output> {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .ok()
}

fn systemd_unit_available() -> bool {
    systemctl(&["cat", UNIT]).is_some_and(|o| o.status.success())
}

fn systemd_enabled() -> bool {
    systemctl(&["is-enabled", UNIT])
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
}

fn write_entry(name: &str, contents: &str) -> Result<(), String> {
    let path = entry_path(name).ok_or("cannot find the autostart folder")?;
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(|e| e.to_string())?;
    std::fs::write(path, contents).map_err(|e| e.to_string())
}

fn remove_entry(name: &str) -> Result<(), String> {
    let Some(path) = entry_path(name) else {
        return Ok(());
    };
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn status() -> Status {
    let flatpak = is_flatpak();
    let daemon = if flatpak {
        false
    } else if systemd_unit_available() {
        systemd_enabled()
    } else {
        entry_path(DAEMON_ENTRY).is_some_and(|p| p.exists())
    };
    Status {
        app: entry_path(APP_ENTRY).is_some_and(|p| p.exists()),
        daemon,
        flatpak,
    }
}

/// Autostart of the window app through an XDG entry (not used inside Flatpak).
pub fn set_app(on: bool) -> Result<(), String> {
    if !on {
        return remove_entry(APP_ENTRY);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    write_entry(
        APP_ENTRY,
        &format!(
            "[Desktop Entry]\nType=Application\nName=Sordino\nComment=Microphone noise suppression\nExec=\"{}\" --hidden\nIcon=io.github.bxnnyg.Sordino\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        ),
    )
}

pub fn set_daemon(on: bool) -> Result<(), String> {
    if is_flatpak() {
        return Err(
            "not available in the Flatpak version; enable \"Start at login\" for the app instead"
                .into(),
        );
    }
    if systemd_unit_available() {
        // No `--now`: the daemon usually runs already, a second instance would just fail.
        let action = if on { "enable" } else { "disable" };
        let out = systemctl(&[action, UNIT]).ok_or("systemctl not found")?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        return Ok(());
    }
    if !on {
        return remove_entry(DAEMON_ENTRY);
    }
    let exe = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("sordinod")));
    let exe = exe
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "sordinod".into());
    write_entry(
        DAEMON_ENTRY,
        &format!("[Desktop Entry]\nType=Application\nName=Sordino daemon\nExec=\"{exe}\"\nIcon=io.github.bxnnyg.Sordino\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n"),
    )
}
