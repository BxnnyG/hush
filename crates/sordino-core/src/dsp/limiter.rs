//! Lookahead peak limiter (1 ms). Guarantees `|out| <= ceiling`.
//!
//! Gain = moving-minimum of the required gain over the lookahead window, smoothed by a box
//! filter of the same length (so the gain ramps down before the peak arrives), then a release
//! stage that can only slow down the *rise* of the gain.

use super::time_coef;
use crate::level::db_to_lin;

const LOOKAHEAD: usize = 48; // 1 ms at 48 kHz

#[derive(Clone, Debug)]
pub struct Limiter {
    ceiling: f32,
    release: f32,
    /// Delayed audio.
    audio: [f32; LOOKAHEAD],
    /// Required gain per sample for the same window.
    need: [f32; LOOKAHEAD],
    /// Window of min-filtered gains for the box filter.
    mins: [f32; LOOKAHEAD],
    pos: usize,
    env: f32,
}

impl Limiter {
    pub fn new(ceiling_db: f32, release_ms: f32, sample_rate: f32) -> Self {
        Limiter {
            ceiling: db_to_lin(ceiling_db),
            release: time_coef(release_ms, sample_rate),
            audio: [0.0; LOOKAHEAD],
            need: [1.0; LOOKAHEAD],
            mins: [1.0; LOOKAHEAD],
            pos: 0,
            env: 1.0,
        }
    }

    pub const fn latency() -> usize {
        LOOKAHEAD - 1
    }

    pub fn reset(&mut self) {
        self.audio = [0.0; LOOKAHEAD];
        self.need = [1.0; LOOKAHEAD];
        self.mins = [1.0; LOOKAHEAD];
        self.env = 1.0;
        self.pos = 0;
    }

    pub fn process(&mut self, block: &mut [f32]) {
        for s in block {
            let x = *s;
            let a = x.abs();
            let need = if a > self.ceiling {
                self.ceiling / a
            } else {
                1.0
            };

            // Write first, then read the sample written LOOKAHEAD-1 steps ago: every one of the
            // LOOKAHEAD window minima averaged below then covers that sample.
            self.audio[self.pos] = x;
            self.need[self.pos] = need;
            let delayed = self.audio[(self.pos + 1) % LOOKAHEAD];

            // Minimum of required gains over the window (includes the newest sample).
            let min = self.need.iter().fold(1.0f32, |m, &g| m.min(g));
            self.mins[self.pos] = min;
            // Box-filtered gain: average of the last LOOKAHEAD window minima.
            let boxed = self.mins.iter().sum::<f32>() / LOOKAHEAD as f32;

            self.env = if boxed < self.env {
                boxed
            } else {
                self.release * self.env + (1.0 - self.release) * boxed
            };
            // Numerical safety net: never exceed the ceiling.
            *s = (delayed * self.env).clamp(-self.ceiling, self.ceiling);

            self.pos = (self.pos + 1) % LOOKAHEAD;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_exceeds_ceiling() {
        let mut l = Limiter::new(-1.0, 50.0, 48000.0);
        let ceiling = db_to_lin(-1.0);
        let mut x: Vec<f32> = (0..48000)
            .map(|i| 3.0 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 48000.0).sin())
            .collect();
        // plus a burst of full-scale clicks
        for i in (0..48000).step_by(997) {
            x[i] = 5.0;
        }
        l.process(&mut x);
        assert!(x.iter().all(|v| v.abs() <= ceiling + 1e-6));
    }

    #[test]
    fn quiet_signal_passes_unchanged_but_delayed() {
        let mut l = Limiter::new(-1.0, 50.0, 48000.0);
        let orig: Vec<f32> = (0..4800)
            .map(|i| 0.1 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect();
        let mut x = orig.clone();
        l.process(&mut x);
        let d = Limiter::latency();
        for i in d..4800 {
            assert!((x[i] - orig[i - d]).abs() < 1e-6, "sample {i}");
        }
    }

    #[test]
    fn recovers_after_peak() {
        let mut l = Limiter::new(-1.0, 20.0, 48000.0);
        let mut x = vec![0.1f32; 48000];
        x[1000] = 4.0;
        l.process(&mut x);
        assert!((x[47000] - 0.1).abs() < 0.001);
    }
}
