//! Level metering helpers.

/// Floor used for silence, in dBFS.
pub const SILENCE_DB: f32 = -100.0;

/// Peak level of a block in dBFS.
pub fn peak_db(block: &[f32]) -> f32 {
    let peak = block.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
    lin_to_db(peak)
}

/// RMS level of a block in dBFS.
pub fn rms_db(block: &[f32]) -> f32 {
    if block.is_empty() {
        return SILENCE_DB;
    }
    let e: f32 = block.iter().map(|x| x * x).sum::<f32>() / block.len() as f32;
    lin_to_db(e.sqrt())
}

pub fn lin_to_db(x: f32) -> f32 {
    if x <= 1e-5 {
        SILENCE_DB
    } else {
        (20.0 * x.log10()).max(SILENCE_DB)
    }
}

pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_sine_rms_is_minus_3db() {
        let block: Vec<f32> = (0..4800)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48000.0).sin())
            .collect();
        assert!((rms_db(&block) + 3.01).abs() < 0.05);
        assert!(peak_db(&block).abs() < 0.01);
    }

    #[test]
    fn silence_is_floor() {
        assert_eq!(peak_db(&[0.0; 10]), SILENCE_DB);
        assert_eq!(rms_db(&[]), SILENCE_DB);
    }
}
