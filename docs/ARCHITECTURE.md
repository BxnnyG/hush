# Architecture and roadmap

## Goal

A Linux desktop app (PipeWire only) that does what Krisp does elsewhere:

1. Open the app, pick a microphone, flip **one switch**.
2. A virtual microphone, **Sordino Mic**, appears in Discord, Element, Teams, Zoom, OBS.
3. A few simple controls for "studio sound" for people who do not know audio.
4. It looks like a modern app, not a 2009 audio tool.

Non-goals: no replacement for EasyEffects or a patchbay, no PulseAudio-only systems, no cloud,
no account, no telemetry.

## Why this exists

Every building block already exists (PipeWire filter-chain and echo-cancel, RNNoise, DeepFilterNet,
EasyEffects, NoiseTorch). What is missing is a *product*: one switch, reliable hotplug, plain-language
errors, and a fix for the classic trap where a USB microphone sits on the `pro-audio` profile and
every noise tool silently does nothing. Sordino is product and reliability work, not new DSP research.

## Rules that keep it from breaking your audio

* **Sordino never writes PipeWire configuration.** It is an ordinary PipeWire client. If it crashes, only
  "Sordino Mic" disappears, the rest of the audio graph is untouched.
* PipeWire's real-time callbacks only copy samples. The neural network allocates while it runs, so
  all DSP happens on a separate thread behind lock-free ring buffers.
* "Sordino Mic" is persistent while the physical microphone comes and goes, so apps keep their link.
* The system default microphone is only overridden on request, the previous value is saved and put
  back on exit, and also after a crash (on the next start).
* Self-monitoring ("hear myself") expires unless a client keeps renewing it.

## Layers

```
UI (Tauri + Svelte, tray)        sordinoctl (CLI)
        \                          /
         └──── D-Bus: io.github.bxnnyg.Sordino ────┐
                                                ▼
                sordinod (Rust user service, PipeWire main-loop thread)
                  • device / profile / default-source watching (hotplug)
                  • owns the audio chain and the DSP worker thread
                  • publishes state as JSON, emits levels
                                                │ libpipewire
                                                ▼
                                   PipeWire + WirePlumber, unchanged
```

Audio path:

```
mic ─▶ [echo cancel] ─▶ [DeepFilterNet 3] ─▶ [gate · EQ · de-esser · compressor · limiter] ─▶ Sordino Mic
         (WebRTC AEC3)    10 ms hops, 48 kHz
```

The echo reference is the monitor of the default output device, captured as a second stream.

## Measured

| What | Result |
|---|---|
| Noise model CPU | about 8 % of one core, worst 10 ms hop 3.8 ms |
| Added latency (processed) | about 30 to 45 ms (the model looks ahead) |
| Noise suppression | 26 to 30 dB on a synthetic noisy recording |
| Echo suppression | echo path reduced by 60 dB or more after 10 to 15 s of adaptation (simulated path) |
| `kill -9` on the daemon | rest of the audio graph unaffected |

## Roadmap

| Milestone | Content | Status |
|---|---|---|
| M0 | Spike: virtual mic with noise suppression, CPU and latency measured | done |
| M1 | Daemon, device management, hotplug, profile detection and switching, `sordinoctl` | done |
| M2 | UI: microphone, switch, strength, level meters, profile banner, test mode with A/B | done |
| M3 | Tray, autostart, default-source handling with restore, plain-language errors, dark/light | done |
| M4 | Studio chain and presets, advanced sliders | done |
| M5 | Echo suppression | done, experimental |
| M6 | Packaging and release: deb, rpm, Arch package, Flatpak, tarball | in progress |
| later | Output side (denoise what others say), per-app profiles | open |

## Risks

* Audio real-time bugs (clicks, dropouts, drift) are hard to debug: test on real hardware.
* `pipewire-rs` has gaps and a moving API: keep PipeWire code thin and isolated (`crates/sordinod`).
* Echo cancellation depends on reference alignment and clock drift between devices. Treat it as
  experimental and say so in the UI.
* Maintenance: keep the code base small, tested and documented, welcome contributors early.
