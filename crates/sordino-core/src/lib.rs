//! Sordino core: everything that does not talk to PipeWire or D-Bus.
//!
//! * [`denoise`]   DeepFilterNet 3 wrapper (10 ms hops at 48 kHz)
//! * [`dsp`]       biquad EQ, gate, de-esser, compressor, limiter
//! * [`studio`]    "Studio sound" presets and the chain built from them
//! * [`echo`]      acoustic echo cancellation (WebRTC AEC3, optional feature)
//! * [`pipeline`]  echo -> denoise -> studio, one hop at a time
//! * [`settings`]  persisted user settings (TOML)
//! * [`ipc`]       state types exchanged between daemon, CLI and UI

pub mod denoise;
pub mod dsp;
pub mod echo;
pub mod ipc;
pub mod level;
pub mod pipeline;
pub mod profile;
pub mod settings;
pub mod studio;

/// All processing runs at 48 kHz mono. PipeWire resamples/downmixes at the edges.
pub const SAMPLE_RATE: u32 = 48_000;
/// DeepFilterNet hop size: 10 ms at 48 kHz.
pub const HOP: usize = 480;

/// D-Bus well-known name, object path and interface of the daemon.
pub const DBUS_NAME: &str = "io.github.bxnnyg.Sordino";
pub const DBUS_PATH: &str = "/io/github/bxnnyg/Sordino";
pub const DBUS_IFACE: &str = "io.github.bxnnyg.Sordino1";

/// `node.name` of the virtual microphone created by the daemon.
pub const VIRTUAL_MIC_NAME: &str = "sordino_mic";
/// Name shown to the user in other apps.
pub const VIRTUAL_MIC_DESCRIPTION: &str = "Sordino Mic";
