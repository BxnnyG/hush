//! Offline echo-canceller check: `aec_file ref.f32 mic.f32 out.f32` (raw mono f32le, 48 kHz).
use sordino_core::echo::Echo;
use sordino_core::HOP;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let read = |p: &str| -> std::io::Result<Vec<f32>> {
        Ok(std::fs::read(p)?
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect())
    };
    let reference = read(&args[1])?;
    let mut mic = read(&args[2])?;
    let mut echo = Echo::new()?;
    let n = reference.len().min(mic.len()) / HOP * HOP;
    for (r, c) in reference[..n]
        .chunks_exact(HOP)
        .zip(mic[..n].chunks_exact_mut(HOP))
    {
        echo.process(r, c)?;
    }
    let bytes: Vec<u8> = mic[..n].iter().flat_map(|v| v.to_le_bytes()).collect();
    std::fs::write(&args[3], bytes)?;
    Ok(())
}
