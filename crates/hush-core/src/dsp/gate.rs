//! Downward expander / noise gate with hysteresis and hold.
//!
//! The gate never mutes completely: `range_db` is the maximum attenuation, which sounds
//! far more natural than a hard cut and hides pumping.

use super::time_coef;
use crate::level::db_to_lin;

#[derive(Clone, Debug)]
pub struct Gate {
    threshold_lin: f32,
    close_lin: f32,
    range_lin: f32,
    hold_samples: u32,
    env_rel: f32,
    gain_attack: f32,
    gain_release: f32,
    env: f32,
    gain: f32,
    hold: u32,
    open: bool,
}

impl Gate {
    pub fn new(threshold_db: f32, range_db: f32, sample_rate: f32) -> Self {
        let mut g = Gate {
            threshold_lin: 0.0,
            close_lin: 0.0,
            range_lin: 0.0,
            hold_samples: (0.08 * sample_rate) as u32,
            env_rel: time_coef(20.0, sample_rate),
            gain_attack: time_coef(1.5, sample_rate),
            gain_release: time_coef(120.0, sample_rate),
            env: 0.0,
            gain: 1.0,
            hold: 0,
            open: true,
        };
        g.set(threshold_db, range_db);
        g
    }

    pub fn set(&mut self, threshold_db: f32, range_db: f32) {
        self.threshold_lin = db_to_lin(threshold_db);
        // 4 dB hysteresis so the gate does not chatter around the threshold.
        self.close_lin = db_to_lin(threshold_db - 4.0);
        self.range_lin = db_to_lin(-range_db.abs());
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
        self.gain = 1.0;
        self.hold = 0;
        self.open = true;
    }

    pub fn process(&mut self, block: &mut [f32]) {
        for s in block {
            let a = s.abs();
            // Peak follower: instant attack, smooth release.
            self.env = if a > self.env {
                a
            } else {
                self.env_rel * self.env + (1.0 - self.env_rel) * a
            };

            if self.env >= self.threshold_lin {
                self.open = true;
                self.hold = self.hold_samples;
            } else if self.env < self.close_lin {
                if self.hold > 0 {
                    self.hold -= 1;
                } else {
                    self.open = false;
                }
            }

            let target = if self.open { 1.0 } else { self.range_lin };
            let coef = if target > self.gain {
                self.gain_attack
            } else {
                self.gain_release
            };
            self.gain = coef * self.gain + (1.0 - coef) * target;
            *s *= self.gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::rms_db;

    fn sine(amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 300.0 * i as f32 / 48000.0).sin())
            .collect()
    }

    #[test]
    fn passes_loud_signal_and_attenuates_quiet() {
        let mut g = Gate::new(-45.0, 30.0, 48000.0);
        let mut loud = sine(0.3, 48000);
        g.process(&mut loud);
        assert!((rms_db(&loud[24000..]) - rms_db(&sine(0.3, 24000))).abs() < 0.5);

        let mut quiet = sine(0.001, 96000); // about -63 dB
        g.process(&mut quiet);
        let before = rms_db(&sine(0.001, 1000));
        assert!(rms_db(&quiet[80000..]) < before - 20.0);
    }

    #[test]
    fn range_limits_attenuation() {
        let mut g = Gate::new(-30.0, 12.0, 48000.0);
        let mut quiet = sine(0.001, 96000);
        g.process(&mut quiet);
        let before = rms_db(&sine(0.001, 1000));
        let after = rms_db(&quiet[80000..]);
        assert!(
            after > before - 13.0,
            "attenuated by more than range: {}",
            before - after
        );
    }
}
