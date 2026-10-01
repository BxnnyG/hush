#!/usr/bin/env python3
"""Compare a raw microphone recording with the processed one and report what changed.

    python3 tools/analyze_recording.py ~/sordino-recordings/<timestamp>   (needs numpy scipy matplotlib soundfile)

Reads raw.wav and processed.wav, aligns them, and prints metrics that map to what people hear:
muffled ("underwater"), pumping/robotic, reverberant ("concert hall"), crackle, residual noise.
Writes report.png next to the files.
"""
import sys
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy import signal

FRAME = 0.02  # seconds
BANDS = [(0, 300), (300, 1000), (1000, 4000), (4000, 8000), (8000, 16000)]


def load(path):
    x, sr = sf.read(path, dtype="float32", always_2d=True)
    return x.mean(axis=1), sr


def frame_db(x, sr):
    n = int(FRAME * sr)
    k = len(x) // n
    f = x[: k * n].reshape(k, n)
    return 20 * np.log10(np.sqrt((f**2).mean(axis=1)) + 1e-9)


def align(raw, proc, sr):
    """Shift `proc` so it lines up with `raw` (cross-correlation of the 1 kHz-lowpassed envelope)."""
    d = 24
    a = signal.resample_poly(np.abs(raw), 1, d)
    b = signal.resample_poly(np.abs(proc), 1, d)
    m = min(len(a), len(b))
    a, b = a[:m] - a[:m].mean(), b[:m] - b[:m].mean()
    cc = signal.correlate(a, b, mode="full", method="fft")
    lag = (np.argmax(cc) - (m - 1)) * d  # samples: positive means proc is late
    lag = int(np.clip(lag, -sr // 2, sr // 2))
    if lag > 0:
        proc = proc[lag:]
    elif lag < 0:
        proc = np.concatenate([np.zeros(-lag, dtype=proc.dtype), proc])
    n = min(len(raw), len(proc))
    return raw[:n], proc[:n], lag * 1000 / sr


def band_db(x, sr):
    f, p = signal.welch(x, sr, nperseg=4096)
    return [10 * np.log10(p[(f >= lo) & (f < hi)].sum() + 1e-18) for lo, hi in BANDS]


def decay_time(db, speech, hop=FRAME):
    """Median time (s) for the level to fall 20 dB after speech ends (reverb tail length)."""
    times = []
    i = 1
    while i < len(speech):
        if speech[i - 1] and not speech[i]:
            peak = db[max(i - 3, 0) : i].max()
            for j in range(i, min(i + 100, len(db))):
                if db[j] < peak - 20:
                    times.append((j - i) * hop)
                    break
            else:
                times.append(100 * hop)
        i += 1
    return float(np.median(times)) if times else float("nan")


def main():
    d = Path(sys.argv[1]).expanduser()
    raw, sr = load(d / "raw.wav")
    proc, sr2 = load(d / "processed.wav")
    assert sr == sr2, "sample rates differ"
    raw, proc, lag_ms = align(raw, proc, sr)

    r, p = frame_db(raw, sr), frame_db(proc, sr)
    floor_r = np.percentile(r, 10)
    speech = r > floor_r + 18
    speech = np.convolve(speech.astype(float), np.ones(5), "same") > 0  # bridge short gaps
    quiet = np.convolve(speech.astype(float), np.ones(15), "same") == 0

    print(f"recording: {len(raw)/sr:.1f} s, alignment {lag_ms:+.0f} ms")
    print(f"speech frames: {speech.mean()*100:.0f} %   quiet frames: {quiet.mean()*100:.0f} %\n")

    print("LEVELS")
    print(f"  noise floor (quiet):  raw {np.median(r[quiet]):6.1f} dB   processed {np.median(p[quiet]):6.1f} dB   -> {np.median(r[quiet])-np.median(p[quiet]):.1f} dB removed")
    print(f"  speech level:         raw {np.median(r[speech]):6.1f} dB   processed {np.median(p[speech]):6.1f} dB   -> {np.median(p[speech])-np.median(r[speech]):+.1f} dB")
    pk = 20 * np.log10(np.abs(proc).max() + 1e-9)
    print(f"  processed peak {pk:.1f} dBFS" + ("   CLIPPING!" if pk > -0.2 else ""))

    # Speech band balance: "underwater" = highs missing relative to the mids.
    sp_idx = np.repeat(speech, int(FRAME * sr))[: len(raw)]
    sp_idx = np.concatenate([sp_idx, np.zeros(len(raw) - len(sp_idx), dtype=bool)])
    br, bp = band_db(raw[sp_idx], sr), band_db(proc[sp_idx], sr)
    print("\nSPEECH SPECTRUM CHANGE (processed - raw, dB)   <- 'underwater' if the high bands drop far below the mids")
    for (lo, hi), a, b in zip(BANDS, br, bp):
        print(f"  {lo:5d}-{hi:5d} Hz  {b-a:+6.1f}")
    tilt = (bp[3] - br[3]) - (bp[2] - br[2])
    print(f"  high-vs-mid tilt: {tilt:+.1f} dB  " + ("(muffled)" if tilt < -4 else "(ok)"))

    # Pumping: how quickly does the processed level move compared to the raw one during speech?
    dr, dp = np.abs(np.diff(r))[speech[1:]], np.abs(np.diff(p))[speech[1:]]
    print(f"\nPUMPING / ROBOTIC")
    print(f"  mean frame-to-frame level change in speech: raw {dr.mean():.2f} dB, processed {dp.mean():.2f} dB ({dp.mean()/max(dr.mean(),1e-6):.1f}x)")
    bursts = int(((p[quiet] > np.median(p[quiet]) + 10)).sum())
    print(f"  residual bursts in quiet parts (>10 dB above processed floor): {bursts} frames  ({bursts*FRAME:.1f} s)")

    print("\nREVERB / 'CONCERT HALL'")
    tr, tp = decay_time(r, speech), decay_time(p, speech)
    print(f"  time for the level to fall 20 dB after speech: raw {tr*1000:.0f} ms, processed {tp*1000:.0f} ms" + ("   <- tail is longer after processing" if tp > tr * 1.3 else ""))

    print("\nGLITCHES")
    z = np.concatenate([[0], (proc == 0).astype(np.int8), [0]])
    edges = np.diff(z)
    runs = np.where(edges == -1)[0] - np.where(edges == 1)[0]
    long_runs = runs[runs >= 16]
    print(f"  silence-padding dropouts (>=16 exact zeros): {len(long_runs)}  ({len(long_runs)/(len(proc)/sr):.2f} per second)")
    jump = np.abs(np.diff(proc))
    clicks = int((jump > 0.5).sum())
    print(f"  large sample jumps (>0.5 full scale): {clicks}")

    # Plot
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots(4, 1, figsize=(13, 11), sharex=True)
    for a_, x, title in ((ax[0], raw, "raw"), (ax[1], proc, "processed")):
        f, t, s = signal.spectrogram(x, sr, nperseg=1024, noverlap=768)
        a_.pcolormesh(t, f / 1000, 10 * np.log10(s + 1e-12), vmin=-110, vmax=-30, shading="auto")
        a_.set_ylabel(f"{title}\nkHz")
        a_.set_ylim(0, 16)
    t = np.arange(len(r)) * FRAME
    ax[2].plot(t, r, label="raw", lw=0.8)
    ax[2].plot(t, p, label="processed", lw=0.8)
    ax[2].set_ylabel("level dB")
    ax[2].legend(loc="upper right")
    ax[3].plot(t, p - r, lw=0.8)
    ax[3].set_ylabel("processed - raw dB")
    ax[3].set_xlabel("seconds")
    fig.tight_layout()
    fig.savefig(d / "report.png", dpi=80)
    print(f"\nwrote {d/'report.png'}")


if __name__ == "__main__":
    main()
