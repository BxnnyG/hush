//! Split-band de-esser.
//!
//! A 4th-order Linkwitz-Riley crossover splits the signal into a low and a high band whose sum
//! is an all-pass, so attenuating only the high band is well-behaved. Sibilance is detected by
//! comparing the high-band level against the broadband level, so it works independent of mic
//! gain: `out = low + high * gain`.

use super::biquad::{Biquad, Coeffs, FilterKind};
use super::time_coef;

#[derive(Clone, Debug)]
pub struct DeEsser {
    hp: [Biquad; 2],
    lp: [Biquad; 2],
    /// Maximum tolerated ratio of high-band to broadband level (linear).
    threshold_ratio: f32,
    /// 0..1, how hard to pull sibilance down.
    amount: f32,
    env_hp: f32,
    env_full: f32,
    att: f32,
    rel: f32,
    gain: f32,
    gain_att: f32,
    gain_rel: f32,
}

impl DeEsser {
    pub fn new(freq: f32, amount: f32, sample_rate: f32) -> Self {
        let hp = Coeffs::new(
            FilterKind::HighPass,
            freq,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            sample_rate,
        );
        let lp = Coeffs::new(
            FilterKind::LowPass,
            freq,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            sample_rate,
        );
        DeEsser {
            hp: [Biquad::new(hp), Biquad::new(hp)],
            lp: [Biquad::new(lp), Biquad::new(lp)],
            threshold_ratio: 0.45,
            amount: amount.clamp(0.0, 1.0),
            env_hp: 0.0,
            env_full: 0.0,
            att: time_coef(0.5, sample_rate),
            rel: time_coef(25.0, sample_rate),
            gain: 1.0,
            gain_att: time_coef(0.6, sample_rate),
            gain_rel: time_coef(15.0, sample_rate),
        }
    }

    pub fn set_amount(&mut self, amount: f32) {
        self.amount = amount.clamp(0.0, 1.0);
    }

    pub fn reset(&mut self) {
        for b in self.hp.iter_mut().chain(self.lp.iter_mut()) {
            b.reset();
        }
        self.env_hp = 0.0;
        self.env_full = 0.0;
        self.gain = 1.0;
    }

    pub fn process(&mut self, block: &mut [f32]) {
        if self.amount <= 0.0 {
            return;
        }
        for s in block {
            let x = *s;
            let h = self.hp[0].process_sample(x);
            let hp = self.hp[1].process_sample(h);
            let l = self.lp[0].process_sample(x);
            let lp = self.lp[1].process_sample(l);
            let (a_hp, a_full) = (hp.abs(), x.abs());
            self.env_hp = if a_hp > self.env_hp {
                self.att * self.env_hp + (1.0 - self.att) * a_hp
            } else {
                self.rel * self.env_hp + (1.0 - self.rel) * a_hp
            };
            self.env_full = if a_full > self.env_full {
                self.att * self.env_full + (1.0 - self.att) * a_full
            } else {
                self.rel * self.env_full + (1.0 - self.rel) * a_full
            };

            // Ignore near-silence, the ratio is meaningless there.
            let target = if self.env_full > 1e-4 {
                let ratio = self.env_hp / self.env_full;
                if ratio > self.threshold_ratio {
                    (self.threshold_ratio / ratio).powf(self.amount * 2.0)
                } else {
                    1.0
                }
            } else {
                1.0
            };
            let coef = if target < self.gain {
                self.gain_att
            } else {
                self.gain_rel
            };
            self.gain = coef * self.gain + (1.0 - coef) * target;
            *s = lp + hp * self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::rms_db;

    fn tone(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / 48000.0).sin())
            .collect()
    }

    #[test]
    fn sibilant_tone_is_attenuated() {
        let mut d = DeEsser::new(5500.0, 1.0, 48000.0);
        let mut x = tone(8000.0, 0.3, 48000);
        d.process(&mut x);
        assert!(rms_db(&x[40000..]) < rms_db(&tone(8000.0, 0.3, 4800)) - 6.0);
    }

    #[test]
    fn low_tone_is_untouched() {
        let mut d = DeEsser::new(5500.0, 1.0, 48000.0);
        let mut x = tone(500.0, 0.3, 48000);
        d.process(&mut x);
        assert!((rms_db(&x[40000..]) - rms_db(&tone(500.0, 0.3, 4800))).abs() < 0.3);
    }

    #[test]
    fn zero_amount_is_bypass() {
        let mut d = DeEsser::new(5500.0, 0.0, 48000.0);
        let orig = tone(8000.0, 0.3, 4800);
        let mut x = orig.clone();
        d.process(&mut x);
        assert_eq!(x, orig);
    }
}
