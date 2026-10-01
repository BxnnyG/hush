//! Measures what the plan asks for in M0: CPU cost and algorithmic latency of the pipeline.
//!
//! `cargo run --release -p sordino-core --example bench`

use std::time::Instant;

use sordino_core::denoise::Strength;
use sordino_core::pipeline::{Pipeline, PipelineParams};
use sordino_core::studio::StudioParams;
use sordino_core::{HOP, SAMPLE_RATE};

fn main() -> anyhow::Result<()> {
    let hops = 3000; // 30 s of audio
    let mut seed = 7u32;
    let mut rnd = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    };
    // Speech-ish test signal: bursts of a harmonic tone stack over white noise.
    let input: Vec<f32> = (0..hops * HOP)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let on = if (t * 2.5).fract() < 0.6 { 1.0 } else { 0.0 };
            let voice: f32 = (1..6)
                .map(|h| (2.0 * std::f32::consts::PI * 140.0 * h as f32 * t).sin() / h as f32)
                .sum();
            0.15 * on * voice + 0.03 * rnd()
        })
        .collect();

    for (label, params) in [
        (
            "noise only (high)",
            PipelineParams {
                echo: false,
                noise: true,
                strength: Strength::High,
                studio: None,
            },
        ),
        (
            "noise + studio",
            PipelineParams {
                echo: false,
                noise: true,
                strength: Strength::High,
                studio: Some(StudioParams::default()),
            },
        ),
        (
            "studio only",
            PipelineParams {
                echo: false,
                noise: false,
                strength: Strength::High,
                studio: Some(StudioParams::default()),
            },
        ),
    ] {
        let mut p = Pipeline::new(params)?;
        let mut out = vec![0.0f32; input.len()];
        // warm-up so one-off allocations do not count
        for (i, o) in input
            .chunks_exact(HOP)
            .zip(out.chunks_exact_mut(HOP))
            .take(50)
        {
            p.process(i, None, o)?;
        }
        let mut worst = 0.0f64;
        let start = Instant::now();
        for (i, o) in input.chunks_exact(HOP).zip(out.chunks_exact_mut(HOP)) {
            let t = Instant::now();
            p.process(i, None, o)?;
            worst = worst.max(t.elapsed().as_secs_f64());
        }
        let total = start.elapsed().as_secs_f64();
        let audio = hops as f64 * HOP as f64 / SAMPLE_RATE as f64;
        println!(
            "{label:<20} cpu {:5.1} % of one core   worst hop {:5.2} ms (budget 10 ms)   algorithmic latency {:.1} ms",
            total / audio * 100.0,
            worst * 1000.0,
            p.latency_samples() as f64 * 1000.0 / SAMPLE_RATE as f64
        );
    }

    Ok(())
}
