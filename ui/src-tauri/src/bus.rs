//! D-Bus client for the Sordino daemon.

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use sordino_core::{DBUS_IFACE, DBUS_NAME, DBUS_PATH};
use tauri::{AppHandle, Emitter};
use zbus::zvariant::DynamicType;
use zbus::{Connection, MatchRule, MessageStream};

use crate::App;

pub struct Bus {
    conn: Connection,
}

impl Bus {
    pub async fn connect() -> zbus::Result<Bus> {
        Ok(Bus {
            conn: Connection::session().await?,
        })
    }

    pub async fn call<B: Serialize + DynamicType>(
        &self,
        method: &str,
        body: &B,
    ) -> Result<(), String> {
        self.conn
            .call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_IFACE), method, body)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub async fn get_state(&self) -> Result<Value, String> {
        let msg = self
            .conn
            .call_method(
                Some(DBUS_NAME),
                DBUS_PATH,
                Some(DBUS_IFACE),
                "GetState",
                &(),
            )
            .await
            .map_err(|e| e.to_string())?;
        let json: String = msg.body().deserialize().map_err(|e| e.to_string())?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }

    /// Whether something owns this bus name, e.g. a system tray host.
    pub async fn name_has_owner(&self, name: &str) -> bool {
        match (
            zbus::fdo::DBusProxy::new(&self.conn).await,
            zbus::names::BusName::try_from(name),
        ) {
            (Ok(p), Ok(n)) => p.name_has_owner(n).await.unwrap_or(false),
            _ => false,
        }
    }

    /// Ask the Background portal (Flatpak) to start the app at login, or to stop doing so.
    pub async fn request_background(&self, autostart: bool) -> Result<(), String> {
        use std::collections::HashMap;
        use zbus::zvariant::Value;
        let mut options: HashMap<&str, Value> = HashMap::new();
        options.insert("autostart", Value::from(autostart));
        options.insert("commandline", Value::from(vec!["sordino", "--hidden"]));
        options.insert(
            "reason",
            Value::from("Keep Sordino Mic available in the background"),
        );
        self.conn
            .call_method(
                Some("org.freedesktop.portal.Desktop"),
                "/org/freedesktop/portal/desktop",
                Some("org.freedesktop.portal.Background"),
                "RequestBackground",
                &("", options),
            )
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn daemon_running(&self) -> bool {
        match zbus::fdo::DBusProxy::new(&self.conn).await {
            Ok(p) => p
                .name_has_owner(DBUS_NAME.try_into().expect("valid bus name"))
                .await
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Start the daemon: through D-Bus activation if installed, otherwise run the binary next to
    /// this executable or from PATH.
    pub async fn start_daemon(&self) -> Result<(), String> {
        if self.daemon_running().await {
            return Ok(());
        }
        if let Ok(p) = zbus::fdo::DBusProxy::new(&self.conn).await {
            if p.start_service_by_name(DBUS_NAME.try_into().expect("valid bus name"), 0)
                .await
                .is_ok()
            {
                return wait_for_daemon(self).await;
            }
        }
        let beside = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("sordinod")));
        let candidates = beside
            .into_iter()
            .filter(|p| p.exists())
            .chain(std::iter::once("sordinod".into()));
        let mut last_err = String::from("sordinod not found");
        for exe in candidates {
            let mut cmd = Command::new(&exe);
            cmd.stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // Own process group so the daemon outlives the app window.
            std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
            match cmd.spawn() {
                Ok(_) => return wait_for_daemon(self).await,
                Err(e) => last_err = format!("{}: {e}", exe.display()),
            }
        }
        Err(last_err)
    }
}

async fn wait_for_daemon(bus: &Bus) -> Result<(), String> {
    for _ in 0..40 {
        if bus.daemon_running().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("the Sordino daemon did not start".into())
}

/// Forward daemon signals and daemon presence to the frontend and the tray.
pub fn spawn_watcher(app: AppHandle, shared: Arc<App>) {
    tauri::async_runtime::spawn(async move {
        // Make sure a daemon exists; failures show up in the UI as "Sordino is not running".
        if !shared.bus.daemon_running().await {
            if let Err(e) = shared.bus.start_daemon().await {
                log::warn!("could not start the daemon: {e}");
            }
        }
        loop {
            if let Err(e) = watch(&app, &shared).await {
                log::warn!("bus watcher stopped: {e}");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

async fn push_state(app: &AppHandle, shared: &Arc<App>, state: Value) {
    *shared.last.lock().unwrap() = Some(state.clone());
    crate::tray::refresh(app, shared);
    let _ = app.emit("sordino://state", state);
}

async fn watch(app: &AppHandle, shared: &Arc<App>) -> zbus::Result<()> {
    let conn = shared.bus.conn.clone();
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(DBUS_NAME)?
        .interface(DBUS_IFACE)?
        .path(DBUS_PATH)?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &conn, Some(64)).await?;
    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let mut owners = dbus.receive_name_owner_changed().await?;

    // Initial state, if the daemon is already up.
    let up = shared.bus.daemon_running().await;
    let _ = app.emit("sordino://daemon", up);
    if up {
        if let Ok(state) = shared.bus.get_state().await {
            push_state(app, shared, state).await;
        }
    }

    loop {
        tokio::select! {
            Some(msg) = signals.next() => {
                let Ok(msg) = msg else { continue };
                let header = msg.header();
                match header.member().map(|m| m.as_str()) {
                    Some("StateChanged") => {
                        if let Ok(json) = msg.body().deserialize::<String>() {
                            if let Ok(state) = serde_json::from_str::<Value>(&json) {
                                push_state(app, shared, state).await;
                            }
                        }
                    }
                    Some("Levels") => {
                        if let Ok((i, o)) = msg.body().deserialize::<(f64, f64)>() {
                            let _ = app.emit("sordino://levels", serde_json::json!({ "input_db": i, "output_db": o }));
                        }
                    }
                    _ => {}
                }
            }
            Some(change) = owners.next() => {
                let Ok(args) = change.args() else { continue };
                if args.name().as_str() != DBUS_NAME {
                    continue;
                }
                let up = args.new_owner().is_some();
                let _ = app.emit("sordino://daemon", up);
                if up {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    if let Ok(state) = shared.bus.get_state().await {
                        push_state(app, shared, state).await;
                    }
                } else {
                    *shared.last.lock().unwrap() = None;
                    crate::tray::refresh(app, shared);
                }
            }
            else => return Ok(()),
        }
    }
}
