//! State types shared by the daemon, `hushctl` and the UI. Serialised as JSON over D-Bus.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::profile::ProfileKind;
use crate::settings::Settings;
use crate::studio::{Preset, StudioParams};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Hush Mic is switched off by the user.
    Off,
    Starting,
    Running,
    /// The selected microphone is not connected; Hush Mic comes back automatically.
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
    pub profile_hint: Option<ProfileHint>,
    /// Estimated end-to-end latency added by Hush, in milliseconds.
    pub latency_ms: Option<f32>,
    pub default_is_hush: bool,
    /// User is currently hearing themselves.
    pub monitoring: bool,
    /// A/B test: monitor plays the unprocessed signal.
    pub ab_original: bool,
    pub echo_available: bool,
    /// Values of the built-in studio presets, so UIs can seed their advanced sliders.
    pub presets: BTreeMap<String, StudioParams>,
}

pub fn builtin_presets() -> BTreeMap<String, StudioParams> {
    [Preset::Natural, Preset::Clear, Preset::Warm]
        .into_iter()
        .filter_map(|p| p.params().map(|v| (p.as_str().to_string(), v)))
        .collect()
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
            profile_hint: None,
            latency_ms: Some(31.5),
            default_is_hush: false,
            monitoring: false,
            ab_original: false,
            echo_available: true,
            presets: builtin_presets(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: State = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
