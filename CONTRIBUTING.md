# Contributing

Thanks for helping. A few notes that save everybody time.

## Layout

| Path | What |
|---|---|
| `crates/hush-core` | Settings, DSP chain, DeepFilterNet wrapper, echo canceller. No PipeWire, no D-Bus. |
| `crates/hushd` | The daemon: PipeWire client, DSP worker thread, D-Bus API. |
| `crates/hushctl` | Command line client. |
| `ui/` | Svelte frontend, `ui/src-tauri` is the Tauri shell. |
| `dist/` | Desktop file, D-Bus activation file, systemd user unit. |
| `packaging/aur` | PKGBUILD. |

## Build and test

```sh
cd ui && npm ci && npm run build && cd ..   # the Tauri crate embeds ui/dist
cargo build --release
cargo test --release --workspace
```

Use `--release` for anything that loads the model: DeepFilterNet's graph optimiser trips over
debug assertions. `cargo build --profile fast` is a quicker optimised profile (no LTO) for iteration.

## Rules that keep Hush from breaking your audio

* Hush never writes PipeWire configuration. Everything is a normal PipeWire client object.
* No allocation, locking or model inference in PipeWire's real-time callbacks. They only copy
  samples into ring buffers; the DSP runs on its own thread.
* Every error that can happen in the chain must end up as a readable message in the UI.
* `pipewire-rs` requires all PipeWire calls on the main thread.
* Callbacks never re-enter the engine synchronously; they enqueue a command.

## Before opening a PR

`cargo fmt --all`, `cargo clippy --release --workspace --all-targets`, `cargo test --release --workspace`
and `cd ui && npm run check`.

## License of contributions

By contributing you agree that your work is licensed under GPL-3.0-or-later with the attribution terms in [NOTICE](NOTICE).
