//! Small, allocation-free DSP building blocks. All of them work on mono f32 at 48 kHz.

pub mod biquad;
pub mod compressor;
pub mod deesser;
pub mod gate;
pub mod limiter;

/// One-pole smoothing coefficient for a time constant in milliseconds.
/// `y += (1 - coef) * (x - y)` reaches ~63 % of a step after `ms`.
pub(crate) fn time_coef(ms: f32, sample_rate: f32) -> f32 {
    if ms <= 0.0 {
        0.0
    } else {
        (-1.0 / (ms * 0.001 * sample_rate)).exp()
    }
}
