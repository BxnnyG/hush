//! Turning PipeWire registry objects into the device list the UI shows.

use std::collections::HashMap;

use pipewire::spa::pod::deserialize::PodDeserializer;
use pipewire::spa::pod::{Object, Value};
use pipewire::spa::sys;
use sordino_core::ipc::{Device, DeviceKind, ProfileInfo};
use sordino_core::profile::classify;

/// What we know about a physical audio source node.
#[derive(Clone, Debug)]
pub struct SourceNode {
    pub node_id: u32,
    pub name: String,
    pub description: String,
    pub card: Option<u32>,
    pub bus: Option<String>,
    pub form_factor: Option<String>,
    pub api: Option<String>,
}

/// What we know about a device (card) object.
#[derive(Clone, Debug, Default)]
pub struct CardInfo {
    pub profiles: Vec<ProfileInfo>,
    pub active: Option<i32>,
}

impl SourceNode {
    pub fn from_props(node_id: u32, get: impl Fn(&str) -> Option<String>) -> Option<SourceNode> {
        Self::from_props_class(node_id, "Audio/Source", get)
    }

    /// Parse a node of the given `media.class` (`Audio/Source` or `Audio/Sink`).
    pub fn from_props_class(
        node_id: u32,
        class: &str,
        get: impl Fn(&str) -> Option<String>,
    ) -> Option<SourceNode> {
        if get("media.class")? != class {
            return None;
        }
        let name = get("node.name")?;
        Some(SourceNode {
            node_id,
            description: get("node.description")
                .or_else(|| get("node.nick"))
                .unwrap_or_else(|| name.clone()),
            name,
            card: get("device.id").and_then(|v| v.parse().ok()),
            bus: get("device.bus"),
            form_factor: get("device.form-factor"),
            api: get("device.api"),
        })
    }

    pub fn kind(&self) -> DeviceKind {
        let name = self.name.to_lowercase();
        let desc = self.description.to_lowercase();
        if name.contains("snd_aloop") || desc.contains("loopback") || desc.contains("schleifen") {
            return DeviceKind::Loopback;
        }
        if desc.contains("webcam") || desc.contains("camera") || desc.contains("kamera") {
            return DeviceKind::Webcam;
        }
        if self.api.as_deref() == Some("bluez5") || name.starts_with("bluez") {
            return DeviceKind::Bluetooth;
        }
        match self.form_factor.as_deref() {
            Some("headset") | Some("headphone") | Some("hands-free") => return DeviceKind::Headset,
            Some("webcam") => return DeviceKind::Webcam,
            Some("internal") => return DeviceKind::Builtin,
            _ => {}
        }
        match self.bus.as_deref() {
            Some("usb") => DeviceKind::Usb,
            Some("pci") => DeviceKind::Builtin,
            _ => DeviceKind::Other,
        }
    }

    /// Webcams, loopbacks and similar are hidden unless the user asks for everything.
    /// Kind of an output device, from the form factor and names.
    pub fn sink_kind(&self) -> DeviceKind {
        let name = self.name.to_lowercase();
        let desc = self.description.to_lowercase();
        if name.contains("snd_aloop") || desc.contains("loopback") {
            return DeviceKind::Loopback;
        }
        if self.api.as_deref() == Some("bluez5") || name.starts_with("bluez") {
            return DeviceKind::Bluetooth;
        }
        if name.contains("hdmi") || desc.contains("hdmi") || desc.contains("displayport") {
            return DeviceKind::Hdmi;
        }
        match self.form_factor.as_deref() {
            Some("headphone") => return DeviceKind::Headphones,
            Some("headset") | Some("hands-free") => return DeviceKind::Headset,
            Some("speaker") | Some("internal") => return DeviceKind::Speaker,
            _ => {}
        }
        match self.bus.as_deref() {
            Some("usb") => DeviceKind::Usb,
            Some("pci") => DeviceKind::Builtin,
            _ => DeviceKind::Other,
        }
    }

    pub fn hidden_by_default(&self) -> bool {
        matches!(self.kind(), DeviceKind::Webcam | DeviceKind::Loopback)
    }

    pub fn to_sink_device(&self, cards: &HashMap<u32, CardInfo>) -> Device {
        Device {
            kind: self.sink_kind(),
            ..self.to_device(cards)
        }
    }

    pub fn to_device(&self, cards: &HashMap<u32, CardInfo>) -> Device {
        let profile = self.card.and_then(|c| cards.get(&c)).and_then(|c| {
            let active = c.active?;
            c.profiles.iter().find(|p| p.index == active).cloned()
        });
        Device {
            id: self.name.clone(),
            node_id: self.node_id,
            name: self.description.clone(),
            kind: self.kind(),
            card: self.card,
            profile,
        }
    }
}

/// Parse one `EnumProfile` pod.
pub fn parse_enum_profile(bytes: &[u8]) -> Option<ProfileInfo> {
    let (_, value) = PodDeserializer::deserialize_any_from(bytes).ok()?;
    let Value::Object(Object { properties, .. }) = value else {
        return None;
    };
    let mut index = None;
    let mut name = None;
    let mut description = None;
    let mut priority = 0;
    let mut available = true;
    for p in properties {
        match (p.key, p.value) {
            (sys::SPA_PARAM_PROFILE_index, Value::Int(v)) => index = Some(v),
            (sys::SPA_PARAM_PROFILE_name, Value::String(v)) => name = Some(v),
            (sys::SPA_PARAM_PROFILE_description, Value::String(v)) => description = Some(v),
            (sys::SPA_PARAM_PROFILE_priority, Value::Int(v)) => priority = v,
            // SPA_PARAM_AVAILABILITY_no = 1
            (sys::SPA_PARAM_PROFILE_available, Value::Id(id)) => available = id.0 != 1,
            _ => {}
        }
    }
    let name = name?;
    Some(ProfileInfo {
        index: index?,
        kind: classify(&name),
        description: description.unwrap_or_else(|| name.clone()),
        name,
        available,
        priority,
    })
}

/// Parse the current `Profile` pod and return the active profile index.
pub fn parse_profile_index(bytes: &[u8]) -> Option<i32> {
    let (_, value) = PodDeserializer::deserialize_any_from(bytes).ok()?;
    let Value::Object(Object { properties, .. }) = value else {
        return None;
    };
    properties.into_iter().find_map(|p| match (p.key, p.value) {
        (sys::SPA_PARAM_PROFILE_index, Value::Int(v)) => Some(v),
        _ => None,
    })
}

/// Build the pod that selects a profile on a device.
pub fn profile_pod(index: i32) -> Option<Vec<u8>> {
    use pipewire::spa::pod::{serialize::PodSerializer, Property, PropertyFlags};
    let obj = Object {
        type_: sys::SPA_TYPE_OBJECT_ParamProfile,
        id: sys::SPA_PARAM_Profile,
        properties: vec![
            Property {
                key: sys::SPA_PARAM_PROFILE_index,
                flags: PropertyFlags::empty(),
                value: Value::Int(index),
            },
            // `save` makes WirePlumber remember the choice for this device.
            Property {
                key: sys::SPA_PARAM_PROFILE_save,
                flags: PropertyFlags::empty(),
                value: Value::Bool(true),
            },
        ],
    };
    PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &Value::Object(obj))
        .ok()
        .map(|(c, _)| c.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, desc: &str, bus: Option<&str>, ff: Option<&str>) -> SourceNode {
        SourceNode {
            node_id: 1,
            name: name.into(),
            description: desc.into(),
            card: Some(2),
            bus: bus.map(Into::into),
            form_factor: ff.map(Into::into),
            api: None,
        }
    }

    #[test]
    fn webcams_and_loopbacks_are_hidden() {
        assert!(node(
            "alsa_input.usb-Example_Studio_Webcam",
            "Studio Webcam Pro Analog Stereo",
            Some("usb"),
            Some("webcam")
        )
        .hidden_by_default());
        assert!(node(
            "alsa_input.platform-snd_aloop.0.analog-stereo",
            "Loopback Analog Stereo",
            None,
            None
        )
        .hidden_by_default());
        assert!(!node(
            "alsa_input.usb-Example_Mic_X1-00.mono-fallback",
            "Example Mic X1 Mono",
            Some("usb"),
            None
        )
        .hidden_by_default());
    }

    #[test]
    fn kinds() {
        assert_eq!(
            node("a", "USB Audio Device Mono", Some("usb"), None).kind(),
            DeviceKind::Usb
        );
        assert_eq!(
            node("a", "Internes Audio Analoges Stereo", Some("pci"), None).kind(),
            DeviceKind::Builtin
        );
        assert_eq!(
            node("bluez_input.x", "Headset", None, None).kind(),
            DeviceKind::Bluetooth
        );
    }

    #[test]
    fn only_sources_are_accepted() {
        let props = |k: &str| match k {
            "media.class" => Some("Audio/Sink".to_string()),
            "node.name" => Some("x".to_string()),
            _ => None,
        };
        assert!(SourceNode::from_props(1, props).is_none());
    }

    #[test]
    fn profile_pod_roundtrips() {
        let bytes = profile_pod(5).unwrap();
        assert_eq!(parse_profile_index(&bytes), Some(5));
    }
}
