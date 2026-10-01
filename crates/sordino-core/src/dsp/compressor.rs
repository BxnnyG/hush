//! Feed-forward compressor with soft knee; detector and smoothing in the dB domain.

use super::time_coef;
use crate::level::db_to_lin;

#[derive(Clone, Debug)]
pub struct Compressor {
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
    makeup_lin: f32,
    attack: f32,
    release: f32,
    /// Smoothed gain reduction in dB (>= 0).
    reduction_db: f32,
}

impl Compressor {
    pub fn new(
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_db: f32,
        sample_rate: f32,
    ) -> Self {
        Compressor {
            threshold_db,
            ratio: ratio.max(1.0),
            knee_db: 6.0,
            makeup_lin: db_to_lin(makeup_db),
            attack: time_coef(attack_ms, sample_rate),
            release: time_coef(release_ms, sample_rate),
            reduction_db: 0.0,
        }
    }

    pub fn set(&mut self, threshold_db: f32, ratio: f32, makeup_db: f32) {
        self.threshold_db = threshold_db;
        self.ratio = ratio.max(1.0);
        self.makeup_lin = db_to_lin(makeup_db);
    }

    pub fn reset(&mut self) {
        self.reduction_db = 0.0;
    }

    /// Static curve: gain reduction (dB, >= 0) for an input level.
    fn reduction_for(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold_db;
        let slope = 1.0 - 1.0 / self.ratio;
        if 2.0 * over < -self.knee_db {
            0.0
        } else if 2.0 * over.abs() <= self.knee_db {
            let x = over + self.knee_db / 2.0;
            slope * x * x / (2.0 * self.knee_db)
        } else {
            slope * over
        }
    }

    pub fn process(&mut self, block: &mut [f32]) {
        for s in block {
            let level_db = 20.0 * s.abs().max(1e-6).log10();
            let target = self.reduction_for(level_db);
            let coef = if target > self.reduction_db {
                self.attack
            } else {
                self.release
            };
            self.reduction_db = coef * self.reduction_db + (1.0 - coef) * target;
            *s *= db_to_lin(-self.reduction_db) * self.makeup_lin;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::rms_db;

    fn sine(amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 500.0 * i as f32 / 48000.0).sin())
            .collect()
    }

    #[test]
    fn steady_tone_is_reduced_per_ratio() {
        // peak -6 dBFS tone, threshold -26 => 20 dB over, ratio 4 => 15 dB reduction
        let mut c = Compressor::new(-26.0, 4.0, 5.0, 100.0, 0.0, 48000.0);
        let mut x = sine(0.5, 48000);
        c.process(&mut x);
        let reduced = rms_db(&sine(0.5, 4800)) - rms_db(&x[40000..]);
        assert!((reduced - 15.0).abs() < 2.0, "reduction was {reduced} dB");
    }

    #[test]
    fn below_threshold_is_untouched() {
        let mut c = Compressor::new(-20.0, 4.0, 5.0, 100.0, 0.0, 48000.0);
        let mut x = sine(0.01, 48000); // -40 dBFS
        c.process(&mut x);
        assert!((rms_db(&x[40000..]) - rms_db(&sine(0.01, 4800))).abs() < 0.2);
    }

    #[test]
    fn makeup_gain_applies() {
        let mut c = Compressor::new(0.0, 2.0, 5.0, 100.0, 6.0, 48000.0);
        let mut x = sine(0.01, 9600);
        c.process(&mut x);
        assert!((rms_db(&x[4800..]) - rms_db(&sine(0.01, 4800)) - 6.0).abs() < 0.2);
    }
}
