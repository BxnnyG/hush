//! The echo canceller must remove a delayed, attenuated copy of the reference from the capture.

#![cfg(feature = "echo")]

use sordino_core::echo::Echo;
use sordino_core::level::rms_db;
use sordino_core::{HOP, SAMPLE_RATE};

/// Deterministic speech-like signal: band-limited noise with a syllable-rate envelope.
fn far_end(n: usize, seed: u32) -> Vec<f32> {
    let mut s = seed;
    let mut lp = 0.0f32;
    (0..n)
        .map(|i| {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let white = (s >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
            lp += 0.35 * (white - lp);
            let t = i as f32 / SAMPLE_RATE as f32;
            let env = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 3.5 * t).sin().max(-0.2);
            0.5 * env * lp
        })
        .collect()
}

fn run(delay_ms: usize, gain: f32, near: Option<&[f32]>, seconds: usize) -> (Vec<f32>, Vec<f32>) {
    let n = SAMPLE_RATE as usize * seconds;
    let reference = far_end(n, 42);
    let delay = delay_ms * SAMPLE_RATE as usize / 1000;
    let mut capture: Vec<f32> = (0..n)
        .map(|i| {
            if i >= delay {
                gain * reference[i - delay]
            } else {
                0.0
            }
        })
        .collect();
    if let Some(near) = near {
        for (c, v) in capture.iter_mut().zip(near) {
            *c += v;
        }
    }
    let mut echo = Echo::new().expect("webrtc-audio-processing available");
    let mut out = capture.clone();
    for (r, c) in reference.chunks_exact(HOP).zip(out.chunks_exact_mut(HOP)) {
        echo.process(r, c).unwrap();
    }
    (capture, out)
}

#[test]
fn pure_echo_is_strongly_reduced() {
    let (capture, out) = run(70, 0.5, None, 10);
    let tail = SAMPLE_RATE as usize * 7..;
    let reduction = rms_db(&capture[tail.clone()]) - rms_db(&out[tail]);
    assert!(reduction > 15.0, "echo only reduced by {reduction:.1} dB");
}

#[test]
fn near_end_speech_survives() {
    // A second, independent talker at the microphone must not be cancelled.
    let near: Vec<f32> = far_end(SAMPLE_RATE as usize * 12, 7)
        .into_iter()
        .map(|v| v * 0.8)
        .collect();
    let (_, out) = run(70, 0.5, Some(&near), 12);
    let tail = SAMPLE_RATE as usize * 8..SAMPLE_RATE as usize * 12;
    let kept = rms_db(&out[tail.clone()]) - rms_db(&near[tail]);
    assert!(
        kept > -6.0,
        "near-end speech was attenuated by {:.1} dB",
        -kept
    );
}
