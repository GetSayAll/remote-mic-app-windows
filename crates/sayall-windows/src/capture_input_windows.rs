use super::{apply, restore, CaptureInputSnapshot, Roles, RouteBackend, Transaction};
use crate::{audio::AudioBeginGuard, AudioEndpoint};
use sayall_core::CaptureInputSettings;
use std::{
    ffi::c_void,
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::{IUnknown, IUnknown_Vtbl, Interface, GUID, HRESULT, PCWSTR},
    Win32::{
        Media::Audio::{eCapture, ERole, IMMDeviceEnumerator, MMDeviceEnumerator},
        Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
            COINIT_MULTITHREADED,
        },
    },
};

const REQUEST_BOUND: Duration = Duration::from_millis(1500);
const RECONCILE_INTERVAL: Duration = Duration::from_millis(50);

// The deliberately narrow undocumented ABI, independently bound from the public
// EreTIk IPolicyConfig header. Only SetDefaultEndpoint is ever called (slot 13).
// No Vista fallback, format/property/session-routing methods or arbitrary IID API.
#[repr(transparent)]
#[derive(Clone)]
struct PolicyConfig(IUnknown);
unsafe impl Interface for PolicyConfig {
    type Vtable = PolicyConfigVtable;
    const IID: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
}
#[repr(C)]
struct PolicyConfigVtable {
    base: IUnknown_Vtbl,
    unused: [usize; 10],
    set_default: unsafe extern "system" fn(*mut c_void, PCWSTR, ERole) -> HRESULT,
    set_visibility_unused: usize,
}
const POLICY_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

fn note(action: &str, result: &str, elapsed: Duration) {
    note_for(0, action, result, elapsed);
}
fn note_for(generation: u64, action: &str, result: &str, elapsed: Duration) {
    crate::gatt_note(format!(
        "capture_input generation={generation} action={action} result={result} elapsed_ms={}",
        elapsed.as_millis()
    ));
}
fn api_error(stage: &str, error: windows::core::Error) -> String {
    format!("{stage}_hr_{:08x}", error.code().0 as u32)
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[derive(Default)]
struct IoCost {
    calls: u64,
    failures: u64,
    elapsed: Duration,
}
impl IoCost {
    fn record<T>(&mut self, elapsed: Duration, result: &Result<T, String>) {
        self.calls += 1;
        self.failures += u64::from(result.is_err());
        self.elapsed += elapsed;
    }
}

#[derive(Default)]
struct PrepareIo {
    roles: IoCost,
    name: IoCost,
    journal: IoCost,
    policy: IoCost,
}
impl PrepareIo {
    fn note(&self, generation: u64, result: &Result<(), String>, elapsed: Duration) {
        let outcome = if result.is_ok() { "passed" } else { "failed" };
        crate::gatt_note(format!("capture_input generation={generation} action=prepare_io result={outcome} roles_calls={} roles_ms={} roles_failures={} name_calls={} name_ms={} name_failures={} journal_calls={} journal_ms={} journal_failures={} policy_calls={} policy_ms={} policy_failures={} elapsed_ms={}",
            self.roles.calls, self.roles.elapsed.as_millis(), self.roles.failures,
            self.name.calls, self.name.elapsed.as_millis(), self.name.failures,
            self.journal.calls, self.journal.elapsed.as_millis(), self.journal.failures,
            self.policy.calls, self.policy.elapsed.as_millis(), self.policy.failures,
            elapsed.as_millis()));
    }
}

struct Native {
    enumerator: IMMDeviceEnumerator,
    policy: Option<PolicyConfig>,
    journal: Option<PathBuf>,
    generation: u64,
    prepare_io: PrepareIo,
    // Only finish() in this process can own automatic zero-write completion.
    // A journal loaded at startup never acquires this ownership.
    normal_recovery: Option<Transaction>,
}
impl Native {
    fn new() -> Result<Self, String> {
        let enumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|e| api_error("enumerator", e))?;
        Ok(Self {
            enumerator,
            policy: None,
            journal: None,
            generation: 0,
            prepare_io: PrepareIo::default(),
            normal_recovery: None,
        })
    }
    fn ensure_policy(&mut self) -> Result<(), String> {
        if self.policy.is_none() {
            let started = Instant::now();
            let result = unsafe { CoCreateInstance(&POLICY_CLIENT, None, CLSCTX_ALL) }
                .map_err(|e| api_error("policy_activate", e));
            self.prepare_io.policy.record(started.elapsed(), &result);
            self.policy = Some(result?);
        }
        Ok(())
    }
    fn list(&self) -> Result<Vec<AudioEndpoint>, String> {
        let en = wasapi::DeviceEnumerator::new().map_err(|_| "enumerator_failed")?;
        let devices = en
            .get_device_collection(&wasapi::Direction::Capture)
            .map_err(|_| "capture_enumeration_failed")?;
        let mut result = Vec::new();
        for device in &devices {
            let device = device.map_err(|_| "capture_device_failed")?;
            let id = device.get_id().map_err(|_| "capture_id_failed")?;
            let name = device
                .get_friendlyname()
                .map_err(|_| "capture_name_failed")?;
            result.push(AudioEndpoint {
                id,
                is_virtual_cable_candidate: name.to_ascii_lowercase().contains("cable output"),
                name,
            });
        }
        Ok(result)
    }
    fn load_journal(&mut self) -> Result<Option<Transaction>, String> {
        let started = Instant::now();
        let result = (|| {
            let path = self.journal.as_ref().ok_or("journal_not_initialized")?;
            match fs::read(path) {
                Ok(bytes) if bytes.len() <= 65536 => {
                    let value: Option<Transaction> =
                        serde_json::from_slice(&bytes).map_err(|_| "journal_invalid")?;
                    if value.as_ref().is_some_and(|v| v.version != 1) {
                        return Err("journal_version".into());
                    }
                    Ok(value)
                }
                Ok(_) => Err("journal_too_large".into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err("journal_read_failed".into()),
            }
        })();
        self.prepare_io.journal.record(started.elapsed(), &result);
        result
    }
}
impl RouteBackend for Native {
    fn observe(&mut self, stage: &str, tx: &Transaction, actual: &Roles) {
        let mask = |matches: &dyn Fn(usize) -> bool| -> u8 {
            (0..3).fold(0, |bits, i| bits | if matches(i) { 1 << i } else { 0 })
        };
        let target = mask(&|i| actual[i].as_deref() == Some(tx.target.as_str()));
        let original = mask(&|i| actual[i] == tx.original[i]);
        let expected = mask(&|i| actual[i] == tx.expected[i]);
        let group = tx.transition.as_ref().map_or(0, |t| t.mask);
        let desired = tx
            .transition
            .as_ref()
            .map_or(0, |t| mask(&|i| actual[i] == t.desired[i]));
        crate::gatt_note(format!("capture_input generation={} action=role_vector stage={stage} target_mask={target} original_mask={original} expected_mask={expected} transition_mask={group} desired_mask={desired}", tx.generation));
    }
    fn roles(&mut self) -> Result<Roles, String> {
        let started = Instant::now();
        let result = (|| {
            let mut roles = [None, None, None];
            for (i, slot) in roles.iter_mut().enumerate() {
                let device = unsafe {
                    self.enumerator
                        .GetDefaultAudioEndpoint(eCapture, ERole(i as i32))
                }
                .map_err(|e| api_error("default_read", e))?;
                let id = unsafe { device.GetId() }.map_err(|e| api_error("default_id", e))?;
                let value = unsafe { id.to_string() };
                unsafe { CoTaskMemFree(Some(id.0.cast())) };
                *slot = Some(value.map_err(|_| "default_id_invalid")?);
            }
            Ok(roles)
        })();
        self.prepare_io.roles.record(started.elapsed(), &result);
        result
    }
    fn active_name(&mut self, id: &str) -> Result<String, String> {
        let started = Instant::now();
        let result = (|| {
            // Enumeration is capture + active only. A render ID is never accepted.
            self.list()?
                .into_iter()
                .find(|v| v.id == id)
                .map(|v| v.name)
                .ok_or_else(|| "capture_endpoint_missing".into())
        })();
        self.prepare_io.name.record(started.elapsed(), &result);
        result
    }
    fn set(&mut self, role: usize, id: &str) -> Result<(), String> {
        if role >= 3 {
            return Err("role_invalid".into());
        }
        let policy = self.policy.as_ref().ok_or("policy_not_initialized")?;
        let endpoint = wide(id);
        let started = Instant::now();
        let result = unsafe {
            (policy.vtable().set_default)(
                policy.as_raw(),
                PCWSTR(endpoint.as_ptr()),
                ERole(role as i32),
            )
            .ok()
        }
        .map_err(|e| api_error("set_default", e));
        self.prepare_io.policy.record(started.elapsed(), &result);
        note_for(
            self.generation,
            &format!("set_role_{role}"),
            result
                .as_ref()
                .map(|_| "passed")
                .unwrap_or_else(|e| e.as_str()),
            started.elapsed(),
        );
        result
    }
    fn persist(&mut self, value: Option<&Transaction>) -> Result<(), String> {
        let started = Instant::now();
        let result = (|| {
            let path = self.journal.as_ref().ok_or("journal_not_initialized")?;
            let parent = path.parent().ok_or("journal_parent_invalid")?;
            fs::create_dir_all(parent).map_err(|_| "journal_directory_failed")?;
            let pending = path.with_extension("pending");
            let bytes = serde_json::to_vec(&value).map_err(|_| "journal_encode_failed")?;
            let mut file = fs::File::create(&pending).map_err(|_| "journal_create_failed")?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "journal_flush_failed")?;
            drop(file);
            let from = wide(&pending.to_string_lossy());
            let to = wide(&path.to_string_lossy());
            unsafe {
                MoveFileExW(
                    PCWSTR(from.as_ptr()),
                    PCWSTR(to.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            }
            .map_err(|e| api_error("journal_replace", e))
        })();
        self.prepare_io.journal.record(started.elapsed(), &result);
        result
    }
}

pub(crate) struct RoutePermit {
    epoch: Arc<AtomicU64>,
    expected: u64,
    deadline: Instant,
}
impl RoutePermit {
    pub(crate) fn cancelled(&self) -> bool {
        self.epoch.load(Ordering::SeqCst) != self.expected || Instant::now() >= self.deadline
    }
}
#[derive(Clone)]
struct Begin {
    generation: u64,
    guard: AudioBeginGuard,
    epoch: u64,
    timeout: Arc<AtomicBool>,
    deadline: Instant,
}
impl Begin {
    fn cancelled_at(&self, epoch: &AtomicU64, now: Instant) -> bool {
        self.guard.cancelled()
            || self.timeout.load(Ordering::SeqCst)
            || epoch.load(Ordering::SeqCst) != self.epoch
            || now >= self.deadline
    }
    fn end_requested(&self, epoch: &AtomicU64) -> bool {
        self.timeout.load(Ordering::SeqCst) || epoch.load(Ordering::SeqCst) != self.epoch
    }
    fn confirm_reply(
        &self,
        epoch: &AtomicU64,
        now: Instant,
        result: Result<(), String>,
    ) -> Result<(), String> {
        result?;
        if self.cancelled_at(epoch, now) {
            Err("cancelled_or_timed_out".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone)]
struct MutationGuard {
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}
impl MutationGuard {
    fn new() -> Self {
        Self {
            deadline: Instant::now() + REQUEST_BOUND,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::SeqCst) || Instant::now() >= self.deadline {
            Err("route_operation_expired".into())
        } else {
            Ok(())
        }
    }
}
enum Command {
    Initialize(PathBuf, CaptureInputSettings, Sender<Result<(), String>>),
    Configure(
        CaptureInputSettings,
        MutationGuard,
        Sender<Result<(), String>>,
    ),
    List(Sender<Result<Vec<AudioEndpoint>, String>>),
    Begin(Begin, Sender<Result<(), String>>),
    End,
    Reconcile,
    Recover(bool, MutationGuard, Sender<Result<(), String>>),
    Shutdown(Sender<Result<(), String>>),
}
pub(crate) struct CaptureInputRuntime {
    sender: SyncSender<Command>,
    state: Arc<Mutex<CaptureInputSnapshot>>,
    epoch: Arc<AtomicU64>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
    pub(crate) config_gate: Arc<Mutex<()>>,
    configuration_uncertain: AtomicBool,
}
impl CaptureInputRuntime {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel(16);
        let state = Arc::new(Mutex::new(CaptureInputSnapshot {
            phase: "disabled".into(),
            ..Default::default()
        }));
        let epoch = Arc::new(AtomicU64::new(0));
        let s = state.clone();
        let e = epoch.clone();
        let notify_sender = sender.clone();
        let worker = thread::Builder::new()
            .name("sayall-capture-route".into())
            .spawn(move || run(receiver, notify_sender, s, e))
            .ok();
        Self {
            sender,
            state,
            epoch,
            worker: Mutex::new(worker),
            config_gate: Arc::new(Mutex::new(())),
            configuration_uncertain: AtomicBool::new(false),
        }
    }
    pub(crate) fn snapshot(&self) -> CaptureInputSnapshot {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn request<T>(
        &self,
        message: impl FnOnce(Sender<Result<T, String>>) -> Command,
    ) -> Result<T, String> {
        let (reply, receive) = mpsc::channel();
        self.sender.try_send(message(reply)).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => "route_worker_busy",
            mpsc::TrySendError::Disconnected(_) => "route_worker_unavailable",
        })?;
        receive.recv_timeout(REQUEST_BOUND).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => "route_request_timeout",
            mpsc::RecvTimeoutError::Disconnected => "route_worker_unavailable",
        })?
    }
    pub(crate) fn initialize(
        &self,
        path: PathBuf,
        settings: CaptureInputSettings,
    ) -> Result<(), String> {
        self.request(|r| Command::Initialize(path, settings, r))
    }
    fn mutate(
        &self,
        make: impl FnOnce(MutationGuard, Sender<Result<(), String>>) -> Command,
    ) -> Result<(), String> {
        let guard = MutationGuard::new();
        let request_guard = guard.clone();
        let result = self.request(|r| make(request_guard, r));
        if result.is_err() {
            guard.cancelled.store(true, Ordering::SeqCst);
        }
        result
    }
    pub(crate) fn configure(&self, settings: CaptureInputSettings) -> Result<(), String> {
        let result = self.mutate(|g, r| Command::Configure(settings, g, r));
        if result.as_ref().is_err_and(|e| e == "route_request_timeout") {
            self.configuration_uncertain.store(true, Ordering::SeqCst);
            publish(
                &self.state,
                "configuration_pending",
                Some("等待配置操作收尾，请重试保存；本次不启动语音".into()),
                self.snapshot().recovery_pending,
            );
        } else if result.is_ok() {
            self.configuration_uncertain.store(false, Ordering::SeqCst);
        }
        result
    }
    pub(crate) fn list(&self) -> Result<Vec<AudioEndpoint>, String> {
        self.request(Command::List)
    }
    pub(crate) fn recover(&self, restore: bool) -> Result<(), String> {
        self.mutate(|g, r| Command::Recover(restore, g, r))
    }
    pub(crate) fn begin(
        &self,
        generation: u64,
        guard: Option<AudioBeginGuard>,
    ) -> Result<Option<RoutePermit>, String> {
        // The same gate is held by Tauri across configure + durable settings +
        // rollback. BLE cannot consume a transient, unpersisted configuration.
        let _configuration = self
            .config_gate
            .try_lock()
            .map_err(|_| "capture_configuration_in_progress")?;
        if self.configuration_uncertain.load(Ordering::SeqCst) {
            return Err("capture_configuration_pending".into());
        }
        if !self.snapshot().settings.enabled {
            return Ok(None);
        }
        let guard = guard.ok_or("route_guard_missing")?;
        let epoch = self.epoch.load(Ordering::SeqCst);
        let timeout = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now() + REQUEST_BOUND;
        let request = Begin {
            generation,
            guard,
            epoch,
            timeout: timeout.clone(),
            deadline,
        };
        let caller = request.clone();
        let result = self.request(|r| Command::Begin(request, r));
        // A queued successful reply still must pass the absolute deadline and source guard.
        let result = caller.confirm_reply(&self.epoch, Instant::now(), result);
        if result.is_err() {
            timeout.store(true, Ordering::SeqCst);
            let _ = self.sender.try_send(Command::Reconcile);
        }
        result.map(|_| {
            Some(RoutePermit {
                epoch: self.epoch.clone(),
                expected: epoch,
                deadline,
            })
        })
    }
    pub(crate) fn end(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        // Even if full, the worker's epoch check handles cancellation after in-flight COM.
        let _ = self.sender.try_send(Command::End);
    }
    pub(crate) fn shutdown(&self) -> Result<(), String> {
        self.end();
        let (done, receive) = mpsc::channel();
        self.sender
            .try_send(Command::Shutdown(done))
            .map_err(|_| "route_shutdown_busy")?;
        let result = receive
            .recv_timeout(REQUEST_BOUND)
            .map_err(|_| "route_shutdown_pending")?;
        if let Some(worker) = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take() {
            worker.join().map_err(|_| "route_worker_panicked")?;
        }
        result
    }
}
impl Drop for CaptureInputRuntime {
    fn drop(&mut self) {
        if self
            .worker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
        {
            if let Err(e) = self.shutdown() {
                note("shutdown", &e, Duration::ZERO);
            }
        }
    }
}
struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}
fn publish(
    state: &Mutex<CaptureInputSnapshot>,
    phase: &str,
    error: Option<String>,
    recovery: bool,
) {
    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
    s.phase = phase.into();
    s.last_error = error;
    s.recovery_pending = recovery;
}
fn finish(
    io: &mut Native,
    active: &mut Option<(Transaction, Begin)>,
    state: &Mutex<CaptureInputSnapshot>,
) {
    let Some((mut tx, _)) = active.take() else {
        return;
    };
    let started = Instant::now();
    io.generation = tx.generation;
    match restore(io, &mut tx) {
        Ok(()) => {
            io.normal_recovery = None;
            publish(state, "idle", None, false);
            note("restore", "passed", started.elapsed());
        }
        Err(e) if e == "external_change" => {
            io.normal_recovery = None;
            let cleared = io.persist(None);
            publish(
                state,
                "relinquished",
                cleared.as_ref().err().cloned(),
                cleared.is_err(),
            );
            note(
                "restore",
                "external_change_preserved_all_roles",
                started.elapsed(),
            );
        }
        Err(e) => {
            io.normal_recovery = Some(tx);
            publish(state, "recovery_required", Some(e.clone()), true);
            note("restore", &e, started.elapsed());
        }
    }
}
fn reconcile_normal_recovery(io: &mut Native, state: &Mutex<CaptureInputSnapshot>) {
    let Some(tx) = io.normal_recovery.clone() else {
        return;
    };
    let started = Instant::now();
    match io.load_journal() {
        Ok(Some(saved)) if saved == tx => {}
        _ => {
            note_for(
                tx.generation,
                "recovery_reconcile",
                "journal_ownership_unconfirmed",
                started.elapsed(),
            );
            return;
        }
    }
    match super::complete_if_already_restored(io, &tx, || Ok(())) {
        Ok(true) => {
            io.normal_recovery = None;
            publish(state, "idle", None, false);
            note_for(
                tx.generation,
                "recovery_reconcile",
                "already_restored_zero_writes",
                started.elapsed(),
            );
        }
        Ok(false) => note_for(
            tx.generation,
            "recovery_reconcile",
            "original_vector_not_confirmed",
            started.elapsed(),
        ),
        Err(e) => {
            publish(state, "recovery_required", Some(e.clone()), true);
            note_for(tx.generation, "recovery_reconcile", &e, started.elapsed());
        }
    }
}
fn run(
    receiver: Receiver<Command>,
    notify_sender: SyncSender<Command>,
    state: Arc<Mutex<CaptureInputSnapshot>>,
    epoch: Arc<AtomicU64>,
) {
    if let Err(e) = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok() {
        publish(&state, "failed", Some(api_error("com_init", e)), false);
        return;
    }
    let _apartment = Apartment;
    let mut io = match Native::new() {
        Ok(v) => v,
        Err(e) => {
            publish(&state, "failed", Some(e), false);
            return;
        }
    };
    let dirty = Arc::new(AtomicBool::new(false));
    let mut callbacks = wasapi::DeviceEventCallbacks::new();
    let signal = {
        let dirty = dirty.clone();
        move || {
            if !dirty.swap(true, Ordering::SeqCst)
                && notify_sender.try_send(Command::Reconcile).is_err()
            {
                dirty.store(false, Ordering::SeqCst);
            }
        }
    };
    callbacks.set_default_device_callback(move |direction, _, _| {
        if direction == wasapi::Direction::Capture {
            signal();
        }
    });
    let notifications =
        wasapi::DeviceEnumerator::new().and_then(|v| v.register_notification_callback(callbacks));
    let _registration = match notifications {
        Ok(v) => v,
        Err(_) => {
            publish(
                &state,
                "failed",
                Some("notification_registration_failed".into()),
                false,
            );
            return;
        }
    };
    let mut active: Option<(Transaction, Begin)> = None;
    loop {
        // A public default-role read is the arbiter. No notification actor/time-window
        // inference. Polling is confined to an active lease and also catches missed events.
        if let Some((tx, b)) = active.as_ref() {
            // Source release only cancels startup. Once handed to BLE, that
            // owner must release its hotkey before end() permits restoration.
            if b.end_requested(&epoch) {
                finish(&mut io, &mut active, &state);
            } else if let Err(e) = super::check_expected(&mut io, tx) {
                if e == "external_change" {
                    epoch.fetch_add(1, Ordering::SeqCst);
                    let cleared = io.persist(None);
                    active = None;
                    publish(
                        &state,
                        "relinquished",
                        cleared.as_ref().err().cloned(),
                        cleared.is_err(),
                    );
                    note(
                        "reconcile",
                        "external_change_preserved_all_roles",
                        Duration::ZERO,
                    );
                } else {
                    finish(&mut io, &mut active, &state);
                }
            }
        }
        let command = if active.is_some() {
            match receiver.recv_timeout(RECONCILE_INTERVAL) {
                Ok(v) => v,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        } else {
            match receiver.recv() {
                Ok(v) => v,
                Err(_) => break,
            }
        };
        match command {
            Command::Initialize(path, config, reply) => {
                let result = if io.journal.is_some() {
                    Err("already_initialized".into())
                } else {
                    io.journal = Some(path);
                    state.lock().unwrap_or_else(|e| e.into_inner()).settings = config;
                    match io.load_journal() {
                        Ok(Some(_)) => {
                            publish(&state, "recovery_required", None, true);
                            Ok(())
                        }
                        Ok(None) => {
                            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                            s.phase = if s.settings.enabled {
                                "idle"
                            } else {
                                "disabled"
                            }
                            .into();
                            Ok(())
                        }
                        Err(e) => {
                            publish(&state, "recovery_required", Some(e.clone()), true);
                            Err(e)
                        }
                    }
                };
                let _ = reply.send(result);
            }
            Command::List(reply) => {
                let _ = reply.send(io.list());
            }
            Command::Configure(config, request, reply) => {
                let result = (|| {
                    request.check()?;
                    // No controller-side restore while the voice owner's hotkey is DOWN.
                    if active.is_some() {
                        return Err("voice_session_active_release_before_configuring".into());
                    }
                    if config.enabled {
                        let id = config
                            .endpoint_id
                            .as_deref()
                            .ok_or("capture_target_unselected")?;
                        if Some(io.active_name(id)?) != config.endpoint_name {
                            return Err("capture_target_changed".into());
                        }
                    }
                    request.check()?;
                    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                    if !s.recovery_pending {
                        s.phase = if config.enabled { "idle" } else { "disabled" }.into();
                    }
                    s.settings = config;
                    Ok(())
                })();
                note(
                    "configure",
                    result
                        .as_ref()
                        .map(|_| "passed")
                        .unwrap_or_else(|e: &String| e.as_str()),
                    Duration::ZERO,
                );
                let _ = reply.send(result);
            }
            Command::Begin(b, reply) => {
                // A duplicate/new START must not restore an existing lease before
                // BLE releases its old chord through the common abort path.
                if active.is_some() {
                    let _ = reply.send(Err("route_voice_session_busy".into()));
                    continue;
                }
                let started = Instant::now();
                io.generation = b.generation;
                io.prepare_io = PrepareIo::default();
                let config = state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .settings
                    .clone();
                let cancelled = || b.cancelled_at(&epoch, Instant::now());
                let mut tx = None;
                let mut result = (|| {
                    if cancelled() {
                        return Err("cancelled".into());
                    }
                    if !config.enabled {
                        return Err("configuration_changed".into());
                    }
                    // Continue this same DOWN after a proven zero-write recovery;
                    // do not consume an extra preparation press or start a timer.
                    reconcile_normal_recovery(&mut io, &state);
                    if state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .recovery_pending
                        || io.load_journal()?.is_some()
                    {
                        return Err("recovery_required".into());
                    }
                    let target = config.endpoint_id.ok_or("capture_target_unselected")?;
                    let target_name = config.endpoint_name.ok_or("capture_target_unselected")?;
                    if io.active_name(&target)? != target_name {
                        return Err("capture_target_changed".into());
                    }
                    io.ensure_policy()?;
                    if cancelled() {
                        return Err("cancelled".into());
                    }
                    let original = io.roles()?;
                    let mut original_names = [None, None, None];
                    for i in 0..3 {
                        if let Some(id) = &original[i] {
                            original_names[i] = Some(io.active_name(id)?);
                        }
                    }
                    tx = Some(Transaction {
                        version: 1,
                        generation: b.generation,
                        target,
                        target_name,
                        original: original.clone(),
                        original_names,
                        expected: original,
                        changed: [false; 3],
                        transition: None,
                    });
                    apply(&mut io, tx.as_mut().unwrap(), cancelled)
                })();
                io.prepare_io.note(b.generation, &result, started.elapsed());
                note_for(
                    b.generation,
                    "prepare",
                    result
                        .as_ref()
                        .map(|_| "default_roles_confirmed")
                        .unwrap_or_else(|e: &String| e.as_str()),
                    started.elapsed(),
                );
                result = b.confirm_reply(&epoch, Instant::now(), result);
                match result {
                    Ok(()) => {
                        active = Some((tx.unwrap(), b));
                        publish(&state, "active", None, false);
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        if e == "external_change" {
                            let cleared = io.persist(None);
                            publish(
                                &state,
                                "relinquished",
                                cleared.as_ref().err().cloned(),
                                cleared.is_err(),
                            );
                        } else if let Some(tx) = tx {
                            active = Some((tx, b));
                            finish(&mut io, &mut active, &state);
                        }
                        let mut s = state.lock().unwrap_or_else(|v| v.into_inner());
                        if e != "recovery_required" {
                            s.last_error = Some(e.clone());
                        }
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Command::Reconcile => {
                dirty.store(false, Ordering::SeqCst);
                reconcile_normal_recovery(&mut io, &state);
            }
            // Epoch ownership, not command arrival order, decides which lease ends.
            Command::End => {}
            Command::Recover(should_restore, request, reply) => {
                let result = (|| {
                    request.check()?;
                    if active.is_some() {
                        return Err("voice_session_active".into());
                    }
                    if !should_restore {
                        request.check()?;
                        io.persist(None)?;
                        io.normal_recovery = None;
                        publish(&state, "idle", None, false);
                        return Ok(());
                    }
                    if let Some(mut tx) = io.load_journal()? {
                        io.generation = tx.generation;
                        if super::complete_if_already_restored(&mut io, &tx, || request.check())? {
                            note_for(
                                tx.generation,
                                "recovery_choice",
                                "already_restored_zero_writes",
                                Duration::ZERO,
                            );
                        } else {
                            io.ensure_policy()?;
                            let actual = io.roles()?;
                            super::validate_recovery(&tx, &actual)?;
                            super::confirm_actual(&mut tx, actual);
                            io.persist(Some(&tx))?;
                            super::restore_checked(&mut io, &mut tx, || request.check())?;
                        }
                    }
                    io.normal_recovery = None;
                    publish(&state, "idle", None, false);
                    Ok(())
                })();
                note(
                    "recovery_choice",
                    result
                        .as_ref()
                        .map(|_| "passed")
                        .unwrap_or_else(|e: &String| e.as_str()),
                    Duration::ZERO,
                );
                let _ = reply.send(result);
            }
            Command::Shutdown(done) => {
                finish(&mut io, &mut active, &state);
                reconcile_normal_recovery(&mut io, &state);
                let s = state.lock().unwrap_or_else(|e| e.into_inner());
                let result = if s.recovery_pending {
                    Err("route_restore_unconfirmed".into())
                } else {
                    Ok(())
                };
                let _ = done.send(result);
                break;
            }
        }
    }
    finish(&mut io, &mut active, &state);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending() -> (Begin, Arc<AtomicU64>, Arc<AtomicU64>) {
        let release = Arc::new(AtomicU64::new(0));
        let epoch = Arc::new(AtomicU64::new(0));
        let b = Begin {
            generation: 1,
            guard: AudioBeginGuard::new(release.clone(), Arc::new(AtomicU64::new(0))),
            epoch: 0,
            timeout: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + REQUEST_BOUND,
        };
        (b, release, epoch)
    }
    #[test]
    fn successful_apply_or_queued_reply_after_deadline_is_rejected() {
        let (b, _, epoch) = pending();
        assert!(b
            .confirm_reply(&epoch, b.deadline - Duration::from_nanos(1), Ok(()))
            .is_ok());
        assert!(b.confirm_reply(&epoch, b.deadline, Ok(())).is_err());
    }
    #[test]
    fn source_release_after_apply_before_reply_prevents_start() {
        let (b, release, epoch) = pending();
        release.fetch_add(1, Ordering::SeqCst);
        assert!(b.confirm_reply(&epoch, Instant::now(), Ok(())).is_err());
    }
    #[test]
    fn external_change_after_reply_invalidates_startup_permit() {
        let (b, _, epoch) = pending();
        let permit = RoutePermit {
            epoch: epoch.clone(),
            expected: 0,
            deadline: b.deadline,
        };
        assert!(!permit.cancelled());
        epoch.fetch_add(1, Ordering::SeqCst);
        assert!(permit.cancelled());
    }
    #[test]
    fn source_up_disconnect_or_sleep_does_not_restore_before_ble_end() {
        let (b, release, epoch) = pending();
        release.fetch_add(1, Ordering::SeqCst);
        assert!(b.guard.cancelled());
        assert!(!b.end_requested(&epoch));
        // BLE now releases the target chord, then audio finish/interrupt calls end.
        epoch.fetch_add(1, Ordering::SeqCst);
        assert!(b.end_requested(&epoch));
    }
    #[test]
    fn timed_out_queued_config_and_recovery_are_rejected_before_effects() {
        let request = MutationGuard::new();
        let queued = request.clone();
        request.cancelled.store(true, Ordering::SeqCst);
        let mut effects = 0;
        if queued.check().is_ok() {
            effects += 1;
        }
        assert_eq!(effects, 0);
        let expired = MutationGuard {
            deadline: Instant::now(),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        assert!(expired.check().is_err());
    }
    #[test]
    fn configuration_reservation_prevents_ble_from_observing_unpersisted_value() {
        let runtime = CaptureInputRuntime::new();
        let reservation = runtime.config_gate.lock().unwrap();
        assert!(runtime.begin(1, None).is_err());
        drop(reservation);
        assert!(runtime.begin(1, None).unwrap().is_none());
        runtime.shutdown().unwrap();
    }
}
