//! D-Bus front end. State travels as JSON strings: the UI and `sordinoctl` stay decoupled from the
//! internal types and old clients keep working when fields are added.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pipewire as pw;
use sordino_core::ipc::State;
use sordino_core::{DBUS_IFACE, DBUS_NAME, DBUS_PATH};
use zbus::blocking::Connection;
use zbus::fdo;

use crate::engine::{Cmd, Event, Shared};

pub struct Iface {
    cmd: Mutex<pw::channel::Sender<Cmd>>,
    shared: Arc<Shared>,
    watching: Arc<AtomicBool>,
}

impl Iface {
    fn send(&self, cmd: Cmd) -> fdo::Result<()> {
        self.cmd
            .lock()
            .map_err(|_| fdo::Error::Failed("poisoned".into()))?
            .send(cmd)
            .map_err(|_| fdo::Error::Failed("daemon is shutting down".into()))
    }

    fn current_state(&self) -> Option<State> {
        serde_json::from_str(&self.shared.state_json.lock().ok()?).ok()
    }
}

#[zbus::interface(name = "io.github.bxnnyg.Sordino1")]
impl Iface {
    /// The full state as JSON.
    fn get_state(&self) -> String {
        self.shared
            .state_json
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Merge a JSON settings patch, e.g. `{"noise":{"enabled":false}}`.
    fn apply(&self, patch: &str) -> fdo::Result<()> {
        let patch: serde_json::Value = serde_json::from_str(patch)
            .map_err(|e| fdo::Error::InvalidArgs(format!("not JSON: {e}")))?;
        // Validate against the current settings so errors reach the caller instead of the log.
        if let Some(state) = self.current_state() {
            state
                .settings
                .patched(&patch)
                .map_err(|e| fdo::Error::InvalidArgs(format!("{e:#}")))?;
        }
        self.send(Cmd::Apply(patch))
    }

    fn set_profile(&self, card: u32, index: i32) -> fdo::Result<()> {
        self.send(Cmd::SetProfile { card, index })
    }

    /// `kind` is "source" (microphone) or "sink" (output).
    fn set_default_device(&self, kind: &str, name: &str) -> fdo::Result<()> {
        let sink = match kind {
            "source" => false,
            "sink" => true,
            other => {
                return Err(fdo::Error::InvalidArgs(format!(
                    "kind must be 'source' or 'sink', got {other:?}"
                )))
            }
        };
        self.send(Cmd::SetDefaultDevice {
            sink,
            name: name.to_string(),
        })
    }

    fn set_monitor(&self, on: bool) -> fdo::Result<()> {
        self.send(Cmd::SetMonitor(on))
    }

    fn set_ab_original(&self, on: bool) -> fdo::Result<()> {
        self.send(Cmd::SetAbOriginal(on))
    }

    fn restore_default(&self) -> fdo::Result<()> {
        self.send(Cmd::RestoreDefault)
    }

    /// Clients that show the level meter call this so the daemon only emits `Levels` when needed.
    fn set_watching(&self, on: bool) {
        self.watching.store(on, Ordering::Relaxed);
    }

    fn quit(&self) -> fdo::Result<()> {
        self.send(Cmd::Quit)
    }

    #[zbus(signal)]
    async fn state_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        state: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn levels(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        input_db: f64,
        output_db: f64,
    ) -> zbus::Result<()>;
}

/// Claim the bus name and serve the interface. Fails if another daemon already runs.
pub fn serve(
    cmd: pw::channel::Sender<Cmd>,
    shared: Arc<Shared>,
) -> anyhow::Result<(Connection, Arc<AtomicBool>)> {
    let watching = Arc::new(AtomicBool::new(false));
    let iface = Iface {
        cmd: Mutex::new(cmd),
        shared,
        watching: watching.clone(),
    };
    let conn = zbus::blocking::connection::Builder::session()?
        .name(DBUS_NAME)?
        .serve_at(DBUS_PATH, iface)?
        .build()
        .map_err(|e| {
            anyhow::anyhow!(
                "cannot own {DBUS_NAME} on the session bus (is Sordino already running?): {e}"
            )
        })?;
    Ok((conn, watching))
}

/// Forward engine events as signals until the engine stops.
pub fn pump(conn: &Connection, shared: &Shared, watching: &AtomicBool, events: Receiver<Event>) {
    fn report(member: &str, r: zbus::Result<()>, ok: &mut bool) {
        match r {
            Ok(()) => *ok = true,
            Err(e) => {
                if *ok {
                    log::warn!("cannot emit {member}: {e}");
                }
                *ok = false;
            }
        }
    }
    let mut ok = true;
    loop {
        match events.recv_timeout(Duration::from_millis(66)) {
            Ok(Event::StateChanged) => {
                let json = shared
                    .state_json
                    .lock()
                    .map(|s| s.clone())
                    .unwrap_or_default();
                report(
                    "StateChanged",
                    conn.emit_signal(
                        None::<&str>,
                        DBUS_PATH,
                        DBUS_IFACE,
                        "StateChanged",
                        &(json,),
                    ),
                    &mut ok,
                );
            }
            Ok(Event::Stopped) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                if watching.load(Ordering::Relaxed) {
                    if let Some(w) = shared.worker.lock().ok().and_then(|w| w.clone()) {
                        let (i, o) = w.meter.take();
                        report(
                            "Levels",
                            conn.emit_signal(
                                None::<&str>,
                                DBUS_PATH,
                                DBUS_IFACE,
                                "Levels",
                                &(i as f64, o as f64),
                            ),
                            &mut ok,
                        );
                    }
                }
            }
        }
    }
}
