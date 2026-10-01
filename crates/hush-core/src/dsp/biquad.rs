//! RBJ audio-EQ-cookbook biquads (transposed direct form II, f64 state).

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterKind {
    HighPass,
    LowPass,
    LowShelf,
    HighShelf,
    Peaking,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coeffs {
    pub const PASS: Coeffs = Coeffs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    pub fn new(kind: FilterKind, freq: f32, q: f32, gain_db: f32, sample_rate: f32) -> Coeffs {
        let fs = sample_rate as f64;
        // Keep the frequency safely below Nyquist so the filter stays stable.
        let f0 = (freq as f64).clamp(10.0, fs * 0.45);
        let q = (q as f64).max(0.05);
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let w0 = 2.0 * PI * f0 / fs;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let (b0, b1, b2, a0, a1, a2) = match kind {
            FilterKind::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::LowPass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::Peaking => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            FilterKind::LowShelf => {
                let beta = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + beta),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - beta),
                    (a + 1.0) + (a - 1.0) * cos + beta,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - beta,
                )
            }
            FilterKind::HighShelf => {
                let beta = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + beta),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - beta),
                    (a + 1.0) - (a - 1.0) * cos + beta,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - beta,
                )
            }
        };
        Coeffs {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Magnitude response in dB at `freq` (used by tests and for sanity checks).
    pub fn response_db(&self, freq: f32, sample_rate: f32) -> f32 {
        let w = 2.0 * PI * freq as f64 / sample_rate as f64;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        let mag = ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt();
        (20.0 * mag.max(1e-12).log10()) as f32
    }
}

#[derive(Clone, Debug)]
pub struct Biquad {
    c: Coeffs,
    z1: f64,
    z2: f64,
}

impl Default for Biquad {
    fn default() -> Self {
        Biquad {
            c: Coeffs::PASS,
            z1: 0.0,
            z2: 0.0,
        }
    }
}

impl Biquad {
    pub fn new(c: Coeffs) -> Self {
        Biquad {
            c,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// Swap coefficients but keep the filter state, so parameter changes do not click.
    pub fn set_coeffs(&mut self, c: Coeffs) {
        self.c = c;
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    #[inline]
    pub fn process_sample(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.c.b0 * x + self.z1;
        self.z1 = self.c.b1 * x - self.c.a1 * y + self.z2;
        self.z2 = self.c.b2 * x - self.c.a2 * y;
        y as f32
    }

    pub fn process(&mut self, block: &mut [f32]) {
        for s in block {
            *s = self.process_sample(*s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    #[test]
    fn highpass_attenuates_below_corner() {
        let c = Coeffs::new(FilterKind::HighPass, 100.0, 0.707, 0.0, FS);
        // 12 dB/octave: two octaves below the corner is about -24 dB plus the knee
        assert!(c.response_db(20.0, FS) < -25.0);
        assert!((c.response_db(100.0, FS) + 3.0).abs() < 0.3);
        assert!(c.response_db(2000.0, FS).abs() < 0.1);
    }

    #[test]
    fn shelves_reach_their_gain() {
        let lo = Coeffs::new(FilterKind::LowShelf, 200.0, 0.707, 6.0, FS);
        assert!((lo.response_db(30.0, FS) - 6.0).abs() < 0.3);
        assert!(lo.response_db(8000.0, FS).abs() < 0.2);
        let hi = Coeffs::new(FilterKind::HighShelf, 8000.0, 0.707, -6.0, FS);
        assert!((hi.response_db(20000.0, FS) + 6.0).abs() < 0.5);
        assert!(hi.response_db(200.0, FS).abs() < 0.2);
    }

    #[test]
    fn peaking_hits_gain_at_center() {
        let p = Coeffs::new(FilterKind::Peaking, 3000.0, 1.0, 4.0, FS);
        assert!((p.response_db(3000.0, FS) - 4.0).abs() < 0.05);
        assert!(p.response_db(100.0, FS).abs() < 0.3);
    }

    #[test]
    fn zero_gain_shelf_and_peak_are_transparent() {
        for kind in [
            FilterKind::LowShelf,
            FilterKind::HighShelf,
            FilterKind::Peaking,
        ] {
            let c = Coeffs::new(kind, 1000.0, 0.9, 0.0, FS);
            for f in [50.0, 500.0, 5000.0, 15000.0] {
                assert!(c.response_db(f, FS).abs() < 0.01);
            }
        }
    }

    #[test]
    fn stays_finite_on_extreme_parameters() {
        let mut b = Biquad::new(Coeffs::new(FilterKind::HighPass, 1.0, 0.01, 0.0, FS));
        let mut x = [1.0f32; 4800];
        b.process(&mut x);
        assert!(x.iter().all(|v| v.is_finite()));
    }
}
