//! sordinoctl: talk to the running Sordino daemon.

use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::json;
use sordino_core::ipc::{Device, State, Status};
use sordino_core::{DBUS_IFACE, DBUS_NAME, DBUS_PATH};
use zbus::blocking::{Connection, Proxy};

const HELP: &str = "\
sordinoctl - control the Sordino daemon

USAGE: sordinoctl <command>

  status                      show what Sordino is doing
  devices [--all]             list microphones
  on | off                    switch Sordino Mic on or off
  mic <node.name|auto>        choose the physical microphone (auto = follow system default)
  noise on|off                noise suppression
  noise light|medium|high|max noise suppression strength
  studio off|natural|clear|warm
                              studio sound preset
  default on|off              make Sordino Mic the system default microphone
  default-mic <node.name>     choose the system default microphone ('sordino_mic' for Sordino Mic)
  default-output <node.name>  choose the system default output (speakers / headphones)
  outputs                     list output devices
  fix-profile                 switch a mic stuck on 'pro-audio' to a call-friendly profile
  profile <card> <index>      switch a device profile by hand
  monitor on|off              hear yourself (use headphones!); 'on' runs until Ctrl+C
  ab original|processed       while monitoring: A/B compare
  restore-default             give the previous default microphone back
  watch                       print live levels (Ctrl+C to stop)
  diag                        audio glitch counters (all zero = clean)
  state                       raw state as JSON
  set '<json>'                merge a JSON settings patch
  quit                        stop the daemon
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sordinoctl: {e:#}");
            ExitCode::FAILURE
        }
    }
}

struct Client {
    proxy: Proxy<'static>,
}

impl Client {
    fn connect() -> Result<Client> {
        let conn = Connection::session().context("cannot reach the session bus")?;
        let proxy =
            Proxy::new(&conn, DBUS_NAME, DBUS_PATH, DBUS_IFACE).context("cannot create proxy")?;
        Ok(Client { proxy })
    }

    fn state(&self) -> Result<State> {
        let json: String = self.proxy.call("GetState", &()).map_err(|e| {
            anyhow!("the Sordino daemon is not running (start it with `sordinod`): {e}")
        })?;
        serde_json::from_str(&json).context("daemon sent an unreadable state")
    }

    fn apply(&self, patch: serde_json::Value) -> Result<()> {
        self.proxy
            .call::<_, _, ()>("Apply", &(patch.to_string(),))
            .map_err(|e| anyhow!("{e}"))
    }
}

fn device_line(d: &Device, active: Option<&str>, selected: Option<&str>) -> String {
    let mark = if active == Some(d.id.as_str()) {
        "*"
    } else {
        " "
    };
    let sel = if selected == Some(d.id.as_str()) {
        " (selected)"
    } else {
        ""
    };
    let profile = d
        .profile
        .as_ref()
        .map(|p| format!("  [profile {}: {}]", p.index, p.name))
        .unwrap_or_default();
    format!("{mark} {}{sel}\n    id: {}{profile}", d.name, d.id)
}

fn on_off(arg: Option<&String>) -> Result<bool> {
    match arg.map(String::as_str) {
        Some("on") => Ok(true),
        Some("off") => Ok(false),
        _ => bail!("expected 'on' or 'off'"),
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("status");
    if matches!(cmd, "-V" | "--version") {
        println!(
            "sordinoctl {} - Sordino by BxnnyG, https://github.com/BxnnyG/sordino",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if matches!(cmd, "-h" | "--help" | "help") {
        print!("{HELP}");
        return Ok(());
    }
    let c = Client::connect()?;
    match cmd {
        "status" => {
            let s = c.state()?;
            let text = match s.status {
                Status::Off => "off",
                Status::Starting => "starting",
                Status::Running => "running",
                Status::MicMissing => "waiting for the microphone",
                Status::NoPipewire => "PipeWire not reachable",
                Status::Error => "error",
            };
            println!("Sordino {}: {text}", s.version);
            if let Some(e) = &s.error {
                println!("  detail: {e}");
            }
            if let Some(m) = &s.active_mic {
                println!("  microphone: {m}");
            }
            println!(
                "  noise suppression: {} ({:?})",
                if s.settings.noise.enabled {
                    "on"
                } else {
                    "off"
                },
                s.settings.noise.strength
            );
            println!("  studio sound: {:?}", s.settings.studio.preset);
            if let Some(l) = s.latency_ms {
                println!("  added latency: ~{l:.0} ms");
            }
            println!(
                "  system default microphone is Sordino Mic: {}",
                if s.default_is_sordino { "yes" } else { "no" }
            );
            if let Some(h) = &s.profile_hint {
                println!(
                    "  hint: {} is on profile '{}'. Run `sordinoctl fix-profile` to switch to '{}'.",
                    h.device_name, h.current.name, h.suggested.name
                );
            }
        }
        "diag" => {
            let s = c.state()?;
            let d = s.diag;
            let secs = d.out_callbacks as f64 * d.quantum as f64 / 48000.0;
            println!(
                "graph quantum:      {} samples ({:.1} ms)",
                d.quantum,
                d.quantum as f64 * 1000.0 / 48000.0
            );
            println!("running for:        {:.0} s", secs);
            println!(
                "output underruns:   {}   (crackle: Sordino Mic ran out of audio)",
                d.out_underruns
            );
            println!(
                "output skipped:     {} samples   (audible skips)",
                d.out_skipped
            );
            println!(
                "input dropped:      {} samples   (DSP thread too slow)",
                d.in_dropped
            );
            println!("model errors:       {}", d.model_errors);
            println!(
                "overload:           {} episodes, {} hops ({:.1} s) without noise suppression",
                d.overload_events,
                d.overload_hops,
                d.overload_hops as f64 * 0.01
            );
            println!(
                "dsp thread:         {}",
                match d.dsp_priority {
                    2 => "real-time priority",
                    1 => "high priority",
                    _ => "normal priority (cannot be raised; may crackle under heavy load)",
                }
            );
            // A single underrun can happen when an app connects and the graph reschedules.
            if d.out_underruns <= 2 && d.out_skipped + d.in_dropped + d.model_errors == 0 {
                println!("=> clean");
            } else {
                println!(
                    "=> the audio path has glitched; please attach this output to a bug report"
                );
            }
        }
        "devices" => {
            let s = c.state()?;
            for d in &s.devices {
                println!(
                    "{}",
                    device_line(d, s.active_mic.as_deref(), s.settings.mic.as_deref())
                );
            }
            if args.get(1).is_some_and(|a| a == "--all") {
                println!("\nhidden by default:");
                for d in &s.hidden_devices {
                    println!(
                        "{}",
                        device_line(d, s.active_mic.as_deref(), s.settings.mic.as_deref())
                    );
                }
            }
        }
        "on" => c.apply(json!({"enabled": true}))?,
        "off" => c.apply(json!({"enabled": false}))?,
        "mic" => {
            let m = args
                .get(1)
                .context("usage: sordinoctl mic <node.name|auto>")?;
            c.apply(json!({"mic": if m == "auto" { serde_json::Value::Null } else { json!(m) }}))?;
        }
        "noise" => match args.get(1).map(String::as_str) {
            Some("on") => c.apply(json!({"noise": {"enabled": true}}))?,
            Some("off") => c.apply(json!({"noise": {"enabled": false}}))?,
            Some(l @ ("light" | "medium" | "high" | "max")) => {
                c.apply(json!({"noise": {"enabled": true, "strength": l}}))?
            }
            _ => bail!("usage: sordinoctl noise on|off|light|medium|high|max"),
        },
        "studio" => match args.get(1).map(String::as_str) {
            Some(p @ ("off" | "natural" | "clear" | "warm")) => {
                c.apply(json!({"studio": {"preset": p}}))?
            }
            _ => bail!("usage: sordinoctl studio off|natural|clear|warm"),
        },
        "default" => c.apply(json!({"set_default": on_off(args.get(1))?}))?,
        "default-mic" | "default-output" => {
            let name = args
                .get(1)
                .context("usage: sordinoctl default-mic|default-output <node.name>")?;
            let kind = if cmd == "default-mic" {
                "source"
            } else {
                "sink"
            };
            c.proxy
                .call::<_, _, ()>("SetDefaultDevice", &(kind, name.as_str()))?;
        }
        "outputs" => {
            let s = c.state()?;
            for d in &s.sinks {
                let mark = if s.default_sink.as_deref() == Some(d.id.as_str()) {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {}  [{:?}]\n    id: {}", d.name, d.kind, d.id);
            }
        }
        "fix-profile" => {
            let s = c.state()?;
            let h = s
                .profile_hint
                .context("nothing to fix: the active microphone is already on a good profile")?;
            c.proxy
                .call::<_, _, ()>("SetProfile", &(h.card, h.suggested.index))?;
            println!("Switching {} to '{}'.", h.device_name, h.suggested.name);
        }
        "profile" => {
            let card: u32 = args
                .get(1)
                .context("usage: sordinoctl profile <card> <index>")?
                .parse()?;
            let index: i32 = args
                .get(2)
                .context("usage: sordinoctl profile <card> <index>")?
                .parse()?;
            c.proxy.call::<_, _, ()>("SetProfile", &(card, index))?;
        }
        "monitor" => {
            if on_off(args.get(1))? {
                // Monitoring stops by itself unless renewed, so keep renewing until Ctrl+C.
                println!("Hearing yourself. Press Ctrl+C to stop.");
                loop {
                    c.proxy.call::<_, _, ()>("SetMonitor", &(true,))?;
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            } else {
                c.proxy.call::<_, _, ()>("SetMonitor", &(false,))?
            }
        }
        "ab" => match args.get(1).map(String::as_str) {
            Some("original") => c.proxy.call::<_, _, ()>("SetAbOriginal", &(true,))?,
            Some("processed") => c.proxy.call::<_, _, ()>("SetAbOriginal", &(false,))?,
            _ => bail!("usage: sordinoctl ab original|processed"),
        },
        "restore-default" => c.proxy.call::<_, _, ()>("RestoreDefault", &())?,
        "state" => println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(
                &c.proxy.call::<_, _, String>("GetState", &())?
            )?)?
        ),
        "set" => c.apply(
            serde_json::from_str(args.get(1).context("usage: sordinoctl set '<json>'")?)
                .context("not valid JSON")?,
        )?,
        "quit" => c.proxy.call::<_, _, ()>("Quit", &())?,
        "watch" => {
            c.proxy.call::<_, _, ()>("SetWatching", &(true,))?;
            let signals = c.proxy.receive_signal("Levels")?;
            for msg in signals {
                let (i, o): (f64, f64) = msg.body().deserialize()?;
                println!("in {i:6.1} dBFS   out {o:6.1} dBFS");
            }
        }
        other => bail!("unknown command {other:?}\n\n{HELP}"),
    }
    Ok(())
}
