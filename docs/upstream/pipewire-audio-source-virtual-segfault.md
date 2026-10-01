# Draft upstream bug report: segfault in a client when a stream uses `media.class = Audio/Source/Virtual`

Target: <https://gitlab.freedesktop.org/pipewire/pipewire/-/issues>
Status: draft, not filed yet. Re-test with the newest PipeWire before filing.

## Summary

A client stream that sets `media.class = Audio/Source/Virtual` and connects with an explicit
audio format crashes inside the client process (SIGSEGV in `libpipewire-0.3.so` called from the
adapter in `libspa-audioconvert.so`). The same stream with `media.class = Audio/Source` works.
The server (pipewire/wireplumber) is not affected.

## Environment

* PipeWire 1.6.8, WirePlumber 0.5.17 (Arch Linux packages)
* Reproduced with `pw-cat`, so no application code is involved

## Reproduction

```sh
# works (the stream shows up as a source and plays the file):
pw-cat -p --rate 48000 --channels 1 --format f32 \
  -P 'media.class=Audio/Source node.name=test node.description=Test' sine.wav

# crashes immediately (exit code 139, SIGSEGV):
pw-cat -p --rate 48000 --channels 1 --format f32 \
  -P 'media.class=Audio/Source/Virtual node.name=test node.description=Test' sine.wav
```

`sine.wav` is any short mono 48 kHz WAV, e.g.
`ffmpeg -f lavfi -i sine=frequency=1000:sample_rate=48000:duration=10 -ac 1 sine.wav`.

The same crash happens with the Rust bindings (`pipewire-rs`) when a playback-direction stream
sets that media class and passes an `EnumFormat` pod with fixed rate/channels. Without the format
param (let the graph choose) it does not crash.

## Backtrace (stripped distribution libraries)

```
Program received signal SIGSEGV, Segmentation fault.
#0  libpipewire-0.3.so.0
#1  libspa-audioconvert.so
#2  libspa-audioconvert.so
#3  libspa-audioconvert.so
#4  libspa-audioconvert.so
#5  libspa-audioconvert.so
#6  libspa-audioconvert.so
#7  libspa-audioconvert.so
#8  libpipewire-module-client-node.so
#9  libpipewire-module-client-node.so
#10 libpipewire-module-protocol-native.so
#11 libpipewire-module-protocol-native.so
#12 libspa-support.so
```

The crash happens while the client-side node handles a message from the server, right after the
node was created.

## Expected

Either the stream works like `Audio/Source`, or the client reports an error. A crash in the
client is never the right outcome for a property value.

## Notes

`Audio/Source/Virtual` is documented as the class for virtual sources, so using it is reasonable.
Applications currently have to use `Audio/Source` as a workaround (this is what
`libpipewire-module-loopback` does). Sordino uses that workaround, see
`crates/sordinod/src/audio.rs`.
