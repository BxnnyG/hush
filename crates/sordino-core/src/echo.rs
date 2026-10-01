//! Acoustic echo cancellation with WebRTC's AEC3.
//!
//! Only useful with speakers: the microphone hears what the speakers play. The canceller needs
//! the *reference* (what is being played) next to the microphone signal. WebRTC estimates the
//! delay between the two itself and tolerates moderate clock drift.
//!
//! Built only with the `echo` feature; without it [`Echo::new`] reports that it is unavailable.

/// Whether this build contains an echo canceller.
pub const AVAILABLE: bool = cfg!(feature = "echo");

#[cfg(feature = "echo")]
mod imp {
    use anyhow::{anyhow, Result};
    use webrtc_audio_processing::Processor;
    use webrtc_audio_processing_config::{Config, EchoCanceller, HighPassFilter};

    use crate::{HOP, SAMPLE_RATE};

    pub struct Echo {
        ap: Processor,
        render: [f32; HOP],
    }

    impl Echo {
        pub fn new() -> Result<Echo> {
            let ap = Processor::new(SAMPLE_RATE).map_err(|e| anyhow!("echo canceller: {e}"))?;
            // Only the echo canceller (plus the high-pass filter WebRTC recommends with it).
            // Noise suppression is DeepFilterNet's job, gain control is deliberately off.
            ap.set_config(Config {
                echo_canceller: Some(EchoCanceller::default()),
                high_pass_filter: Some(HighPassFilter::default()),
                ..Default::default()
            });
            if ap.num_samples_per_frame() != HOP {
                return Err(anyhow!(
                    "echo canceller frame size {} does not match hop {HOP}",
                    ap.num_samples_per_frame()
                ));
            }
            Ok(Echo {
                ap,
                render: [0.0; HOP],
            })
        }

        /// One-line summary of the canceller's internal estimates, for debug logs.
        pub fn stats(&self) -> String {
            let s = self.ap.get_stats();
            format!(
                "delay {:?} ms, ERL {:?} dB, ERLE {:?} dB",
                s.delay_ms,
                s.echo_return_loss.map(f64::round),
                s.echo_return_loss_enhancement.map(f64::round)
            )
        }

        /// Remove the echo of `reference` from `capture`, one hop.
        pub fn process(&mut self, reference: &[f32], capture: &mut [f32]) -> Result<()> {
            debug_assert_eq!(reference.len(), HOP);
            debug_assert_eq!(capture.len(), HOP);
            self.render.copy_from_slice(reference);
            self.ap
                .process_render_frame([&mut self.render[..]])
                .map_err(|e| anyhow!("echo canceller (render): {e}"))?;
            self.ap
                .process_capture_frame([&mut capture[..]])
                .map_err(|e| anyhow!("echo canceller (capture): {e}"))
        }
    }
}

#[cfg(not(feature = "echo"))]
mod imp {
    use anyhow::{anyhow, Result};

    pub struct Echo;

    impl Echo {
        pub fn new() -> Result<Echo> {
            Err(anyhow!("this build of Sordino has no echo cancellation"))
        }

        pub fn process(&mut self, _reference: &[f32], _capture: &mut [f32]) -> Result<()> {
            Ok(())
        }

        pub fn stats(&self) -> String {
            String::new()
        }
    }
}

pub use imp::Echo;
