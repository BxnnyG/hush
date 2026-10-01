//! DeepFilterNet 3 wrapper. Works on exactly one hop (10 ms, 480 samples) at a time.

use anyhow::{anyhow, Result};
use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};
use serde::{Deserialize, Serialize};

use crate::{HOP, SAMPLE_RATE};

/// User-facing noise suppression levels, mapped to DeepFilterNet's attenuation limit.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    Light,
    Medium,
    #[default]
    High,
    Max,
}

impl Strength {
    pub const ALL: [Strength; 4] = [
        Strength::Light,
        Strength::Medium,
        Strength::High,
        Strength::Max,
    ];

    /// Maximum attenuation in dB handed to DeepFilterNet (>= 100 means unlimited).
    pub fn atten_limit_db(self) -> f32 {
        match self {
            Strength::Light => 12.0,
            Strength::Medium => 24.0,
            Strength::High => 40.0,
            Strength::Max => 100.0,
        }
    }

    /// Post-filter strength; only the strongest level trades artifacts for extra suppression.
    fn post_filter_beta(self) -> f32 {
        match self {
            Strength::Max => 0.02,
            _ => 0.0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Strength::Light => "light",
            Strength::Medium => "medium",
            Strength::High => "high",
            Strength::Max => "max",
        }
    }

    pub fn parse(s: &str) -> Option<Strength> {
        Strength::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// 0..=3 slider position.
    pub fn index(self) -> usize {
        Strength::ALL.iter().position(|s| *s == self).unwrap_or(2)
    }
}

/// DeepFilterNet's local-SNR switching thresholds (dB). Below `min` a frame counts as pure noise
/// (zero mask), above `max_erb` as clean speech (passed through), above `max_df` only the first
/// stage runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thresholds {
    pub min_db: f32,
    pub max_erb_db: f32,
    pub max_df_db: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        // NOT the library defaults (-10 / 30 / 20). With those, any frame above 20 dB local SNR
        // skips the deep-filtering stage and only gets the coarse first-stage mask, which sounds
        // muffled and robotic ("underwater") and even degrades *clean* speech. Measured on real
        // speech (SI-SDR / STOI / PESQ, mean over 3 SNRs and 2 noise types): library defaults
        // 11.4 dB / 0.901 / 2.41, these values 19.9 dB / 0.976 / 3.10 (the DeepFilterNet LADSPA
        // plugin uses the same ones). `min` barely matters, `max_df` is what counts.
        Thresholds {
            min_db: -15.0,
            max_erb_db: 35.0,
            max_df_db: 35.0,
        }
    }
}

pub struct Denoiser {
    df: DfTract,
    strength: Strength,
}

impl Denoiser {
    pub fn new(strength: Strength) -> Result<Self> {
        Self::with_thresholds(strength, Thresholds::default())
    }

    pub fn with_thresholds(strength: Strength, t: Thresholds) -> Result<Self> {
        let rp = RuntimeParams::default_with_ch(1)
            .with_atten_lim(strength.atten_limit_db())
            .with_post_filter(strength.post_filter_beta())
            .with_thresholds(t.min_db, t.max_erb_db, t.max_df_db);
        let df = DfTract::new(DfParams::default(), &rp)
            .map_err(|e| anyhow!("DeepFilterNet could not be loaded: {e}"))?;
        if df.sr != SAMPLE_RATE as usize || df.hop_size != HOP || df.ch != 1 {
            return Err(anyhow!(
                "unexpected model configuration: {} Hz, hop {}, {} channels",
                df.sr,
                df.hop_size,
                df.ch
            ));
        }
        Ok(Denoiser { df, strength })
    }

    pub fn strength(&self) -> Strength {
        self.strength
    }

    pub fn set_strength(&mut self, strength: Strength) {
        self.strength = strength;
        self.df.set_atten_lim(strength.atten_limit_db());
        self.df.set_pf_beta(strength.post_filter_beta());
    }

    /// Algorithmic latency in samples (STFT overlap plus the model's look-ahead).
    pub fn latency_samples(&self) -> usize {
        (self.df.fft_size - self.df.hop_size)
            + self.df.lookahead.max(self.df.conv_lookahead) * self.df.hop_size
    }

    /// Process one hop. Returns the model's local SNR estimate in dB.
    pub fn process_hop(&mut self, input: &[f32], output: &mut [f32]) -> Result<f32> {
        debug_assert_eq!(input.len(), HOP);
        debug_assert_eq!(output.len(), HOP);
        let inp =
            ArrayView2::from_shape((1, HOP), input).map_err(|e| anyhow!("input shape: {e}"))?;
        let out = ArrayViewMut2::from_shape((1, HOP), output)
            .map_err(|e| anyhow!("output shape: {e}"))?;
        self.df
            .process(inp, out)
            .map_err(|e| anyhow!("DeepFilterNet: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::rms_db;

    /// Deterministic white noise so the test does not need a `rand` dependency.
    fn noise(n: usize, amp: f32, seed: &mut u32) -> Vec<f32> {
        (0..n)
            .map(|_| {
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                amp * ((*seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0)
            })
            .collect()
    }

    fn run(d: &mut Denoiser, x: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0; x.len()];
        for (i, o) in x.chunks_exact(HOP).zip(out.chunks_exact_mut(HOP)) {
            d.process_hop(i, o).unwrap();
        }
        out
    }

    #[test]
    fn default_thresholds_keep_deep_filtering_on_for_loud_speech() {
        // Regression guard for the "underwater" bug: stage 2 must run up to 35 dB local SNR.
        let t = Thresholds::default();
        assert!(t.max_df_db >= 35.0 && t.max_erb_db >= 35.0);
        assert!(t.min_db <= -15.0);
    }

    #[test]
    fn strength_mapping_is_monotonic() {
        let v: Vec<f32> = Strength::ALL.iter().map(|s| s.atten_limit_db()).collect();
        assert!(v.windows(2).all(|w| w[0] < w[1]));
        for s in Strength::ALL {
            assert_eq!(Strength::parse(s.as_str()), Some(s));
            assert_eq!(Strength::ALL[s.index()], s);
        }
    }

    #[test]
    fn white_noise_is_suppressed_and_stronger_levels_suppress_more() {
        let mut seed = 1;
        let x = noise(HOP * 300, 0.1, &mut seed); // 3 s of -21 dB white noise
        let mut light = Denoiser::new(Strength::Light).unwrap();
        let mut max = Denoiser::new(Strength::Max).unwrap();
        let tail = HOP * 200..;
        let l = rms_db(&run(&mut light, &x)[tail.clone()]);
        let m = rms_db(&run(&mut max, &x)[tail]);
        let input = rms_db(&x);
        assert!(l < input - 6.0, "light only reduced {} dB", input - l);
        assert!(
            m < l - 3.0,
            "max ({m}) should be clearly quieter than light ({l})"
        );
    }

    #[test]
    fn latency_is_reasonable() {
        let d = Denoiser::new(Strength::High).unwrap();
        let ms = d.latency_samples() as f32 * 1000.0 / SAMPLE_RATE as f32;
        assert!((10.0..=60.0).contains(&ms), "{ms} ms");
    }
}
