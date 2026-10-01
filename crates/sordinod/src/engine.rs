//! The engine lives on the PipeWire main-loop thread. It watches the graph (devices, hotplug,
//! default source), owns the audio chain and turns every change into a fresh [`State`].
//!
//! Rules that keep it robust:
//! * Sordino never writes PipeWire config. Everything is a normal client object that disappears
//!   with the process.
//! * "Sordino Mic" stays alive while the physical mic comes and goes, so apps keep their link.
//! * Callbacks never re-enter the engine synchronously; they only enqueue a [`Cmd`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender as StdSender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use pipewire as pw;
use pw::context::ContextRc;
use pw::core::CoreRc;
use pw::device::{Device as PwDevice, DeviceChangeMask, DeviceListener};
use pw::metadata::{Metadata, MetadataListener};
use pw::registry::{GlobalObject, Listener as RegistryListener, RegistryRc};
use pw::spa::param::ParamType;
use pw::spa::pod::Pod;
use pw::spa::utils::dict::DictRef;
use pw::types::ObjectType;
use sordino_core::ipc::{Device, ProfileHint, State, Status};
use sordino_core::profile::{is_unfavourable, suggest};
use sordino_core::settings::{RuntimeState, Settings};
use sordino_core::{HOP, SAMPLE_RATE, VIRTUAL_MIC_NAME};

use crate::audio::{self, AudioStream, ErrorSink, Worker, WorkerCmd, WorkerHealth, WorkerShared};
use crate::devices::{self, CardInfo, SourceNode};

const DEFAULT_SOURCE: &str = "default.audio.source";
/// A client must renew `SetMonitor(true)` within this time or monitoring switches itself off.
const MONITOR_KEEPALIVE: Duration = Duration::from_secs(5);
const DEFAULT_SINK: &str = "default.audio.sink";
const CONFIGURED_DEFAULT_SINK: &str = "default.configured.audio.sink";
const CONFIGURED_DEFAULT_SOURCE: &str = "default.configured.audio.source";

/// Commands from D-Bus and from stream callbacks.
pub enum Cmd {
    /// Merge a JSON patch into the settings.
    Apply(serde_json::Value),
    SetProfile {
        card: u32,
        index: i32,
    },
    SetMonitor(bool),
    /// Choose the system default microphone (`sink == false`) or output (`sink == true`).
    SetDefaultDevice {
        sink: bool,
        name: String,
    },
    SetAbOriginal(bool),
    /// Put the system default microphone back to what it was before Sordino.
    RestoreDefault,
    StreamError(&'static str, String),
    CoreLost,
    Quit,
}

pub enum Event {
    StateChanged,
    Stopped,
}

/// Data shared with the D-Bus thread.
pub struct Shared {
    pub state_json: Mutex<String>,
    pub worker: Mutex<Option<Arc<WorkerShared>>>,
}

struct CardProxy {
    device: PwDevice,
    _listener: DeviceListener,
}

pub struct Engine {
    mainloop: pw::main_loop::MainLoopWeak,
    context: ContextRc,
    cmd_tx: pw::channel::Sender<Cmd>,
    events: StdSender<Event>,
    shared: Arc<Shared>,
    me: Weak<RefCell<Engine>>,

    // connection
    core: Option<CoreRc>,
    registry: Option<RegistryRc>,
    _registry_listener: Option<RegistryListener>,
    _core_listener: Option<pw::core::Listener>,
    next_connect: Instant,

    // graph knowledge
    sources: HashMap<u32, SourceNode>,
    /// Output devices (`node.name`), the echo reference is taken from their monitor.
    sinks: HashMap<u32, SourceNode>,
    cards: HashMap<u32, CardInfo>,
    card_proxies: HashMap<u32, CardProxy>,
    metadata: Option<(u32, Metadata, MetadataListener)>,
    sordino_mic_node: Option<u32>,
    /// Current `default.audio.source` (the resolved one).
    default_source: Option<String>,
    default_sink: Option<String>,
    configured_default_raw: Option<String>,
    /// Last default source that was not Sordino Mic: what "follow the system default" resolves to.
    foreign_default: Option<String>,
    pending_profile_switch: Option<(u32, String, Instant)>,

    // user intent
    settings: Settings,
    runtime: RuntimeState,
    monitoring: bool,
    /// Self-monitoring only lasts while a client keeps asking for it, so it can never get stuck on.
    monitor_until: Option<Instant>,
    ab_original: bool,
    /// Shared with the monitor stream's RT callback.
    ab_flag: Arc<AtomicBool>,
    stats: Arc<audio::AudioStats>,
    /// Consumer end of the raw microphone tap, handed to the monitor stream.
    raw_mon: Option<Arc<Mutex<ringbuf::HeapCons<f32>>>>,
    persist: bool,
    mic_override: Option<String>,
    quit_at: Option<Instant>,
    /// Until this moment we wait for the system default source to be reported.
    default_wait_until: Instant,
    default_wait_done: bool,

    // chain
    worker: Option<Worker>,
    virtual_mic: Option<AudioStream>,
    capture: Option<AudioStream>,
    capture_target: Option<String>,
    reference: Option<AudioStream>,
    reference_target: Option<String>,
    monitor: Option<AudioStream>,
    overrode_default: bool,

    // health
    status: Status,
    error: Option<String>,
    failures: u32,
    retry_at: Option<Instant>,
    stable_since: Option<Instant>,
    last_health: Instant,
    dirty_since: Option<Instant>,
}

impl Engine {
    pub fn new(
        mainloop: &pw::main_loop::MainLoopRc,
        cmd_tx: pw::channel::Sender<Cmd>,
        events: StdSender<Event>,
        shared: Arc<Shared>,
        persist: bool,
        mic_override: Option<String>,
    ) -> Result<Rc<RefCell<Engine>>> {
        let context = ContextRc::new(mainloop, None)?;
        let settings = if persist {
            Settings::load()
        } else {
            Settings::default()
        };
        let runtime = if persist {
            RuntimeState::load()
        } else {
            RuntimeState::default()
        };
        let engine = Rc::new(RefCell::new(Engine {
            mainloop: mainloop.downgrade(),
            context,
            cmd_tx,
            events,
            shared,
            me: Weak::new(),
            core: None,
            registry: None,
            _registry_listener: None,
            _core_listener: None,
            next_connect: Instant::now(),
            sources: HashMap::new(),
            sinks: HashMap::new(),
            cards: HashMap::new(),
            card_proxies: HashMap::new(),
            metadata: None,
            sordino_mic_node: None,
            default_source: None,
            default_sink: None,
            configured_default_raw: None,
            foreign_default: None,
            pending_profile_switch: None,
            settings,
            runtime,
            monitoring: false,
            monitor_until: None,
            ab_original: false,
            ab_flag: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(audio::AudioStats::default()),
            raw_mon: None,
            persist,
            mic_override,
            quit_at: None,
            default_wait_until: Instant::now() + Duration::from_millis(1500),
            default_wait_done: false,
            worker: None,
            virtual_mic: None,
            capture: None,
            capture_target: None,
            reference: None,
            reference_target: None,
            monitor: None,
            overrode_default: false,
            status: Status::Starting,
            error: None,
            failures: 0,
            retry_at: None,
            stable_since: None,
            last_health: Instant::now(),
            dirty_since: Some(Instant::now()),
        }));
        engine.borrow_mut().me = Rc::downgrade(&engine);
        Ok(engine)
    }

    fn error_sink(&self) -> ErrorSink {
        let tx = self.cmd_tx.clone();
        // `Sender::send` takes &self and is thread-safe; wrap it so streams can report errors.
        let tx = Arc::new(Mutex::new(tx));
        Arc::new(move |which, msg| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(Cmd::StreamError(which, msg));
            }
        })
    }

    fn dirty(&mut self) {
        if self.dirty_since.is_none() {
            self.dirty_since = Some(Instant::now());
        }
    }

    // -----------------------------------------------------------------------------------------
    // Connection to PipeWire
    // -----------------------------------------------------------------------------------------

    fn try_connect(&mut self) {
        if self.core.is_some() || Instant::now() < self.next_connect {
            return;
        }
        match self.context.connect_rc(None) {
            Ok(core) => {
                log::info!("connected to PipeWire");
                let weak = self.me.clone();
                let tx = self.cmd_tx.clone();
                let core_listener = core
                    .add_listener_local()
                    .error(move |id, _seq, res, msg| {
                        // id 0 is the core itself: the connection is gone (EPIPE).
                        log::warn!("pipewire error id {id}: {res} {msg}");
                        if id == 0 && res == -32 {
                            let _ = tx.send(Cmd::CoreLost);
                        }
                    })
                    .register();
                match core.get_registry_rc() {
                    Ok(registry) => {
                        let w1 = weak.clone();
                        let w2 = weak;
                        let reg_l = registry
                            .add_listener_local()
                            .global(move |obj| {
                                if let Some(e) = w1.upgrade() {
                                    if let Ok(mut e) = e.try_borrow_mut() {
                                        e.on_global(obj);
                                    }
                                }
                            })
                            .global_remove(move |id| {
                                if let Some(e) = w2.upgrade() {
                                    if let Ok(mut e) = e.try_borrow_mut() {
                                        e.on_global_remove(id);
                                    }
                                }
                            })
                            .register();
                        self.registry = Some(registry);
                        self._registry_listener = Some(reg_l);
                        self._core_listener = Some(core_listener);
                        self.core = Some(core);
                        self.status = Status::Starting;
                        self.reconcile();
                    }
                    Err(e) => log::warn!("could not get registry: {e}"),
                }
            }
            Err(e) => {
                if self.status != Status::NoPipewire {
                    log::warn!("PipeWire not reachable: {e}");
                }
                self.status = Status::NoPipewire;
                self.error = Some(e.to_string());
                self.next_connect = Instant::now() + Duration::from_secs(2);
                self.dirty();
            }
        }
    }

    fn on_core_lost(&mut self) {
        log::warn!("lost connection to PipeWire, will reconnect");
        self.teardown_chain(false);
        self.worker = None;
        self.metadata = None;
        self.card_proxies.clear();
        self._registry_listener = None;
        self.registry = None;
        self._core_listener = None;
        self.core = None;
        self.sources.clear();
        self.sinks.clear();
        self.cards.clear();
        self.sordino_mic_node = None;
        self.overrode_default = false;
        self.status = Status::NoPipewire;
        self.next_connect = Instant::now() + Duration::from_secs(1);
        self.dirty();
    }

    // -----------------------------------------------------------------------------------------
    // Registry
    // -----------------------------------------------------------------------------------------

    fn on_global(&mut self, obj: &GlobalObject<&DictRef>) {
        let Some(props) = obj.props else { return };
        let get = |k: &str| props.get(k).map(str::to_string);
        match obj.type_ {
            ObjectType::Node => {
                let name = get("node.name").unwrap_or_default();
                if name == VIRTUAL_MIC_NAME {
                    self.sordino_mic_node = Some(obj.id);
                } else if !name.starts_with("sordino.") {
                    if get("media.class").as_deref() == Some("Audio/Sink") {
                        if let Some(sink) = SourceNode::from_props_class(obj.id, "Audio/Sink", get)
                        {
                            self.sinks.insert(obj.id, sink);
                        }
                    }
                    if let Some(src) = SourceNode::from_props(obj.id, get) {
                        log::debug!("source added: {} ({})", src.name, src.description);
                        self.on_source_added(&src);
                        self.sources.insert(obj.id, src);
                    }
                }
            }
            ObjectType::Device => {
                if get("media.class").as_deref() == Some("Audio/Device") {
                    self.bind_card(obj);
                }
            }
            ObjectType::Metadata => {
                if get("metadata.name").as_deref() == Some("default") && self.metadata.is_none() {
                    self.bind_metadata(obj);
                }
            }
            _ => return,
        }
        self.reconcile();
        self.dirty();
    }

    fn on_global_remove(&mut self, id: u32) {
        if self.sordino_mic_node == Some(id) {
            self.sordino_mic_node = None;
        }
        if let Some(src) = self.sources.remove(&id) {
            log::debug!("source removed: {}", src.name);
        }
        self.sinks.remove(&id);
        self.cards.remove(&id);
        self.card_proxies.remove(&id);
        if self.metadata.as_ref().is_some_and(|(mid, _, _)| *mid == id) {
            self.metadata = None;
        }
        self.reconcile();
        self.dirty();
    }

    /// A new source appeared. If it is the result of a profile switch we triggered, keep the
    /// user's microphone choice pointing at the same physical device.
    fn on_source_added(&mut self, src: &SourceNode) {
        if let Some((card, old, until)) = self.pending_profile_switch.clone() {
            if Instant::now() > until {
                self.pending_profile_switch = None;
            } else if src.card == Some(card)
                && src.name != old
                && self.settings.mic.as_deref() == Some(old.as_str())
            {
                log::info!("microphone moved to {} after profile switch", src.name);
                self.settings.mic = Some(src.name.clone());
                self.pending_profile_switch = None;
                self.save_settings();
            }
        }
    }

    fn bind_card(&mut self, obj: &GlobalObject<&DictRef>) {
        let Some(registry) = self.registry.clone() else {
            return;
        };
        let device: PwDevice = match registry.bind(obj) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("cannot bind device {}: {e}", obj.id);
                return;
            }
        };
        let id = obj.id;
        let w_info = self.me.clone();
        let w_param = self.me.clone();
        let listener = device
            .add_listener_local()
            .info(move |info| {
                if info.change_mask().contains(DeviceChangeMask::PARAMS) {
                    if let Some(e) = w_info.upgrade() {
                        if let Ok(e) = e.try_borrow() {
                            e.refresh_card(id);
                        }
                    }
                }
            })
            .param(move |_seq, param_id, index, _next, param| {
                let Some(pod) = param else { return };
                if let Some(e) = w_param.upgrade() {
                    if let Ok(mut e) = e.try_borrow_mut() {
                        e.on_card_param(id, param_id, index, pod);
                    }
                }
            })
            .register();
        device.enum_params(0, Some(ParamType::EnumProfile), 0, u32::MAX);
        device.enum_params(1, Some(ParamType::Profile), 0, u32::MAX);
        self.card_proxies.insert(
            id,
            CardProxy {
                device,
                _listener: listener,
            },
        );
        self.cards.entry(id).or_default();
    }

    fn refresh_card(&self, id: u32) {
        if let Some(c) = self.card_proxies.get(&id) {
            c.device
                .enum_params(0, Some(ParamType::EnumProfile), 0, u32::MAX);
            c.device
                .enum_params(1, Some(ParamType::Profile), 0, u32::MAX);
        }
    }

    fn on_card_param(&mut self, id: u32, param_id: ParamType, index: u32, pod: &Pod) {
        let card = self.cards.entry(id).or_default();
        match param_id {
            ParamType::EnumProfile => {
                if index == 0 {
                    card.profiles.clear();
                }
                if let Some(p) = devices::parse_enum_profile(pod.as_bytes()) {
                    card.profiles.retain(|x| x.index != p.index);
                    card.profiles.push(p);
                }
            }
            ParamType::Profile => {
                card.active = devices::parse_profile_index(pod.as_bytes());
            }
            _ => return,
        }
        self.dirty();
    }

    fn bind_metadata(&mut self, obj: &GlobalObject<&DictRef>) {
        let Some(registry) = self.registry.clone() else {
            return;
        };
        let md: Metadata = match registry.bind(obj) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("cannot bind metadata: {e}");
                return;
            }
        };
        let weak = self.me.clone();
        let listener = md
            .add_listener_local()
            .property(move |subject, key, _type, value| {
                if subject == 0 {
                    if let (Some(key), Some(e)) = (key, weak.upgrade()) {
                        if let Ok(mut e) = e.try_borrow_mut() {
                            e.on_default_metadata(key, value);
                        }
                    }
                }
                0
            })
            .register();
        self.metadata = Some((obj.id, md, listener));
    }

    fn on_default_metadata(&mut self, key: &str, value: Option<&str>) {
        let name = || -> Option<String> {
            let v: serde_json::Value = serde_json::from_str(value?).ok()?;
            v.get("name")?.as_str().map(str::to_string)
        };
        match key {
            DEFAULT_SOURCE => {
                self.default_source = name();
                if let Some(n) = &self.default_source {
                    if n != VIRTUAL_MIC_NAME {
                        self.foreign_default = Some(n.clone());
                    }
                }
            }
            DEFAULT_SINK => self.default_sink = name(),
            CONFIGURED_DEFAULT_SOURCE => self.configured_default_raw = value.map(str::to_string),
            _ => return,
        }
        self.reconcile();
        self.dirty();
    }

    // -----------------------------------------------------------------------------------------
    // Commands
    // -----------------------------------------------------------------------------------------

    pub fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Apply(patch) => match self.settings.patched(&patch) {
                Ok(s) => {
                    self.settings = s;
                    self.save_settings();
                    self.reconcile();
                }
                Err(e) => log::warn!("rejected settings patch: {e}"),
            },
            Cmd::SetProfile { card, index } => {
                if let Some(proxy) = self.card_proxies.get(&card) {
                    if let Some(bytes) = devices::profile_pod(index) {
                        if let Some(pod) = Pod::from_bytes(&bytes) {
                            // Remember which mic this was so the choice follows the new node name.
                            let old = self.active_mic_name();
                            if let Some(old) = old.filter(|n| {
                                self.sources
                                    .values()
                                    .any(|s| s.card == Some(card) && &s.name == n)
                            }) {
                                self.pending_profile_switch =
                                    Some((card, old, Instant::now() + Duration::from_secs(10)));
                            }
                            proxy.device.set_param(ParamType::Profile, 0, pod);
                            log::info!("switching card {card} to profile {index}");
                        }
                    }
                }
            }
            Cmd::SetDefaultDevice { sink, name } => self.set_default_device(sink, &name),
            Cmd::SetMonitor(on) => {
                self.monitoring = on;
                self.monitor_until = on.then(|| Instant::now() + MONITOR_KEEPALIVE);
                if !on {
                    self.ab_original = false;
                }
                self.reconcile();
            }
            Cmd::SetAbOriginal(b) => {
                self.ab_original = b;
                self.ab_flag.store(b, std::sync::atomic::Ordering::Relaxed);
            }
            Cmd::RestoreDefault => {
                self.restore_default();
                self.reconcile();
            }
            Cmd::StreamError(which, msg) => {
                log::warn!("stream '{which}' failed: {msg}");
                if which == "monitor" {
                    // "Hear myself" is a side feature (and fails e.g. when there is no output
                    // device). It must never take Sordino Mic down with it: just switch it off.
                    self.monitor = None;
                    self.monitoring = false;
                    self.ab_original = false;
                    self.ab_flag
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    self.monitor_until = None;
                    if let Some(w) = &self.worker {
                        w.send(WorkerCmd::Monitor(None));
                    }
                } else {
                    self.fail_chain(format!("{which}: {msg}"));
                }
            }
            Cmd::CoreLost => self.on_core_lost(),
            Cmd::Quit => {
                if self.quit_at.is_none() {
                    self.shutdown();
                    // Keep the loop running briefly so the queued metadata change reaches PipeWire.
                    self.quit_at = Some(Instant::now() + Duration::from_millis(150));
                }
                return;
            }
        }
        self.dirty();
    }

    /// Make `name` the system default microphone or output.
    fn set_default_device(&mut self, sink: bool, name: &str) {
        if !sink && name == VIRTUAL_MIC_NAME {
            // Same as the "use as default microphone" switch: remembers and restores the old one.
            self.settings.set_default = true;
            self.save_settings();
            self.reconcile();
            return;
        }
        if !sink && self.settings.set_default {
            // Leaving Sordino Mic as the default: give the previous value back first, then
            // overwrite it with the explicit choice below.
            self.settings.set_default = false;
            self.save_settings();
            self.restore_default();
        }
        let Some((_, md, _)) = &self.metadata else {
            return;
        };
        let key = if sink {
            CONFIGURED_DEFAULT_SINK
        } else {
            CONFIGURED_DEFAULT_SOURCE
        };
        let value = serde_json::json!({ "name": name }).to_string();
        md.set_property(0, key, Some("Spa:String:JSON"), Some(&value));
        log::info!(
            "default {} set to {name}",
            if sink { "output" } else { "microphone" }
        );
        self.reconcile();
    }

    fn save_settings(&self) {
        if self.persist {
            if let Err(e) = self.settings.save() {
                log::warn!("could not save settings: {e:#}");
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Reconcile: make the chain match the settings and the graph
    // -----------------------------------------------------------------------------------------

    fn active_mic_name(&self) -> Option<String> {
        self.capture_target.clone()
    }

    /// The physical microphone that should feed the chain right now.
    fn effective_mic(&self) -> Option<String> {
        let exists = |n: &str| self.sources.values().any(|s| s.name == n);
        if let Some(m) = self
            .mic_override
            .as_deref()
            .or(self.settings.mic.as_deref())
        {
            return exists(m).then(|| m.to_string());
        }
        if !self.default_known() {
            return None;
        }
        if let Some(d) = &self.foreign_default {
            if exists(d) {
                return Some(d.clone());
            }
        }
        // No usable default: first non-hidden microphone.
        let mut visible: Vec<&SourceNode> = self
            .sources
            .values()
            .filter(|s| !s.hidden_by_default())
            .collect();
        visible.sort_by_key(|s| s.node_id);
        visible.first().map(|s| s.name.clone())
    }

    /// Whether the system default source has been reported (or we gave up waiting for it).
    fn default_known(&self) -> bool {
        self.default_source.is_some() || Instant::now() >= self.default_wait_until
    }

    fn backoff(&self) -> Duration {
        Duration::from_secs((1u64 << self.failures.min(5)).min(30))
    }

    fn fail_chain(&mut self, msg: String) {
        self.failures += 1;
        self.error = Some(msg);
        self.status = Status::Error;
        self.stable_since = None;
        self.teardown_chain(true);
        let wait = self.backoff();
        log::warn!(
            "restarting the audio chain in {wait:?} (failure #{})",
            self.failures
        );
        self.retry_at = Some(Instant::now() + wait);
        self.dirty();
    }

    /// Destroy streams (and the worker if `drop_worker`). Sordino Mic disappears with them.
    fn teardown_chain(&mut self, drop_worker: bool) {
        self.monitor = None;
        self.reference = None;
        self.reference_target = None;
        self.capture = None;
        self.capture_target = None;
        self.virtual_mic = None;
        if drop_worker {
            self.worker = None;
            *self.shared.worker.lock().unwrap() = None;
        } else if let Some(w) = &self.worker {
            w.send(WorkerCmd::Input(None));
        }
    }

    fn reconcile(&mut self) {
        let Some(core) = self.core.clone() else {
            return;
        };
        if let Some(t) = self.retry_at {
            if Instant::now() < t {
                return;
            }
            self.retry_at = None;
        }

        if !self.settings.enabled {
            self.teardown_chain(true);
            self.manage_default(false);
            self.status = Status::Off;
            self.error = None;
            return;
        }

        // 1. DSP worker
        if self.worker.is_none() {
            match Worker::spawn(self.settings.pipeline_params(), self.stats.clone()) {
                Ok(w) => {
                    *self.shared.worker.lock().unwrap() = Some(w.shared.clone());
                    self.worker = Some(w);
                }
                Err(e) => return self.fail_chain(format!("cannot start audio thread: {e:#}")),
            }
        }
        let worker = self.worker.as_ref().expect("worker exists");
        worker.send(WorkerCmd::Params(self.settings.pipeline_params()));

        // 2. Sordino Mic (persistent across microphone changes)
        if self.virtual_mic.is_none() {
            let (prod, cons) = audio::new_ring();
            match audio::create_virtual_mic(&core, cons, self.stats.clone(), self.error_sink()) {
                Ok(s) => {
                    worker.send(WorkerCmd::Output(prod));
                    self.virtual_mic = Some(s);
                }
                Err(e) => return self.fail_chain(format!("cannot create Sordino Mic: {e:#}")),
            }
        }

        // 3. Physical microphone
        let wanted = self.effective_mic();
        if wanted != self.capture_target || (wanted.is_some() && self.capture.is_none()) {
            self.capture = None;
            self.capture_target = None;
            // The monitor is wired to the old capture's raw tap, so it has to be rebuilt too.
            self.monitor = None;
            self.raw_mon = None;
            worker.send(WorkerCmd::Monitor(None));
            worker.send(WorkerCmd::Input(None));
            if let Some(target) = wanted {
                let (prod, cons) = audio::new_ring();
                let (raw_prod, raw_cons) = audio::new_ring();
                match audio::create_capture(
                    &core,
                    &target,
                    prod,
                    raw_prod,
                    self.stats.clone(),
                    worker.wake_handle(),
                    self.error_sink(),
                ) {
                    Ok(s) => {
                        log::info!("capturing from {target}");
                        self.raw_mon = Some(Arc::new(Mutex::new(raw_cons)));
                        worker.send(WorkerCmd::Input(Some(cons)));
                        self.capture = Some(s);
                        self.capture_target = Some(target);
                    }
                    Err(e) => return self.fail_chain(format!("cannot capture {target}: {e:#}")),
                }
            }
        }

        // 3b. Echo reference (what the speakers play), only while echo cancellation is on
        let echo_ok = worker
            .shared
            .echo_available
            .load(std::sync::atomic::Ordering::Relaxed);
        let wanted_ref = if self.settings.echo.enabled && echo_ok && self.capture.is_some() {
            self.settings
                .echo
                .reference
                .clone()
                .or_else(|| self.default_sink.clone())
        } else {
            None
        };
        if wanted_ref != self.reference_target || (wanted_ref.is_some() && self.reference.is_none())
        {
            self.reference = None;
            self.reference_target = None;
            worker.send(WorkerCmd::Reference(None));
            if let Some(target) = wanted_ref {
                let is_sink = self.sinks.values().any(|n| n.name == target);
                let (prod, cons) = audio::new_ring();
                match audio::create_reference(&core, &target, is_sink, prod, worker.wake_handle()) {
                    Ok(s) => {
                        log::info!("echo reference: {target}");
                        worker.send(WorkerCmd::Reference(Some(cons)));
                        self.reference = Some(s);
                        self.reference_target = Some(target);
                    }
                    Err(e) => log::warn!("cannot capture the echo reference: {e:#}"),
                }
            }
        }

        // 4. Monitor ("hear myself")
        if self.monitoring && self.monitor.is_none() && self.capture.is_some() {
            if let Some(raw) = self.raw_mon.clone() {
                let (prod, cons) = audio::new_ring();
                match audio::create_monitor(
                    &core,
                    cons,
                    raw,
                    self.ab_flag.clone(),
                    self.error_sink(),
                ) {
                    Ok(s) => {
                        worker.send(WorkerCmd::Monitor(Some(prod)));
                        self.monitor = Some(s);
                    }
                    Err(e) => log::warn!("cannot start monitor: {e:#}"),
                }
            }
        } else if (!self.monitoring || self.capture.is_none()) && self.monitor.is_some() {
            worker.send(WorkerCmd::Monitor(None));
            self.monitor = None;
        }

        // 5. Default microphone
        self.manage_default(self.settings.set_default);

        // 6. Status
        if self.capture.is_none() {
            let waiting =
                self.settings.mic.is_none() && self.mic_override.is_none() && !self.default_known();
            self.status = if waiting {
                Status::Starting
            } else {
                Status::MicMissing
            };
            self.error = None;
        } else {
            match self.worker.as_ref().map(Worker::health) {
                Some(WorkerHealth::Ready) => {
                    self.status = Status::Running;
                    self.error = None;
                    if self.stable_since.is_none() {
                        self.stable_since = Some(Instant::now());
                    }
                }
                Some(WorkerHealth::Loading) | None => self.status = Status::Starting,
                Some(WorkerHealth::Failed(msg)) => self.fail_chain(msg),
            }
        }
    }

    /// Make Sordino Mic the default microphone (`on`) or give the old default back (`!on`).
    fn manage_default(&mut self, on: bool) {
        let Some((_, md, _)) = &self.metadata else {
            return;
        };
        if on {
            if self.sordino_mic_node.is_none() || self.virtual_mic.is_none() {
                return;
            }
            let ours = serde_json::json!({ "name": VIRTUAL_MIC_NAME }).to_string();
            if self
                .configured_default_raw
                .as_deref()
                .is_some_and(|raw| raw.contains(VIRTUAL_MIC_NAME))
            {
                self.overrode_default = true;
                return;
            }
            if self.runtime.previous_default_source.is_none() {
                self.runtime.previous_default_source =
                    Some(self.configured_default_raw.clone().unwrap_or_default());
                if self.persist {
                    let _ = self.runtime.save();
                }
            }
            md.set_property(
                0,
                CONFIGURED_DEFAULT_SOURCE,
                Some("Spa:String:JSON"),
                Some(&ours),
            );
            self.overrode_default = true;
            log::info!("Sordino Mic set as default microphone");
        } else if self.overrode_default || self.runtime.previous_default_source.is_some() {
            self.restore_default();
        }
    }

    fn restore_default(&mut self) {
        let Some((_, md, _)) = &self.metadata else {
            return;
        };
        let Some(prev) = self.runtime.previous_default_source.take() else {
            self.overrode_default = false;
            return;
        };
        if prev.is_empty() || prev.contains(VIRTUAL_MIC_NAME) {
            md.set_property(0, CONFIGURED_DEFAULT_SOURCE, None, None);
        } else {
            md.set_property(
                0,
                CONFIGURED_DEFAULT_SOURCE,
                Some("Spa:String:JSON"),
                Some(&prev),
            );
        }
        self.overrode_default = false;
        if self.persist {
            let _ = self.runtime.save();
        }
        log::info!("default microphone restored");
    }

    pub fn shutdown(&mut self) {
        log::info!("shutting down");
        self.restore_default();
        self.teardown_chain(true);
    }

    // -----------------------------------------------------------------------------------------
    // Periodic work (called from a timer every 100 ms)
    // -----------------------------------------------------------------------------------------

    pub fn tick(&mut self) {
        if let Some(t) = self.quit_at {
            if Instant::now() >= t {
                if let Some(ml) = self.mainloop.upgrade() {
                    ml.quit();
                }
            }
            return;
        }
        self.try_connect();
        if self.core.is_none() {
            self.flush_if_dirty();
            return;
        }

        if self.retry_at.is_some_and(|t| Instant::now() >= t) {
            self.reconcile();
            self.dirty();
        }
        if self.monitoring && self.monitor_until.is_some_and(|t| Instant::now() >= t) {
            log::info!("monitoring switched off: the client stopped renewing it");
            self.monitoring = false;
            self.ab_original = false;
            self.monitor_until = None;
            self.reconcile();
            self.dirty();
        }
        if !self.default_wait_done && Instant::now() >= self.default_wait_until {
            self.default_wait_done = true;
            self.reconcile();
            self.dirty();
        }

        if self.last_health.elapsed() >= Duration::from_millis(500) {
            self.last_health = Instant::now();
            self.health_check();
        }
        self.flush_if_dirty();
    }

    fn health_check(&mut self) {
        let mut changed = false;
        if let Some(w) = &self.worker {
            match w.health() {
                WorkerHealth::Failed(msg) => return self.fail_chain(msg),
                WorkerHealth::Ready
                    if self.status == Status::Starting && self.capture.is_some() =>
                {
                    self.status = Status::Running;
                    self.stable_since = Some(Instant::now());
                    changed = true;
                }
                _ => {}
            }
        }
        let stream_error = [
            ("capture", self.capture.as_ref()),
            ("virtual-mic", self.virtual_mic.as_ref()),
        ]
        .into_iter()
        .find_map(|(name, s)| {
            s.and_then(audio::stream_failed)
                .map(|err| format!("{name}: {err}"))
        });
        if let Some(msg) = stream_error {
            return self.fail_chain(msg);
        }
        if self
            .stable_since
            .is_some_and(|t| t.elapsed() > Duration::from_secs(30))
            && self.failures > 0
        {
            self.failures = 0;
        }
        if changed {
            self.dirty();
        }
    }

    fn flush_if_dirty(&mut self) {
        if self
            .dirty_since
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(30))
        {
            self.dirty_since = None;
            self.publish();
        }
    }

    // -----------------------------------------------------------------------------------------
    // State
    // -----------------------------------------------------------------------------------------

    fn build_state(&self) -> State {
        let mut visible = Vec::new();
        let mut hidden = Vec::new();
        let mut sources: Vec<&SourceNode> = self.sources.values().collect();
        sources.sort_by_key(|s| s.node_id);
        for s in sources {
            let d: Device = s.to_device(&self.cards);
            if s.hidden_by_default() {
                hidden.push(d);
            } else {
                visible.push(d);
            }
        }

        let profile_hint = self.capture_target.as_ref().and_then(|name| {
            let src = self.sources.values().find(|s| &s.name == name)?;
            let card_id = src.card?;
            let card = self.cards.get(&card_id)?;
            let current = card
                .profiles
                .iter()
                .find(|p| Some(p.index) == card.active)?;
            if !is_unfavourable(current.kind) {
                return None;
            }
            let suggested = suggest(&card.profiles, card.active)?;
            Some(ProfileHint {
                card: card_id,
                device_id: src.name.clone(),
                device_name: src.description.clone(),
                current: current.clone(),
                suggested: suggested.clone(),
            })
        });

        let latency_ms = self
            .worker
            .as_ref()
            .filter(|_| self.status == Status::Running)
            .map(|w| {
                // processing + the Sordino Mic jitter cushion + one hop of block alignment
                (w.latency_samples() + HOP * 3) as f32 * 1000.0 / SAMPLE_RATE as f32
            });

        State {
            version: env!("CARGO_PKG_VERSION").to_string(),
            status: self.status,
            error: self.error.clone(),
            settings: self.settings.clone(),
            devices: visible,
            hidden_devices: hidden,
            active_mic: self.capture_target.clone(),
            sinks: {
                let mut v: Vec<&SourceNode> = self.sinks.values().collect();
                v.sort_by_key(|n| n.node_id);
                v.into_iter()
                    .map(|n| n.to_sink_device(&self.cards))
                    .collect()
            },
            default_source: self.default_source.clone(),
            default_sink: self.default_sink.clone(),
            profile_hint,
            latency_ms,
            default_is_sordino: self.default_source.as_deref() == Some(VIRTUAL_MIC_NAME),
            monitoring: self.monitor.is_some(),
            ab_original: self.ab_original,
            echo_available: self.worker.as_ref().is_some_and(|w| {
                w.shared
                    .echo_available
                    .load(std::sync::atomic::Ordering::Relaxed)
            }),
            presets: sordino_core::ipc::builtin_presets(),
            diag: self.diag(),
        }
    }

    fn diag(&self) -> sordino_core::ipc::Diag {
        use std::sync::atomic::Ordering::Relaxed;
        sordino_core::ipc::Diag {
            out_underruns: self.stats.out_underruns.load(Relaxed),
            out_skipped: self.stats.out_skipped.load(Relaxed),
            in_dropped: self.stats.in_dropped.load(Relaxed),
            model_errors: self
                .worker
                .as_ref()
                .map_or(0, |w| w.shared.model_errors.load(Relaxed) as u64),
            quantum: self.stats.quantum.load(Relaxed),
            out_callbacks: self.stats.out_callbacks.load(Relaxed),
            overload_hops: self
                .worker
                .as_ref()
                .map_or(0, |w| w.shared.overload_hops.load(Relaxed)),
            overload_events: self
                .worker
                .as_ref()
                .map_or(0, |w| w.shared.overload_events.load(Relaxed) as u64),
            dsp_priority: self
                .worker
                .as_ref()
                .map_or(0, |w| w.shared.priority.load(Relaxed)),
            skip_events: self.stats.skip_events.load(Relaxed),
            last_skip_cb: self.stats.last_skip_cb.load(Relaxed),
            last_drop_cycle: self.stats.last_drop_cycle.load(Relaxed),
        }
    }

    fn publish(&self) {
        match serde_json::to_string(&self.build_state()) {
            Ok(json) => {
                *self.shared.state_json.lock().unwrap() = json;
                let _ = self.events.send(Event::StateChanged);
            }
            Err(e) => log::error!("cannot serialise state: {e}"),
        }
    }
}
