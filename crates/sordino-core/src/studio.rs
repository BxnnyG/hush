//! "Studio sound": presets instead of EQ curves, plus an optional advanced view.
//!
//! Chain order (see the plan): low-cut -> gate -> EQ -> de-esser -> compressor -> limiter.

use serde::{Deserialize, Serialize};

use crate::dsp::biquad::{Biquad, Coeffs, FilterKind};
use crate::dsp::compressor::Compressor;
use crate::dsp::deesser::DeEsser;
use crate::dsp::gate::Gate;
use crate::dsp::limiter::Limiter;
use crate::SAMPLE_RATE;

const FS: f32 = SAMPLE_RATE as f32;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    /// Only noise suppression, nothing else.
    Off,
    #[default]
    Natural,
    Clear,
    Warm,
    /// Use the user's advanced values.
    Custom,
}

impl Preset {
    pub const ALL: [Preset; 5] = [
        Preset::Off,
        Preset::Natural,
        Preset::Clear,
        Preset::Warm,
        Preset::Custom,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Preset::Off => "off",
            Preset::Natural => "natural",
            Preset::Clear => "clear",
            Preset::Warm => "warm",
            Preset::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.as_str() == s)
    }

    /// Parameters of a built-in preset. `Off` and `Custom` have none of their own.
    pub fn params(self) -> Option<StudioParams> {
        match self {
            Preset::Off | Preset::Custom => None,
            Preset::Natural => Some(StudioParams {
                lowcut_hz: 80.0,
                warmth_db: 0.0,
                presence_db: 1.0,
                air_db: 0.0,
                deess: 0.2,
                compression: 0.3,
                gate: 0.0,
                limiter: true,
            }),
            Preset::Clear => Some(StudioParams {
                lowcut_hz: 100.0,
                warmth_db: -1.0,
                presence_db: 4.0,
                air_db: 2.0,
                deess: 0.5,
                compression: 0.5,
                gate: 0.25,
                limiter: true,
            }),
            Preset::Warm => Some(StudioParams {
                lowcut_hz: 70.0,
                warmth_db: 4.0,
                presence_db: 1.0,
                air_db: -1.0,
                deess: 0.4,
                compression: 0.75,
                gate: 0.25,
                limiter: true,
            }),
        }
    }
}

/// The handful of knobs behind the presets. Also what the "advanced" sliders edit.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct StudioParams {
    /// Low-cut corner in Hz, 0 = off.
    pub lowcut_hz: f32,
    /// Low shelf at 180 Hz, dB (-6..6).
    pub warmth_db: f32,
    /// Peak at 3.2 kHz, dB (-6..6).
    pub presence_db: f32,
    /// High shelf at 9 kHz, dB (-6..6).
    pub air_db: f32,
    /// De-esser strength 0..1.
    pub deess: f32,
    /// Compression amount 0..1, 0 = off.
    pub compression: f32,
    /// Gate amount 0..1, 0 = off.
    pub gate: f32,
    pub limiter: bool,
}

impl Default for StudioParams {
    fn default() -> Self {
        Preset::Natural.params().expect("natural has params")
    }
}

impl StudioParams {
    /// Clamp everything to its valid range; NaNs (from a hand-edited config) become defaults.
    pub fn sanitized(mut self) -> Self {
        let d = StudioParams::default();
        let fix = |v: f32, lo: f32, hi: f32, dflt: f32| {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                dflt
            }
        };
        self.lowcut_hz = fix(self.lowcut_hz, 0.0, 300.0, d.lowcut_hz);
        self.warmth_db = fix(self.warmth_db, -6.0, 6.0, d.warmth_db);
        self.presence_db = fix(self.presence_db, -6.0, 6.0, d.presence_db);
        self.air_db = fix(self.air_db, -6.0, 6.0, d.air_db);
        self.deess = fix(self.deess, 0.0, 1.0, d.deess);
        self.compression = fix(self.compression, 0.0, 1.0, d.compression);
        self.gate = fix(self.gate, 0.0, 1.0, d.gate);
        self
    }

    fn compressor_curve(&self) -> (f32, f32, f32) {
        let c = self.compression;
        let threshold = -14.0 - 14.0 * c;
        let ratio = 1.5 + 3.5 * c;
        // Rough automatic make-up so presets do not get quieter when compressing.
        let makeup = 0.3 * (-threshold) * (1.0 - 1.0 / ratio);
        (threshold, ratio, makeup)
    }

    fn gate_curve(&self) -> (f32, f32) {
        (-62.0 + 20.0 * self.gate, 12.0 + 18.0 * self.gate)
    }
}

pub struct StudioChain {
    params: StudioParams,
    lowcut: [Biquad; 2],
    warmth: Biquad,
    presence: Biquad,
    air: Biquad,
    gate: Gate,
    deesser: DeEsser,
    comp: Compressor,
    limiter: Limiter,
}

impl StudioChain {
    pub fn new(params: StudioParams) -> Self {
        let params = params.sanitized();
        let (gt, gr) = params.gate_curve();
        let (ct, cr, cm) = params.compressor_curve();
        let mut chain = StudioChain {
            params,
            lowcut: [Biquad::default(), Biquad::default()],
            warmth: Biquad::default(),
            presence: Biquad::default(),
            air: Biquad::default(),
            gate: Gate::new(gt, gr, FS),
            deesser: DeEsser::new(5500.0, params.deess, FS),
            comp: Compressor::new(ct, cr, 6.0, 140.0, cm, FS),
            limiter: Limiter::new(-1.0, 60.0, FS),
        };
        chain.apply(params);
        chain
    }

    pub fn params(&self) -> StudioParams {
        self.params
    }

    pub fn set_params(&mut self, params: StudioParams) {
        self.apply(params.sanitized());
    }

    fn apply(&mut self, p: StudioParams) {
        self.params = p;
        // 4th-order Butterworth low-cut as two biquads; passes everything when off.
        if p.lowcut_hz > 1.0 {
            self.lowcut[0].set_coeffs(Coeffs::new(
                FilterKind::HighPass,
                p.lowcut_hz,
                0.5412,
                0.0,
                FS,
            ));
            self.lowcut[1].set_coeffs(Coeffs::new(
                FilterKind::HighPass,
                p.lowcut_hz,
                1.3066,
                0.0,
                FS,
            ));
        } else {
            self.lowcut[0].set_coeffs(Coeffs::PASS);
            self.lowcut[1].set_coeffs(Coeffs::PASS);
        }
        self.warmth.set_coeffs(Coeffs::new(
            FilterKind::LowShelf,
            180.0,
            0.707,
            p.warmth_db,
            FS,
        ));
        self.presence.set_coeffs(Coeffs::new(
            FilterKind::Peaking,
            3200.0,
            0.9,
            p.presence_db,
            FS,
        ));
        self.air.set_coeffs(Coeffs::new(
            FilterKind::HighShelf,
            9000.0,
            0.707,
            p.air_db,
            FS,
        ));
        let (gt, gr) = p.gate_curve();
        self.gate.set(gt, gr);
        self.deesser.set_amount(p.deess);
        let (ct, cr, cm) = p.compressor_curve();
        self.comp.set(ct, cr, cm);
    }

    pub fn reset(&mut self) {
        for b in &mut self.lowcut {
            b.reset();
        }
        self.warmth.reset();
        self.presence.reset();
        self.air.reset();
        self.gate.reset();
        self.deesser.reset();
        self.comp.reset();
        self.limiter.reset();
    }

    /// Extra latency in samples introduced by this chain.
    pub fn latency(&self) -> usize {
        if self.params.limiter {
            Limiter::latency()
        } else {
            0
        }
    }

    pub fn process(&mut self, block: &mut [f32]) {
        let p = self.params;
        if p.lowcut_hz > 1.0 {
            self.lowcut[0].process(block);
            self.lowcut[1].process(block);
        }
        if p.gate > 0.0 {
            self.gate.process(block);
        }
        if p.warmth_db != 0.0 {
            self.warmth.process(block);
        }
        if p.presence_db != 0.0 {
            self.presence.process(block);
        }
        if p.air_db != 0.0 {
            self.air.process(block);
        }
        if p.deess > 0.0 {
            self.deesser.process(block);
        }
        if p.compression > 0.0 {
            self.comp.process(block);
        }
        if p.limiter {
            self.limiter.process(block);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{peak_db, rms_db};

    fn tone(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / FS).sin())
            .collect()
    }

    #[test]
    fn presets_are_in_range_and_distinct() {
        let mut seen = Vec::new();
        for p in [Preset::Natural, Preset::Clear, Preset::Warm] {
            let params = p.params().unwrap();
            assert_eq!(params, params.sanitized(), "{p:?} is out of range");
            assert!(!seen.contains(&params));
            seen.push(params);
        }
        assert!(Preset::Off.params().is_none());
    }

    #[test]
    fn preset_names_roundtrip() {
        for p in Preset::ALL {
            assert_eq!(Preset::parse(p.as_str()), Some(p));
        }
        assert_eq!(Preset::parse("nope"), None);
    }

    #[test]
    fn sanitize_fixes_garbage() {
        let p = StudioParams {
            lowcut_hz: f32::NAN,
            warmth_db: 99.0,
            compression: -3.0,
            ..Default::default()
        }
        .sanitized();
        assert!(p.lowcut_hz.is_finite());
        assert_eq!(p.warmth_db, 6.0);
        assert_eq!(p.compression, 0.0);
    }

    #[test]
    fn lowcut_removes_rumble_keeps_voice() {
        let mut c = StudioChain::new(StudioParams {
            lowcut_hz: 100.0,
            warmth_db: 0.0,
            presence_db: 0.0,
            air_db: 0.0,
            deess: 0.0,
            compression: 0.0,
            gate: 0.0,
            limiter: false,
        });
        let mut rumble = tone(30.0, 0.3, 48000);
        c.process(&mut rumble);
        assert!(rms_db(&rumble[24000..]) < rms_db(&tone(30.0, 0.3, 4800)) - 30.0);
        c.reset();
        let mut voice = tone(400.0, 0.3, 48000);
        c.process(&mut voice);
        assert!((rms_db(&voice[24000..]) - rms_db(&tone(400.0, 0.3, 4800))).abs() < 0.3);
    }

    #[test]
    fn every_preset_stays_below_ceiling_and_finite() {
        for preset in [Preset::Natural, Preset::Clear, Preset::Warm] {
            let mut c = StudioChain::new(preset.params().unwrap());
            // loud speech-like mix: 200 Hz + 3 kHz + 7 kHz bursts
            let mut x: Vec<f32> = (0..96000)
                .map(|i| {
                    let t = i as f32 / FS;
                    let env = if (t * 3.0).fract() < 0.5 { 1.0 } else { 0.05 };
                    env * (0.5 * (2.0 * std::f32::consts::PI * 200.0 * t).sin()
                        + 0.4 * (2.0 * std::f32::consts::PI * 3000.0 * t).sin()
                        + 0.4 * (2.0 * std::f32::consts::PI * 7000.0 * t).sin())
                })
                .collect();
            c.process(&mut x);
            assert!(x.iter().all(|v| v.is_finite()), "{preset:?}");
            assert!(peak_db(&x) <= -0.9, "{preset:?} peak {}", peak_db(&x));
        }
    }

    #[test]
    fn param_changes_are_click_free() {
        let mut c = StudioChain::new(Preset::Natural.params().unwrap());
        let mut a = tone(300.0, 0.2, 24000);
        c.process(&mut a);
        c.set_params(Preset::Warm.params().unwrap());
        let mut b = tone(300.0, 0.2, 24000);
        c.process(&mut b);
        // The largest sample-to-sample jump right after the switch must be in the same
        // ballpark as before it (no state reset spike).
        let max_jump = |x: &[f32]| {
            x.windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max)
        };
        assert!(max_jump(&b[..480]) < max_jump(&a[20000..]) * 3.0 + 0.05);
    }
}
