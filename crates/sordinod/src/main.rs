//! sordinod: owns the "Sordino Mic" PipeWire source and the D-Bus API.

mod audio;
mod dbus;
mod devices;
mod engine;
mod rt;

use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use pipewire as pw;
use pw::loop_::Signal;

use engine::{Cmd, Engine, Event, Shared};

struct Args {
    persist: bool,
    mic: Option<String>,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        persist: true,
        mic: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--mic" => args.mic = Some(it.next().context("--mic needs a node name")?),
            "--no-persist" => args.persist = false,
            "-h" | "--help" => {
                println!(
                    "sordinod {}\n\nUsage: sordinod [--mic NODE_NAME] [--no-persist]\n\n  --mic NODE_NAME  use this microphone (node.name) instead of the saved choice\n  --no-persist     do not read or write config files (testing)\n\nSet RUST_LOG=debug for more output. Control the daemon with sordinoctl or the Sordino app.",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!(
                    "sordinod {} - Sordino by BxnnyG, https://github.com/BxnnyG/sordino",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument {other:?} (try --help)"),
        }
    }
    Ok(args)
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(
        "info,tract_core=warn,tract_onnx=warn,tract_hir=warn,tract_linalg=warn,df=warn",
    ))
    .init();
    let args = parse_args()?;

    let shared = Arc::new(Shared {
        state_json: std::sync::Mutex::new(String::from("{}")),
        worker: std::sync::Mutex::new(None),
    });
    let (event_tx, event_rx) = mpsc::channel::<Event>();
    let (cmd_tx, cmd_rx) = pw::channel::channel::<Cmd>();

    // Claim the bus name first: it doubles as the single-instance lock.
    let (conn, watching) = dbus::serve(cmd_tx.clone(), shared.clone())?;
    log::info!(
        "sordinod {} running on the session bus",
        env!("CARGO_PKG_VERSION")
    );

    // pipewire-rs requires every PipeWire call on the thread that called `pw::init()`: the main
    // thread. D-Bus signal emission therefore runs on a helper thread.
    let pump_shared = shared.clone();
    let pump = std::thread::Builder::new()
        .name("sordino-dbus".into())
        .spawn(move || {
            dbus::pump(&conn, &pump_shared, &watching, event_rx);
        })?;

    let stopped_tx = event_tx.clone();
    let result = run_pipewire(cmd_tx, cmd_rx, event_tx, shared, args);
    let _ = stopped_tx.send(Event::Stopped);
    let _ = pump.join();
    result
}

fn run_pipewire(
    cmd_tx: pw::channel::Sender<Cmd>,
    cmd_rx: pw::channel::Receiver<Cmd>,
    events: mpsc::Sender<Event>,
    shared: Arc<Shared>,
    args: Args,
) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let engine = Engine::new(
        &mainloop,
        cmd_tx.clone(),
        events,
        shared,
        args.persist,
        args.mic,
    )?;

    let e = engine.clone();
    let _rx = cmd_rx.attach(mainloop.loop_(), move |cmd| {
        if let Ok(mut e) = e.try_borrow_mut() {
            e.handle(cmd);
        }
    });

    let quit = |tx: pw::channel::Sender<Cmd>| {
        move || {
            let _ = tx.send(Cmd::Quit);
        }
    };
    let _sigint = mainloop
        .loop_()
        .add_signal_local(Signal::INT, quit(cmd_tx.clone()));
    let _sigterm = mainloop
        .loop_()
        .add_signal_local(Signal::TERM, quit(cmd_tx));

    let e = engine.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        if let Ok(mut e) = e.try_borrow_mut() {
            e.tick();
        }
    });
    timer.update_timer(
        Some(Duration::from_millis(10)),
        Some(Duration::from_millis(100)),
    );

    mainloop.run();
    if let Ok(mut e) = engine.try_borrow_mut() {
        e.shutdown();
    }
    Ok(())
}
