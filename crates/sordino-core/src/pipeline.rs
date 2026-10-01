//! Echo cancel -> denoise -> studio chain, one hop at a time, with click-free on/off switching.

use anyhow::Result;

use crate::denoise::{Denoiser, Strength, Thresholds};
use crate::echo::Echo;
use crate::studio::{StudioChain, StudioParams};
use crate::HOP;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipelineParams {
    pub echo: bool,
    pub noise: bool,
    pub strength: Strength,
    /// `None` = studio sound off.
    pub studio: Option<StudioParams>,
}

impl Default for PipelineParams {
    fn default() -> Self {
        PipelineParams {
            echo: false,
            noise: true,
            strength: Strength::High,
            studio: StudioParams::default().into(),
        }
    }
}

pub struct Pipeline {
    echo: Option<Echo>,
    denoiser: Denoiser,
    studio: StudioChain,
    params: PipelineParams,
    /// Current wet/dry mix (0 = dry, 1 = processed) of each stage, ramped over one hop.
    echo_mix: f32,
    noise_mix: f32,
    studio_mix: f32,
    stage: [f32; HOP],
    scratch: [f32; HOP],
    /// The caller is falling behind real time: skip the expensive noise stage until it catches up.
    overloaded: bool,
}

const SILENCE: [f32; HOP] = [0.0; HOP];

impl Pipeline {
    pub fn new(params: PipelineParams) -> Result<Self> {
        Self::with_thresholds(params, Thresholds::default())
    }

    pub fn with_thresholds(params: PipelineParams, thresholds: Thresholds) -> Result<Self> {
        let denoiser = Denoiser::with_thresholds(params.strength, thresholds)?;
        let studio = StudioChain::new(params.studio.unwrap_or_default());
        // A missing echo canceller is not fatal: everything else keeps working.
        let echo = match Echo::new() {
            Ok(e) => Some(e),
            Err(e) => {
                log::info!("echo cancellation unavailable: {e}");
                None
            }
        };
        Ok(Pipeline {
            echo,
            denoiser,
            studio,
            params,
            echo_mix: 0.0,
            noise_mix: if params.noise { 1.0 } else { 0.0 },
            studio_mix: if params.studio.is_some() { 1.0 } else { 0.0 },
            stage: [0.0; HOP],
            scratch: [0.0; HOP],
            overloaded: false,
        })
    }

    /// Tell the pipeline that the caller is behind real time. While set, the noise stage is
    /// bypassed (with the usual crossfade) so the backlog can be worked off cheaply.
    pub fn set_overloaded(&mut self, overloaded: bool) {
        self.overloaded = overloaded;
    }

    pub fn params(&self) -> PipelineParams {
        self.params
    }

    /// Debug summary of the echo canceller, if there is one.
    pub fn echo_stats(&self) -> Option<String> {
        self.echo.as_ref().map(Echo::stats)
    }

    /// Whether echo cancellation can be switched on.
    pub fn echo_available(&self) -> bool {
        self.echo.is_some()
    }

    pub fn set_params(&mut self, p: PipelineParams) {
        if p.strength != self.params.strength {
            self.denoiser.set_strength(p.strength);
        }
        if let Some(s) = p.studio {
            self.studio.set_params(s);
            if self.studio_mix == 0.0 {
                // Coming back from "off": do not start from stale filter state.
                self.studio.reset();
            }
        }
        self.params = p;
    }

    /// Latency in samples of the currently active stages.
    pub fn latency_samples(&self) -> usize {
        let noise = if self.params.noise {
            self.denoiser.latency_samples()
        } else {
            0
        };
        let studio = if self.params.studio.is_some() {
            self.studio.latency()
        } else {
            0
        };
        noise + studio
    }

    /// Process one hop. `reference` is what is currently being played (for the echo canceller);
    /// pass `None` when there is none. `output` is always written: if a stage fails, its input is
    /// passed through and the error is returned so the caller can report it.
    pub fn process(
        &mut self,
        input: &[f32],
        reference: Option<&[f32]>,
        output: &mut [f32],
    ) -> Result<()> {
        debug_assert_eq!(input.len(), HOP);
        debug_assert_eq!(output.len(), HOP);
        self.stage.copy_from_slice(input);
        let mut result = Ok(());

        // Stage 0: echo cancellation.
        let echo_target = if self.params.echo && self.echo.is_some() {
            1.0
        } else {
            0.0
        };
        if self.echo_mix > 0.0 || echo_target > 0.0 {
            if let Some(echo) = self.echo.as_mut() {
                self.scratch.copy_from_slice(&self.stage);
                match echo.process(reference.unwrap_or(&SILENCE), &mut self.scratch) {
                    Ok(()) => crossfade(&mut self.stage, &self.scratch, self.echo_mix, echo_target),
                    Err(e) => result = Err(e),
                }
            }
            self.echo_mix = echo_target;
        }

        // Stage 1: noise suppression.
        let noise_target = if self.params.noise && !self.overloaded {
            1.0
        } else {
            0.0
        };
        if self.noise_mix > 0.0 || noise_target > 0.0 {
            match self.denoiser.process_hop(&self.stage, &mut self.scratch) {
                Ok(_) => crossfade(&mut self.stage, &self.scratch, self.noise_mix, noise_target),
                Err(e) => result = Err(e),
            }
            self.noise_mix = noise_target;
        }

        // Stage 2: studio chain.
        let studio_target = if self.params.studio.is_some() {
            1.0
        } else {
            0.0
        };
        if self.studio_mix > 0.0 || studio_target > 0.0 {
            self.scratch.copy_from_slice(&self.stage);
            self.studio.process(&mut self.scratch);
            crossfade(
                &mut self.stage,
                &self.scratch,
                self.studio_mix,
                studio_target,
            );
            self.studio_mix = studio_target;
        }

        // Never hand NaN/inf to the graph (a single one can poison downstream filters).
        for (o, s) in output.iter_mut().zip(self.stage.iter()) {
            *o = if s.is_finite() { *s } else { 0.0 };
        }
        result
    }
}

/// In place: `dry = dry + (wet - dry) * mix`, with `mix` ramping linearly from `from` to `to`.
fn crossfade(dry: &mut [f32], wet: &[f32], from: f32, to: f32) {
    let n = dry.len() as f32;
    for (i, d) in dry.iter_mut().enumerate() {
        let m = from + (to - from) * (i as f32 + 1.0) / n;
        *d += (wet[i] - *d) * m;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::rms_db;

    fn tone(n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin())
            .collect()
    }

    fn run(p: &mut Pipeline, x: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0; x.len()];
        for (i, o) in x.chunks_exact(HOP).zip(out.chunks_exact_mut(HOP)) {
            p.process(i, None, o).unwrap();
        }
        out
    }

    fn off() -> PipelineParams {
        PipelineParams {
            echo: false,
            noise: false,
            strength: Strength::High,
            studio: None,
        }
    }

    #[test]
    fn everything_off_is_bit_exact_passthrough() {
        let mut p = Pipeline::new(off()).unwrap();
        let x = tone(HOP * 20);
        assert_eq!(run(&mut p, &x), x);
        assert_eq!(p.latency_samples(), 0);
    }

    #[test]
    fn toggling_noise_has_no_hard_discontinuity() {
        let mut p = Pipeline::new(PipelineParams {
            noise: true,
            ..off()
        })
        .unwrap();
        let x = tone(HOP * 100);
        let mut out = run(&mut p, &x[..HOP * 50]);
        let mut params = p.params();
        params.noise = false;
        p.set_params(params);
        out.extend(run(&mut p, &x[HOP * 50..]));
        // DeepFilterNet's delay makes dry/wet differ in phase, so require only that the
        // switch does not produce a spike beyond what the signal itself could.
        assert!(out.iter().all(|v| v.abs() < 0.6));
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn output_is_always_finite_and_studio_changes_loudness_bounded() {
        let mut p = Pipeline::new(PipelineParams::default()).unwrap();
        let x = tone(HOP * 100);
        let out = run(&mut p, &x);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(rms_db(&out) < 0.0);
        assert!(p.latency_samples() > 0);
    }

    #[test]
    fn echo_flag_without_canceller_is_harmless() {
        let mut p = Pipeline::new(PipelineParams {
            echo: true,
            ..off()
        })
        .unwrap();
        let x = tone(HOP * 10);
        let out = run(&mut p, &x);
        if !p.echo_available() {
            assert_eq!(out, x);
        }
        assert!(out.iter().all(|v| v.is_finite()));
    }
}

#[cfg(test)]
mod overload_tests {
    use super::*;

    #[test]
    fn overload_bypasses_the_noise_stage_and_recovers() {
        let mut p = Pipeline::new(PipelineParams {
            echo: false,
            noise: true,
            strength: Strength::High,
            studio: None,
        })
        .unwrap();
        let x: Vec<f32> = (0..HOP * 40)
            .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 300.0 * i as f32 / 48000.0).sin())
            .collect();
        let mut out = vec![0.0; x.len()];
        p.set_overloaded(true);
        for (i, o) in x.chunks_exact(HOP).zip(out.chunks_exact_mut(HOP)).take(20) {
            p.process(i, None, o).unwrap();
        }
        // While overloaded the signal passes through untouched (after the fade-out hop).
        assert_eq!(&out[HOP * 2..HOP * 20], &x[HOP * 2..HOP * 20]);
        p.set_overloaded(false);
        for (i, o) in x.chunks_exact(HOP).zip(out.chunks_exact_mut(HOP)).skip(20) {
            p.process(i, None, o).unwrap();
        }
        assert!(out.iter().all(|v| v.is_finite()));
    }
}
