//! Audio-profile knowledge: classify PipeWire device profiles and suggest a better one.
//!
//! The typical trap: a USB mic sits on `pro-audio`, which exposes a raw multichannel node
//! instead of a normal mono microphone. Sordino recognises that and offers a one-click fix.

use serde::{Deserialize, Serialize};

use crate::ipc::ProfileInfo;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    /// Mono microphone profile, best for calls.
    Voice,
    /// `pro-audio`: raw multichannel, not meant for calls.
    Studio,
    /// Normal stereo input.
    Stereo,
    /// Bluetooth headset (HFP/HSP) profile.
    Headset,
    /// Bluetooth music profile (no microphone).
    Music,
    Off,
    Other,
}

/// Classify a PipeWire profile by its name (e.g. `input:mono-fallback`).
pub fn classify(name: &str) -> ProfileKind {
    match name {
        "off" => ProfileKind::Off,
        "pro-audio" => ProfileKind::Studio,
        n if n.starts_with("a2dp") => ProfileKind::Music,
        n if n.starts_with("headset") || n.contains("hfp") || n.contains("hsp") => {
            ProfileKind::Headset
        }
        n if n.contains("input:mono-fallback") => ProfileKind::Voice,
        n if n.contains("input:") => ProfileKind::Stereo,
        _ => ProfileKind::Other,
    }
}

/// Whether a device on this profile deserves a warning banner.
pub fn is_unfavourable(kind: ProfileKind) -> bool {
    matches!(kind, ProfileKind::Studio)
}

/// Pick the best profile to switch a device on an unfavourable profile to.
///
/// Prefers a mono microphone profile that keeps audio output working (`output:..+input:mono-fallback`),
/// then any mono microphone, then any stereo input. Never suggests unavailable profiles.
pub fn suggest(profiles: &[ProfileInfo], current: Option<i32>) -> Option<&ProfileInfo> {
    let score = |p: &ProfileInfo| -> Option<u32> {
        if !p.available || Some(p.index) == current {
            return None;
        }
        match p.kind {
            ProfileKind::Voice if p.name.contains("output:") => Some(4),
            ProfileKind::Voice => Some(3),
            ProfileKind::Stereo if p.name.contains("output:") => Some(2),
            ProfileKind::Stereo => Some(1),
            _ => None,
        }
    };
    profiles
        .iter()
        .filter_map(|p| score(p).map(|s| (s, p)))
        // Highest score wins; ties go to the profile PipeWire itself ranks higher.
        .max_by_key(|(s, p)| (*s, p.priority))
        .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(index: i32, name: &str, available: bool, priority: i32) -> ProfileInfo {
        ProfileInfo {
            index,
            name: name.into(),
            description: name.into(),
            kind: classify(name),
            available,
            priority,
        }
    }

    #[test]
    fn classification() {
        assert_eq!(classify("off"), ProfileKind::Off);
        assert_eq!(classify("pro-audio"), ProfileKind::Studio);
        assert_eq!(classify("input:mono-fallback"), ProfileKind::Voice);
        assert_eq!(
            classify("output:analog-stereo+input:mono-fallback"),
            ProfileKind::Voice
        );
        assert_eq!(classify("input:analog-stereo"), ProfileKind::Stereo);
        assert_eq!(classify("a2dp-sink"), ProfileKind::Music);
        assert_eq!(classify("headset-head-unit"), ProfileKind::Headset);
        assert_eq!(classify("output:analog-stereo"), ProfileKind::Other);
        assert!(is_unfavourable(ProfileKind::Studio));
        assert!(!is_unfavourable(ProfileKind::Voice));
    }

    #[test]
    fn suggests_mono_with_output_first() {
        let profiles = vec![
            p(0, "off", true, 0),
            p(1, "pro-audio", true, 1),
            p(2, "input:mono-fallback", true, 1),
            p(3, "output:analog-stereo+input:mono-fallback", true, 6),
            p(4, "input:analog-stereo", true, 60),
        ];
        assert_eq!(suggest(&profiles, Some(1)).unwrap().index, 3);
    }

    #[test]
    fn skips_unavailable_and_current() {
        let profiles = vec![
            p(1, "pro-audio", true, 1),
            p(2, "input:mono-fallback", false, 1),
            p(4, "input:analog-stereo", true, 60),
        ];
        assert_eq!(suggest(&profiles, Some(1)).unwrap().index, 4);
        let none = vec![p(1, "pro-audio", true, 1), p(0, "off", true, 0)];
        assert!(suggest(&none, Some(1)).is_none());
    }
}
