//! The audio path.
//!
//! ```text
//! PipeWire RT thread            DSP worker thread                    PipeWire RT thread
//! capture cb ──ring_in──▶  denoise + studio (non-RT)  ──ring_out──▶  "Hush Mic" cb
//!                                   └────ring_mon────▶  monitor cb (optional, "hear myself")
//! ```
//!
//! The RT callbacks only copy samples and never allocate, lock or run the model. DeepFilterNet
//! (tract) allocates while inferring, so it lives on its own thread.

use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, Thread};
use std::time::Duration;

use anyhow::{anyhow, Result};
use hush_core::level::{peak_db, SILENCE_DB};
use hush_core::pipeline::{Pipeline, PipelineParams};
use hush_core::{HOP, SAMPLE_RATE, VIRTUAL_MIC_DESCRIPTION, VIRTUAL_MIC_NAME};
use pipewire as pw;
use pw::core::CoreRc;
use pw::properties::properties;
use pw::spa;
use pw::stream::{StreamFlags, StreamRc, StreamState};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use spa::pod::Pod;

/// Samples per ring buffer (~340 ms). Only a safety margin; steady state holds a few hops.
const RING_CAPACITY: usize = 16384;
/// The Hush Mic callback waits for this many samples before it starts playing (jitter cushion).
const PREBUFFER: usize = HOP * 2;
/// If the output ring holds more than this, the surplus is dropped to bound latency.
const MAX_FILL: usize = HOP * 6;

pub fn new_ring() -> (HeapProd<f32>, HeapCons<f32>) {
    HeapRb::<f32>::new(RING_CAPACITY).split()
}

/// Fixed-point (centi-dB) peak meters that RT/worker threads max-hold and readers drain.
pub struct Meter {
    input: AtomicI32,
    output: AtomicI32,
}

const METER_FLOOR: i32 = (SILENCE_DB * 100.0) as i32;

impl Meter {
    fn new() -> Self {
        Meter {
            input: AtomicI32::new(METER_FLOOR),
            output: AtomicI32::new(METER_FLOOR),
        }
    }

    fn update(&self, input_db: f32, output_db: f32) {
        self.input
            .fetch_max((input_db * 100.0) as i32, Ordering::Relaxed);
        self.output
            .fetch_max((output_db * 100.0) as i32, Ordering::Relaxed);
    }

    /// Peak since the last call, in dBFS.
    pub fn take(&self) -> (f32, f32) {
        let i = self.input.swap(METER_FLOOR, Ordering::Relaxed);
        let o = self.output.swap(METER_FLOOR, Ordering::Relaxed);
        (i as f32 / 100.0, o as f32 / 100.0)
    }
}

// ---------------------------------------------------------------------------------------------
// DSP worker
// ---------------------------------------------------------------------------------------------

pub enum WorkerCmd {
    Params(PipelineParams),
    /// New microphone feed (or `None` when the mic is gone).
    Input(Option<HeapCons<f32>>),
    /// Feed for "Hush Mic".
    Output(HeapProd<f32>),
    /// What is currently being played (echo reference), or `None`.
    Reference(Option<HeapCons<f32>>),
    /// Feed for the "hear myself" monitor.
    Monitor(Option<HeapProd<f32>>),
    Shutdown,
}

const WORKER_LOADING: u8 = 0;
const WORKER_READY: u8 = 1;
const WORKER_FAILED: u8 = 2;

/// State the worker publishes for the engine's health checks.
pub struct WorkerShared {
    status: AtomicU8,
    error: Mutex<Option<String>>,
    latency_samples: AtomicU32,
    pub echo_available: AtomicBool,
    pub meter: Meter,
    /// Number of hops where the model failed (dry signal passed through).
    pub model_errors: AtomicU32,
}

pub enum WorkerHealth {
    Loading,
    Ready,
    Failed(String),
}

pub struct Worker {
    tx: Sender<WorkerCmd>,
    thread: Option<JoinHandle<()>>,
    wake: Thread,
    pub shared: Arc<WorkerShared>,
}

impl Worker {
    pub fn spawn(params: PipelineParams) -> Result<Worker> {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(WorkerShared {
            status: AtomicU8::new(WORKER_LOADING),
            error: Mutex::new(None),
            latency_samples: AtomicU32::new(0),
            echo_available: AtomicBool::new(false),
            meter: Meter::new(),
            model_errors: AtomicU32::new(0),
        });
        let s = shared.clone();
        let handle = thread::Builder::new()
            .name("hush-dsp".into())
            .spawn(move || worker_main(params, rx, s))?;
        let wake = handle.thread().clone();
        Ok(Worker {
            tx,
            thread: Some(handle),
            wake,
            shared,
        })
    }

    pub fn send(&self, cmd: WorkerCmd) {
        let _ = self.tx.send(cmd);
        self.wake.unpark();
    }

    pub fn wake_handle(&self) -> Thread {
        self.wake.clone()
    }

    pub fn health(&self) -> WorkerHealth {
        if self.thread.as_ref().is_some_and(|t| t.is_finished())
            && self.shared.status.load(Ordering::Acquire) != WORKER_FAILED
        {
            return WorkerHealth::Failed("the audio processing thread stopped unexpectedly".into());
        }
        match self.shared.status.load(Ordering::Acquire) {
            WORKER_READY => WorkerHealth::Ready,
            WORKER_FAILED => WorkerHealth::Failed(
                self.shared
                    .error
                    .lock()
                    .map(|e| e.clone().unwrap_or_default())
                    .unwrap_or_default(),
            ),
            _ => WorkerHealth::Loading,
        }
    }

    pub fn latency_samples(&self) -> usize {
        self.shared.latency_samples.load(Ordering::Relaxed) as usize
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.tx.send(WorkerCmd::Shutdown);
        self.wake.unpark();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn worker_main(params: PipelineParams, rx: Receiver<WorkerCmd>, shared: Arc<WorkerShared>) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        worker_loop(params, &rx, &shared)
    }));
    let msg = match result {
        Ok(Ok(())) => return,
        Ok(Err(e)) => e.to_string(),
        Err(_) => "the audio processing thread crashed".to_string(),
    };
    log::error!("dsp worker failed: {msg}");
    if let Ok(mut e) = shared.error.lock() {
        *e = Some(msg);
    }
    shared.status.store(WORKER_FAILED, Ordering::Release);
}

fn worker_loop(
    params: PipelineParams,
    rx: &Receiver<WorkerCmd>,
    shared: &WorkerShared,
) -> Result<()> {
    let mut pipeline = Pipeline::new(params)?;
    shared
        .latency_samples
        .store(pipeline.latency_samples() as u32, Ordering::Relaxed);
    shared
        .echo_available
        .store(pipeline.echo_available(), Ordering::Relaxed);
    shared.status.store(WORKER_READY, Ordering::Release);
    log::info!(
        "dsp worker ready (algorithmic latency {} samples)",
        pipeline.latency_samples()
    );

    let mut input: Option<HeapCons<f32>> = None;
    let mut output: Option<HeapProd<f32>> = None;
    let mut monitor: Option<HeapProd<f32>> = None;
    let mut reference: Option<HeapCons<f32>> = None;
    let mut refbuf = [0.0f32; HOP];
    let (mut hops_with_ref, mut ref_missing, mut ref_dropped) = (0u64, 0u64, 0u64);
    let mut dry = [0.0f32; HOP];
    let mut wet = [0.0f32; HOP];

    loop {
        loop {
            match rx.try_recv() {
                Ok(WorkerCmd::Params(p)) => {
                    pipeline.set_params(p);
                    shared
                        .latency_samples
                        .store(pipeline.latency_samples() as u32, Ordering::Relaxed);
                }
                Ok(WorkerCmd::Input(c)) => input = c,
                Ok(WorkerCmd::Output(p)) => output = Some(p),
                Ok(WorkerCmd::Reference(c)) => reference = c,
                Ok(WorkerCmd::Monitor(p)) => monitor = p,
                Ok(WorkerCmd::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        if let (Some(inp), Some(out)) = (input.as_mut(), output.as_mut()) {
            while inp.occupied_len() >= HOP {
                if out.vacant_len() < HOP {
                    // Nobody is reading Hush Mic. Do not queue stale audio: keep only the newest hop.
                    let stale = inp.occupied_len() - HOP;
                    inp.skip(stale);
                    break;
                }
                inp.pop_slice(&mut dry);
                // Echo reference: the newest hop if available, otherwise silence.
                refbuf.fill(0.0);
                if let Some(r) = reference.as_mut() {
                    let avail = r.occupied_len();
                    if avail > HOP * 8 {
                        r.skip(avail - HOP * 3);
                        ref_dropped += 1;
                    }
                    if r.occupied_len() >= HOP {
                        r.pop_slice(&mut refbuf);
                    } else {
                        ref_missing += 1;
                    }
                    hops_with_ref += 1;
                    if hops_with_ref % 500 == 0 {
                        log::debug!(
                            "echo reference: {hops_with_ref} hops, {ref_missing} missing, {ref_dropped} drops, fill {}; {}",
                            r.occupied_len(),
                            pipeline.echo_stats().unwrap_or_default()
                        );
                    }
                }
                if let Err(e) = pipeline.process(&dry, Some(&refbuf), &mut wet) {
                    if shared.model_errors.fetch_add(1, Ordering::Relaxed) == 0 {
                        log::warn!("noise suppression failed, passing audio through: {e}");
                    }
                }
                out.push_slice(&wet);
                if let Some(m) = monitor.as_mut() {
                    if m.vacant_len() >= HOP {
                        m.push_slice(&wet);
                    }
                }
                shared.meter.update(peak_db(&dry), peak_db(&wet));
            }
        }
        thread::park_timeout(Duration::from_millis(5));
    }
}

// ---------------------------------------------------------------------------------------------
// PipeWire streams
// ---------------------------------------------------------------------------------------------

/// A connected stream plus its listener. Dropping it disconnects and destroys the stream.
pub struct AudioStream {
    // Field order matters: the listener must go before the stream it listens to.
    _listener: Box<dyn Any>,
    stream: StreamRc,
}

impl AudioStream {
    pub fn state(&self) -> StreamState {
        self.stream.state()
    }
}

/// Called (from the PipeWire loop thread) when a stream enters the error state.
pub type ErrorSink = Arc<dyn Fn(&'static str, String) + Send + Sync>;

fn format_pod() -> Result<Vec<u8>> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(SAMPLE_RATE);
    info.set_channels(1);
    let mut position = [0; spa::param::audio::MAX_CHANNELS];
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let obj = spa::pod::Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Format,
        id: spa::sys::SPA_PARAM_EnumFormat,
        properties: info.into(),
    };
    let bytes = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|e| anyhow!("building audio format: {e:?}"))?
    .0
    .into_inner();
    Ok(bytes)
}

fn watch_state<D>(
    builder_name: &'static str,
    errors: ErrorSink,
) -> impl FnMut(&pw::stream::Stream, &mut D, StreamState, StreamState) + 'static {
    move |_, _, _old, new| {
        if let StreamState::Error(msg) = new {
            errors(builder_name, msg);
        }
    }
}

fn as_f32(bytes: &mut [u8]) -> Option<&mut [f32]> {
    // PipeWire buffers are suitably aligned; refuse rather than misread if they are not.
    let (pre, mid, post) = unsafe { bytes.align_to_mut::<f32>() };
    (pre.is_empty() && post.is_empty()).then_some(mid)
}

struct CaptureData {
    prod: HeapProd<f32>,
    wake: Thread,
    /// Direct copy of the microphone for zero-detour monitoring (see `create_monitor`).
    raw: Option<HeapProd<f32>>,
}

fn process_capture(stream: &pw::stream::Stream, data: &mut CaptureData) {
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let datas = buffer.datas_mut();
    let Some(d) = datas.first_mut() else { return };
    let n = d.chunk().size() as usize;
    let offset = d.chunk().offset() as usize;
    let Some(bytes) = d.data() else { return };
    let end = (offset + n).min(bytes.len());
    if let Some(samples) = as_f32(&mut bytes[offset.min(end)..end]) {
        data.prod.push_slice(samples);
        if let Some(raw) = data.raw.as_mut() {
            raw.push_slice(samples);
        }
        data.wake.unpark();
    }
}

/// Capture the signal that is being played (the echo reference): the monitor of an output device
/// (`is_sink`) or, for unusual setups, any source.
pub fn create_reference(
    core: &CoreRc,
    target: &str,
    is_sink: bool,
    prod: HeapProd<f32>,
    wake: Thread,
) -> Result<AudioStream> {
    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => "hush.reference",
        *pw::keys::NODE_DESCRIPTION => "Hush (Echo)",
        *pw::keys::APP_NAME => "Hush",
        *pw::keys::TARGET_OBJECT => target,
        "node.dont-reconnect" => "true",
        "node.dont-fallback" => "true",
        "node.group" => "hush",
    };
    if is_sink {
        props.insert("stream.capture.sink", "true");
    }
    let stream = StreamRc::new(core.clone(), "Hush echo reference", props)?;
    let listener = stream
        .add_local_listener_with_user_data(CaptureData {
            prod,
            wake,
            raw: None,
        })
        .process(process_capture)
        .register()?;
    let bytes = format_pod()?;
    let mut params = [Pod::from_bytes(&bytes).ok_or_else(|| anyhow!("invalid format pod"))?];
    stream.connect(
        spa::utils::Direction::Input,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    Ok(AudioStream {
        _listener: Box::new(listener),
        stream,
    })
}

/// Capture `target` (a `node.name`) as mono 48 kHz float and feed `prod`.
pub fn create_capture(
    core: &CoreRc,
    target: &str,
    prod: HeapProd<f32>,
    raw: HeapProd<f32>,
    wake: Thread,
    errors: ErrorSink,
) -> Result<AudioStream> {
    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Communication",
        *pw::keys::NODE_NAME => "hush.capture",
        *pw::keys::NODE_DESCRIPTION => "Hush",
        *pw::keys::APP_NAME => "Hush",
        *pw::keys::TARGET_OBJECT => target,
        // Never wander off to another source (it could be Hush Mic itself: feedback loop).
        "node.dont-reconnect" => "true",
        "node.dont-fallback" => "true",
        "node.group" => "hush",
    };
    let stream = StreamRc::new(core.clone(), "Hush capture", props)?;
    let listener = stream
        .add_local_listener_with_user_data(CaptureData {
            prod,
            wake,
            raw: Some(raw),
        })
        .state_changed(|_, _: &mut CaptureData, _, _| {})
        .process(process_capture)
        .register()?;
    let _ = errors; // capture errors surface through the watchdog (stream state is polled)
    let bytes = format_pod()?;
    let mut params = [Pod::from_bytes(&bytes).ok_or_else(|| anyhow!("invalid format pod"))?];
    stream.connect(
        spa::utils::Direction::Input,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    Ok(AudioStream {
        _listener: Box::new(listener),
        stream,
    })
}

struct SourceData {
    cons: HeapCons<f32>,
    primed: bool,
    /// Samples to collect before playing starts (jitter cushion).
    prebuffer: usize,
}

/// Fill `out` from a ring, with a jitter cushion and bounded latency.
fn fill_ring(
    cons: &mut HeapCons<f32>,
    primed: &mut bool,
    prebuffer: usize,
    max_fill: usize,
    out: &mut [f32],
) {
    let avail = cons.occupied_len();
    if avail > max_fill {
        cons.skip(avail - prebuffer);
    }
    if !*primed {
        if cons.occupied_len() >= prebuffer {
            *primed = true;
        } else {
            out.fill(0.0);
            return;
        }
    }
    let got = cons.pop_slice(out);
    if got < out.len() {
        out[got..].fill(0.0);
        *primed = false; // underrun: rebuffer
    }
}

/// Hand the stream's next buffer to `fill`, which writes mono f32 samples.
fn with_output_buffer(stream: &pw::stream::Stream, fill: impl FnOnce(&mut [f32])) {
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let requested = buffer.requested() as usize;
    let datas = buffer.datas_mut();
    let Some(d) = datas.first_mut() else { return };
    let max_frames = d.as_raw().maxsize as usize / 4;
    let frames = if requested > 0 {
        requested.min(max_frames)
    } else {
        max_frames
    };
    let written = match d.data().and_then(as_f32) {
        Some(slice) => {
            let frames = frames.min(slice.len());
            fill(&mut slice[..frames]);
            frames
        }
        None => 0,
    };
    let chunk = d.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = 4;
    *chunk.size_mut() = (written * 4) as u32;
}

fn process_source(stream: &pw::stream::Stream, st: &mut SourceData) {
    with_output_buffer(stream, |out| {
        fill_ring(&mut st.cons, &mut st.primed, st.prebuffer, MAX_FILL, out)
    });
}

/// Raw monitoring is a tight loop: tiny cushion, tiny backlog.
const RAW_PREBUFFER: usize = 192;
const RAW_MAX_FILL: usize = 1024;

struct MonitorData {
    processed: SourceData,
    /// Microphone copied straight from the capture callback (no DSP thread in between). Shared
    /// with the engine so a later monitor can pick it up again; the engine never holds the lock,
    /// so `try_lock` in the RT callback cannot fail in practice and never blocks.
    raw: Arc<Mutex<HeapCons<f32>>>,
    raw_primed: bool,
    /// A/B test: play the raw microphone instead of the processed signal.
    use_raw: Arc<AtomicBool>,
}

fn process_monitor(stream: &pw::stream::Stream, m: &mut MonitorData) {
    with_output_buffer(stream, |out| {
        let Ok(mut raw) = m.raw.try_lock() else {
            out.fill(0.0);
            return;
        };
        if m.use_raw.load(Ordering::Relaxed) {
            // Keep the other ring empty so switching back never plays stale audio.
            let stale = m.processed.cons.occupied_len();
            m.processed.cons.skip(stale);
            m.processed.primed = false;
            fill_ring(
                &mut raw,
                &mut m.raw_primed,
                RAW_PREBUFFER,
                RAW_MAX_FILL,
                out,
            );
        } else {
            let stale = raw.occupied_len();
            raw.skip(stale);
            m.raw_primed = false;
            let p = &mut m.processed;
            fill_ring(&mut p.cons, &mut p.primed, p.prebuffer, MAX_FILL, out);
        }
    });
}

/// Create the persistent virtual microphone.
pub fn create_virtual_mic(
    core: &CoreRc,
    cons: HeapCons<f32>,
    errors: ErrorSink,
) -> Result<AudioStream> {
    // `Audio/Source` (not `Audio/Source/Virtual`): the latter crashes libpipewire's client-side
    // adapter when combined with a fixed format. This is also what the loopback module uses.
    let props = properties! {
        *pw::keys::MEDIA_CLASS => "Audio/Source",
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::NODE_NAME => VIRTUAL_MIC_NAME,
        *pw::keys::NODE_DESCRIPTION => VIRTUAL_MIC_DESCRIPTION,
        *pw::keys::NODE_NICK => VIRTUAL_MIC_DESCRIPTION,
        *pw::keys::NODE_VIRTUAL => "true",
        *pw::keys::DEVICE_ICON_NAME => "audio-input-microphone",
        *pw::keys::APP_NAME => "Hush",
        "node.group" => "hush",
        "audio.position" => "MONO",
    };
    let stream = StreamRc::new(core.clone(), VIRTUAL_MIC_DESCRIPTION, props)?;
    let listener = stream
        .add_local_listener_with_user_data(SourceData {
            cons,
            primed: false,
            prebuffer: PREBUFFER,
        })
        .state_changed(watch_state("virtual-mic", errors))
        .process(process_source)
        .register()?;
    let bytes = format_pod()?;
    let mut params = [Pod::from_bytes(&bytes).ok_or_else(|| anyhow!("invalid format pod"))?];
    // No AUTOCONNECT: this node is a source that other apps connect to.
    stream.connect(
        spa::utils::Direction::Output,
        None,
        StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    Ok(AudioStream {
        _listener: Box::new(listener),
        stream,
    })
}

/// "Hear myself": plays the monitor ring on the default output.
pub fn create_monitor(
    core: &CoreRc,
    cons: HeapCons<f32>,
    raw: Arc<Mutex<HeapCons<f32>>>,
    use_raw: Arc<AtomicBool>,
    errors: ErrorSink,
) -> Result<AudioStream> {
    let props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Playback",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::NODE_NAME => "hush.monitor",
        *pw::keys::NODE_DESCRIPTION => "Hush (Test)",
        // Ask for a small buffer while the user is listening to themselves.
        *pw::keys::NODE_LATENCY => "128/48000",
        *pw::keys::APP_NAME => "Hush",
    };
    let stream = StreamRc::new(core.clone(), "Hush monitor", props)?;
    let listener = stream
        .add_local_listener_with_user_data(MonitorData {
            processed: SourceData {
                cons,
                primed: false,
                prebuffer: HOP,
            },
            raw,
            raw_primed: false,
            use_raw,
        })
        .state_changed(watch_state("monitor", errors))
        .process(process_monitor)
        .register()?;
    let bytes = format_pod()?;
    let mut params = [Pod::from_bytes(&bytes).ok_or_else(|| anyhow!("invalid format pod"))?];
    stream.connect(
        spa::utils::Direction::Output,
        None,
        StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    Ok(AudioStream {
        _listener: Box::new(listener),
        stream,
    })
}

/// Whether an audio stream shows signs of life (used by the watchdog).
pub fn stream_failed(s: &AudioStream) -> Option<String> {
    match s.state() {
        StreamState::Error(e) => Some(e),
        _ => None,
    }
}
