//! Persisted user settings (`~/.config/sordino/config.toml`) and runtime state.
//!
//! Unknown keys are ignored and missing ones fall back to defaults, so old and new versions
//! can read each other's files. A broken file is moved aside instead of crashing the daemon.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::denoise::Strength;
use crate::pipeline::PipelineParams;
use crate::studio::{Preset, StudioParams};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct NoiseSettings {
    pub enabled: bool,
    pub strength: Strength,
}

impl Default for NoiseSettings {
    fn default() -> Self {
        NoiseSettings {
            enabled: true,
            strength: Strength::High,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct EchoSettings {
    pub enabled: bool,
    /// `node.name` of the sink (or source) whose signal is the echo reference. `None` uses the
    /// default output device, which is right unless you play through something unusual.
    pub reference: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct StudioSettings {
    pub preset: Preset,
    /// Values used when `preset == Custom`; the UI seeds them from the last preset.
    pub custom: StudioParams,
}

impl StudioSettings {
    /// Effective parameters, `None` when studio sound is off.
    pub fn effective(&self) -> Option<StudioParams> {
        match self.preset {
            Preset::Off => None,
            Preset::Custom => Some(self.custom.sanitized()),
            p => p.params(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    /// Sordino Mic exists and processes audio.
    pub enabled: bool,
    /// `node.name` of the physical microphone; `None` follows the system default.
    pub mic: Option<String>,
    /// Make Sordino Mic the system default microphone while running.
    pub set_default: bool,
    /// Keep running in the tray when the window is closed.
    pub run_in_background: bool,
    pub show_all_devices: bool,
    pub noise: NoiseSettings,
    pub echo: EchoSettings,
    pub studio: StudioSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: true,
            mic: None,
            set_default: false,
            run_in_background: true,
            show_all_devices: false,
            noise: NoiseSettings::default(),
            echo: EchoSettings::default(),
            studio: StudioSettings::default(),
        }
    }
}

impl Settings {
    pub fn pipeline_params(&self) -> PipelineParams {
        PipelineParams {
            echo: self.echo.enabled,
            noise: self.noise.enabled,
            strength: self.noise.strength,
            studio: self.studio.effective(),
        }
    }

    /// Merge a JSON patch (RFC 7386 style: objects merge, everything else replaces).
    pub fn patched(&self, patch: &serde_json::Value) -> Result<Settings> {
        let mut base = serde_json::to_value(self)?;
        merge(&mut base, patch);
        let mut s: Settings = serde_json::from_value(base).context("invalid settings patch")?;
        s.studio.custom = s.studio.custom.sanitized();
        Ok(s)
    }
}

fn merge(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                merge(b.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
        (b, p) => *b = p.clone(),
    }
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() && PathBuf::from(&v).is_absolute() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback),
    }
}

pub fn config_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
        .join("sordino")
        .join("config.toml")
}

pub fn state_path() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state")
        .join("sordino")
        .join("state.toml")
}

/// Write atomically: a crash mid-write must never leave a half-written config behind.
fn write_atomic(path: &PathBuf, contents: &str) -> Result<()> {
    let dir = path.parent().context("path has no parent")?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = path.with_extension("tmp");
    {
        let mut f =
            fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("renaming to {}", path.display()))?;
    Ok(())
}

impl Settings {
    pub fn load() -> Settings {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &PathBuf) -> Settings {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => return Settings::default(),
        };
        match toml::from_str::<Settings>(&text) {
            Ok(mut s) => {
                s.studio.custom = s.studio.custom.sanitized();
                s
            }
            Err(e) => {
                log::warn!(
                    "config {} is invalid ({e}); moving it aside and using defaults",
                    path.display()
                );
                let _ = fs::rename(path, path.with_extension("toml.broken"));
                Settings::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&config_path())
    }

    pub fn save_to(&self, path: &PathBuf) -> Result<()> {
        write_atomic(path, &toml::to_string_pretty(self)?)
    }
}

/// Things the daemon must remember across restarts but that are not user preferences.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
#[serde(default)]
pub struct RuntimeState {
    /// Value of `default.configured.audio.source` before Sordino overrode it.
    /// `Some("")` means "there was none". `None` means Sordino did not touch the default.
    pub previous_default_source: Option<String>,
}

impl RuntimeState {
    pub fn load() -> RuntimeState {
        fs::read_to_string(state_path())
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        write_atomic(&state_path(), &toml::to_string_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sordino-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.join("config.toml")
    }

    #[test]
    fn roundtrip() {
        let p = tmp("roundtrip");
        let s = Settings {
            mic: Some("alsa_input.usb-x".into()),
            noise: NoiseSettings {
                strength: Strength::Max,
                ..Default::default()
            },
            studio: StudioSettings {
                preset: Preset::Clear,
                ..Default::default()
            },
            ..Default::default()
        };
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
    }

    #[test]
    fn missing_file_gives_defaults() {
        assert_eq!(Settings::load_from(&tmp("missing")), Settings::default());
    }

    #[test]
    fn partial_and_unknown_keys_are_tolerated() {
        let p = tmp("partial");
        fs::write(&p, "future_key = 1\n[noise]\nstrength = \"light\"\n").unwrap();
        let s = Settings::load_from(&p);
        assert_eq!(s.noise.strength, Strength::Light);
        assert!(s.noise.enabled);
        assert!(s.enabled);
    }

    #[test]
    fn broken_file_is_moved_aside() {
        let p = tmp("broken");
        fs::write(&p, "this is [not toml").unwrap();
        assert_eq!(Settings::load_from(&p), Settings::default());
        assert!(!p.exists());
        assert!(p.with_extension("toml.broken").exists());
    }

    #[test]
    fn hand_edited_garbage_values_are_sanitized() {
        let p = tmp("garbage");
        fs::write(
            &p,
            "[studio.custom]\nwarmth_db = 99.0\ncompression = -4.0\n",
        )
        .unwrap();
        let s = Settings::load_from(&p);
        assert_eq!(s.studio.custom.warmth_db, 6.0);
        assert_eq!(s.studio.custom.compression, 0.0);
    }

    #[test]
    fn patch_merges_nested_objects() {
        let s = Settings::default();
        let n = s
            .patched(&serde_json::json!({"noise": {"strength": "max"}, "set_default": true}))
            .unwrap();
        assert_eq!(n.noise.strength, Strength::Max);
        assert!(n.noise.enabled, "sibling keys must survive");
        assert!(n.set_default);
        assert!(s
            .patched(&serde_json::json!({"noise": {"strength": "bogus"}}))
            .is_err());
    }

    #[test]
    fn studio_effective() {
        let mut s = StudioSettings::default();
        assert_eq!(s.effective(), Preset::Natural.params());
        s.preset = Preset::Off;
        assert_eq!(s.effective(), None);
        s.preset = Preset::Custom;
        s.custom.warmth_db = 2.0;
        assert_eq!(s.effective().unwrap().warmth_db, 2.0);
    }
}
