//! State types shared by the daemon, `sordinoctl` and the UI. Serialised as JSON over D-Bus.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::profile::ProfileKind;
use crate::settings::Settings;
use crate::studio::{Preset, StudioParams};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Sordino Mic is switched off by the user.
    Off,
    Starting,
    Running,
    /// The selected microphone is not connected; Sordino Mic comes back automatically.
    MicMissing,
    /// No PipeWire server reachable.
    NoPipewire,
    /// Something in the chain failed; see `State::error`.
    Error,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Usb,
    Builtin,
    Bluetooth,
    Headset,
    Headphones,
    Speaker,
    Hdmi,
    Webcam,
    Loopback,
    Other,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ProfileInfo {
    /// PipeWire profile index, used to switch profiles.
    pub index: i32,
    /// Technical name, e.g. `input:mono-fallback`.
    pub name: String,
    /// PipeWire's own description.
    pub description: String,
    pub kind: ProfileKind,
    pub available: bool,
    pub priority: i32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Device {
    /// `node.name`, stable across reconnects. This is what settings refer to.
    pub id: String,
    pub node_id: u32,
    /// Human readable name.
    pub name: String,
    pub kind: DeviceKind,
    /// PipeWire device (card) object id, if the node belongs to one.
    pub card: Option<u32>,
    pub profile: Option<ProfileInfo>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ProfileHint {
    pub card: u32,
    pub device_id: String,
    pub device_name: String,
    pub current: ProfileInfo,
    pub suggested: ProfileInfo,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct State {
    pub version: String,
    pub status: Status,
    /// Technical detail for `Status::Error` (shown behind a "details" toggle).
    pub error: Option<String>,
    pub settings: Settings,
    /// Microphones worth showing.
    pub devices: Vec<Device>,
    /// Webcams, loopbacks and similar, only shown on request.
    pub hidden_devices: Vec<Device>,
    /// `node.name` of the microphone currently feeding the chain.
    pub active_mic: Option<String>,
    /// Output devices (speakers, headphones, HDMI) for choosing the system default output.
    pub sinks: Vec<Device>,
    /// `node.name` of the current system default microphone / output.
    pub default_source: Option<String>,
    pub default_sink: Option<String>,
    pub profile_hint: Option<ProfileHint>,
    /// Estimated end-to-end latency added by Sordino, in milliseconds.
    pub latency_ms: Option<f32>,
    pub default_is_sordino: bool,
    /// User is currently hearing themselves.
    pub monitoring: bool,
    /// A/B test: monitor plays the unprocessed signal.
    pub ab_original: bool,
    pub echo_available: bool,
    /// Values of the built-in studio presets, so UIs can seed their advanced sliders.
    pub presets: BTreeMap<String, StudioParams>,
    pub diag: Diag,
}

pub fn builtin_presets() -> BTreeMap<String, StudioParams> {
    [Preset::Natural, Preset::Clear, Preset::Warm]
        .into_iter()
        .filter_map(|p| p.params().map(|v| (p.as_str().to_string(), v)))
        .collect()
}

/// Counters that reveal audio glitches. All zero means the audio path ran cleanly.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(default)]
pub struct Diag {
    /// "Sordino Mic" callbacks that had to be padded with silence (audible as crackle).
    pub out_underruns: u64,
    /// Samples discarded because the output fell behind (audible as a skip).
    pub out_skipped: u64,
    /// Microphone samples lost because the DSP thread was too slow.
    pub in_dropped: u64,
    /// Hops where the noise model failed and audio passed through unprocessed.
    pub model_errors: u64,
    /// Graph quantum (samples per cycle) seen by "Sordino Mic".
    pub quantum: u32,
    pub out_callbacks: u64,
    /// Hops processed without noise suppression because the DSP thread fell behind real time.
    pub overload_hops: u64,
    pub overload_events: u64,
    /// DSP thread priority: 0 normal, 1 high (nice), 2 real-time.
    pub dsp_priority: u8,
    pub skip_events: u64,
    /// Callback number of the most recent skip / cycle number of the most recent input drop.
    pub last_skip_cb: u64,
    pub last_drop_cycle: u64,
}

/// Live levels, pushed ~20 times per second while a client is watching.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub struct Levels {
    pub input_db: f32,
    pub output_db: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrips_through_json() {
        let s = State {
            version: "0.1.0".into(),
            status: Status::Running,
            error: None,
            settings: Settings::default(),
            devices: vec![],
            hidden_devices: vec![],
            active_mic: Some("alsa_input.x".into()),
            sinks: vec![],
            default_source: None,
            default_sink: None,
            profile_hint: None,
            latency_ms: Some(31.5),
            default_is_sordino: false,
            monitoring: false,
            ab_original: false,
            echo_available: true,
            presets: builtin_presets(),
            diag: Diag::default(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: State = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
