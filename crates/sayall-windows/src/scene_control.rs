//! Program-default templates and our own interactive template menu.
//! Third-party controls are never queried; mapped keys run in ButtonMappingRuntime.

use crate::application_control::{
    ApplicationControlBackend, ApplicationControlError, ApplicationController, WindowToken,
};
use crate::button_mapping::{FiredGesture, GestureDisposition, RoutedGesture};
use crate::raw_input::{ButtonEdge, RemoteButton};
use crate::send_input::{
    ButtonAction, ButtonActions, ButtonMappings, ButtonTrigger, KeyChord, KeyCode, TaskSwitchView,
};
use crate::templates::MappingConfiguration;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const PROGRAM_QUEUE_CAPACITY: usize = 32;
// Failure budget for an unconfirmed Shell handoff, not an input/gesture delay.
const TASK_TARGET_TIMEOUT: Duration = Duration::from_secs(10);
static VOICE_SCENE: std::sync::OnceLock<Mutex<Weak<SceneController>>> = std::sync::OnceLock::new();

pub(crate) fn register_voice_scene(scene: &Arc<SceneController>) {
    *lock(VOICE_SCENE.get_or_init(|| Mutex::new(Weak::new()))) = Arc::downgrade(scene);
}

pub(crate) fn notify_voice_activity(active: bool) {
    let scene = VOICE_SCENE.get().and_then(|slot| lock(slot).upgrade());
    if let Some(scene) = scene {
        scene.notify_voice_active(active);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenePanel {
    Template,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneMenuItem {
    pub application_id: Option<String>,
    pub template_id: String,
    pub label: String,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneSnapshot {
    pub mapping_notice_enabled: bool,
    pub mapping_notice: Option<MappingNotice>,
    pub mapping_notice_revision: u64,
    pub enabled: bool,
    pub generation: u64,
    pub foreground_generation: u64,
    pub application_id: Option<String>,
    pub template_id: Option<String>,
    pub panel: Option<ScenePanel>,
    pub update_default: bool,
    pub preference_pending: bool,
    pub preference_error: bool,
    pub selected_index: Option<usize>,
    pub menu_items: Vec<SceneMenuItem>,
    pub waiting_for_release: bool,
    pub voice_active: bool,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SceneEvent {
    MappingNoticeEnabled {
        enabled: bool,
    },
    MappingApplied {
        notice: MappingNotice,
        revision: u64,
    },
    Snapshot {
        snapshot: SceneSnapshot,
    },
    DefaultTemplatePersistenceRequested {
        request_id: u64,
        application_id: String,
        template_id: String,
    },
    MenuPreferencePersistenceRequested {
        request_id: u64,
        enabled: bool,
    },
}

pub type SceneEventCallback = Arc<dyn Fn(SceneEvent) + Send + Sync>;

/// Current foreground selection, confirmed by the mapping engine after applying it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingNotice {
    pub kind: String,
    pub template_id: Option<String>,
    pub name: Option<String>,
    pub actions_available: bool,
    pub default_save_status: Option<String>,
}

#[derive(Debug, Clone)]
struct PanelState {
    kind: ScenePanel,
    selected: usize,
}

#[derive(Clone)]
struct TaskSwitchSession {
    origin: WindowToken,
    target: Option<WindowToken>,
    view: TaskSwitchView,
    started_at: Instant,
    tv_actions: ButtonActions,
}

#[derive(Clone)]
struct ManualTemplateSelection {
    // None is an explicit selection of common mappings, not absence of a choice.
    template_id: Option<String>,
    application_id: Option<String>,
    source: &'static str,
}

struct PendingTemplateSelection {
    generation: u64,
    foreground_generation: u64,
    previous: Option<ManualTemplateSelection>,
    deadline: Instant,
    reply: SyncSender<Result<SceneSnapshot, &'static str>>,
}

struct State {
    task_switch: Option<TaskSwitchSession>,
    task_draining: BTreeSet<RemoteButton>,
    task_native_exit: BTreeSet<RemoteButton>,
    manual_selection: Option<ManualTemplateSelection>,
    pending_selection: Option<PendingTemplateSelection>,
    mapping_notice: Option<MappingNotice>,
    mapping_notice_revision: u64,
    configuration: MappingConfiguration,
    generation: u64,
    foreground_generation: u64,
    token: Option<WindowToken>,
    // The actual window that opened the menu is independent of template selection.
    menu_return_target: Option<WindowToken>,
    closing_panel: Option<PanelState>,
    panel: Option<PanelState>,
    template_menu_focused: bool,
    menu_application_id: Option<String>,
    update_default: bool,
    preference_request: Option<(u64, u64, bool)>,
    preference_error: bool,
    default_request_sequence: u64,
    default_request: Option<(u64, String, String)>,
    default_event: Option<SceneEvent>,
    default_save_status: Option<String>,
    menu_native_held: BTreeSet<RemoteButton>,
    menu_native_routed: BTreeSet<RemoteButton>,
    menu_close_after_release: Option<bool>,
    menu_press_generation: Option<u64>,
    held: BTreeSet<RemoteButton>,
    handled_in_cycle: BTreeSet<RemoteButton>,
    waiting_for_release: bool,
    voice_active: bool,
    status: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            task_switch: None,
            task_draining: BTreeSet::new(),
            task_native_exit: BTreeSet::new(),
            manual_selection: None,
            pending_selection: None,
            mapping_notice: None,
            mapping_notice_revision: 0,
            configuration: MappingConfiguration::default(),
            generation: 1,
            foreground_generation: 0,
            token: None,
            menu_return_target: None,
            closing_panel: None,
            panel: None,
            template_menu_focused: false,
            menu_application_id: None,
            update_default: false,
            preference_request: None,
            preference_error: false,
            default_request_sequence: 0,
            default_request: None,
            default_event: None,
            default_save_status: None,
            menu_native_held: BTreeSet::new(),
            menu_native_routed: BTreeSet::new(),
            menu_close_after_release: None,
            menu_press_generation: None,
            held: BTreeSet::new(),
            handled_in_cycle: BTreeSet::new(),
            waiting_for_release: false,
            voice_active: false,
            status: None,
        }
    }
}

enum Work {
    RefreshForeground,
    RestoreForeground {
        token: WindowToken,
        generation: u64,
        expires_at: Instant,
        reply: SyncSender<Result<(), ApplicationControlError>>,
    },
    Shutdown,
}

fn application_error_reason(error: &ApplicationControlError) -> &'static str {
    match error {
        ApplicationControlError::NoForegroundWindow => "foreground_unavailable",
        ApplicationControlError::UnsupportedPlatform => "unsupported_platform",
        ApplicationControlError::IdentityUnavailable => "identity_unavailable",
        ApplicationControlError::StaleToken => "stale_window",
        ApplicationControlError::ModifiersHeld => "modifiers_held",
        ApplicationControlError::WindowOperationFailed => "window_operation_failed",
    }
}

pub struct SceneController {
    application: Arc<dyn ApplicationControlBackend>,
    state: Arc<Mutex<State>>,
    sender: SyncSender<Work>,
    callbacks: Arc<RwLock<Vec<SceneEventCallback>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    #[cfg(windows)]
    foreground_watcher: Mutex<Option<ForegroundWatcher>>,
}

impl SceneController {
    pub fn new() -> Arc<Self> {
        Self::with_backends(Arc::new(ApplicationController::new()), true)
    }

    fn with_backends(
        application: Arc<dyn ApplicationControlBackend>,
        watch_foreground: bool,
    ) -> Arc<Self> {
        let state = Arc::new(Mutex::new(State::default()));
        let callbacks = Arc::new(RwLock::new(Vec::new()));
        let (sender, receiver) = mpsc::sync_channel(PROGRAM_QUEUE_CAPACITY);
        let worker_state = Arc::clone(&state);
        let worker_callbacks = Arc::clone(&callbacks);
        let worker_application = Arc::clone(&application);
        let worker = std::thread::Builder::new()
            .name("sayall-program-selection".to_owned())
            .spawn(move || {
                program_worker(receiver, worker_state, worker_callbacks, worker_application)
            })
            .ok();
        let controller = Arc::new(Self {
            application,
            state,
            sender,
            callbacks,
            worker: Mutex::new(worker),
            #[cfg(windows)]
            foreground_watcher: Mutex::new(None),
        });
        #[cfg(windows)]
        if watch_foreground {
            *lock(&controller.foreground_watcher) =
                ForegroundWatcher::start(controller.sender.clone());
        }
        #[cfg(not(windows))]
        let _ = watch_foreground;
        controller.refresh_foreground();
        controller
    }

    pub fn set_configuration(&self, configuration: MappingConfiguration) -> ButtonMappings {
        {
            let mut state = lock(&self.state);
            cancel_state(&mut state, "configuration_changed");
            let follow_reenabled = !state.configuration.button_mapping_follow_enabled
                && configuration.button_mapping_follow_enabled;
            state.configuration = configuration;
            if state
                .manual_selection
                .as_ref()
                .and_then(|selection| selection.template_id.as_deref())
                .is_some_and(|id| !template_exists(&state.configuration, id))
                || (follow_reenabled && !manual_selection_matches_application(&state))
            {
                state.manual_selection = None;
            }
        }
        self.emit_snapshot();
        self.refresh_foreground();
        self.active_scene_mappings()
    }

    pub fn snapshot(&self) -> SceneSnapshot {
        snapshot_from(&lock(&self.state))
    }

    /// Select the current executable profile without changing program defaults.
    /// Wait for MappingApplied without holding the scene lock before returning.
    pub fn select_template(&self, template_id: Option<&str>) -> Result<SceneSnapshot, String> {
        // This is a bounded engine acknowledgement, never a gesture delay.
        let timeout = Duration::from_secs(1);
        let (reply, receiver) = mpsc::sync_channel(1);
        let generation = {
            let mut state = lock(&self.state);
            let reason = if template_id.is_some_and(|id| !template_exists(&state.configuration, id))
            {
                Some(("template_missing", "所选模板不存在，请刷新后重试。"))
            } else if state.voice_active {
                Some(("voice_active", "请结束当前语音后再切换模板。"))
            } else if !state.held.is_empty()
                || !state.menu_native_held.is_empty()
                || state.waiting_for_release
            {
                Some(("keys_held", "请松开遥控器按键后再切换模板。"))
            } else if state.pending_selection.is_some() {
                Some(("selection_pending", "模板正在切换，请稍后重试。"))
            } else if state.panel.is_some() || state.task_switch.is_some() {
                Some(("menu_active", "请先关闭当前遥控菜单再切换模板。"))
            } else {
                None
            };
            if let Some((reason, message)) = reason {
                crate::gatt_note(format!("template_selection source=buttons_page phase=rejected reason={reason} generation={}", state.generation));
                return Err(message.into());
            }
            let application = state
                .token
                .as_ref()
                .map(|token| token.application_id().to_owned());
            cancel_state(&mut state, "manual_template_selected");
            state.pending_selection = Some(PendingTemplateSelection {
                generation: state.generation,
                foreground_generation: state.foreground_generation,
                previous: state.manual_selection.clone(),
                deadline: Instant::now() + timeout,
                reply,
            });
            apply_manual_selection(
                &mut state,
                template_id.map(str::to_owned),
                application,
                "buttons_page",
            );
            state.generation
        };
        self.emit_snapshot();
        match receiver.recv_timeout(timeout) {
            Ok(Ok(snapshot)) => Ok(snapshot),
            Ok(Err(reason)) => {
                crate::gatt_note(format!("template_selection source=buttons_page phase=cancelled reason={reason} generation={generation}"));
                Err("模板切换期间状态已变化，请重试。".into())
            }
            Err(_) => {
                let cancelled = {
                    let mut state = lock(&self.state);
                    if state
                        .pending_selection
                        .as_ref()
                        .is_some_and(|pending| pending.generation == generation)
                    {
                        cancel_state(&mut state, "selection_ack_timeout");
                        true
                    } else {
                        false
                    }
                };
                if cancelled {
                    self.emit_snapshot();
                } else if let Ok(Ok(snapshot)) = receiver.try_recv() {
                    // The engine committed before its deadline while this caller
                    // was rescheduled. Return that confirmed result, not a guess.
                    return Ok(snapshot);
                }
                crate::gatt_note(format!("template_selection source=buttons_page phase=failed reason=ack_timeout generation={generation} rollback_requested={cancelled}"));
                Err("按键映射尚未确认切换，已取消本次选择，请重试。".into())
            }
        }
    }

    pub fn set_mapping_notice_enabled(&self, enabled: bool) {
        lock(&self.state).configuration.mapping_notice_enabled = enabled;
        // Presentation-only: do not cancel held keys or reapply input mappings.
        emit(
            &self.callbacks,
            SceneEvent::MappingNoticeEnabled { enabled },
        );
    }

    #[cfg(test)]
    pub(crate) fn mapping_notice_candidate(&self) -> (u64, u64, MappingNotice) {
        let state = lock(&self.state);
        (
            state.generation,
            state.foreground_generation,
            mapping_notice_from(&state),
        )
    }

    pub(crate) fn confirm_mapping_notice(
        &self,
        generation: u64,
        foreground: u64,
        mut notice: MappingNotice,
        available: bool,
    ) {
        let (event, source, selection_confirmed) = {
            let mut state = lock(&self.state);
            if state.generation != generation || state.foreground_generation != foreground {
                return;
            }
            if state.pending_selection.as_ref().is_some_and(|pending| {
                pending.generation == generation
                    && pending.foreground_generation == foreground
                    && Instant::now() >= pending.deadline
            }) {
                crate::gatt_note(format!("template_selection source=buttons_page phase=ack_ignored reason=expired generation={generation}"));
                return;
            }
            notice.actions_available =
                available && !matches!(notice.kind.as_str(), "disabled" | "unconfigured");
            let event = if state.mapping_notice.as_ref() != Some(&notice) {
                state.mapping_notice_revision = state.mapping_notice_revision.saturating_add(1);
                state.mapping_notice = Some(notice.clone());
                Some(SceneEvent::MappingApplied {
                    notice,
                    revision: state.mapping_notice_revision,
                })
            } else {
                None
            };
            let selection_confirmed = state.pending_selection.as_ref().is_some_and(|pending| {
                pending.generation == generation && pending.foreground_generation == foreground
            });
            if selection_confirmed {
                if let Some(pending) = state.pending_selection.take() {
                    let _ = pending.reply.try_send(Ok(snapshot_from(&state)));
                }
            }
            (event, selection_source(&state), selection_confirmed)
        };
        if event.is_none() && !selection_confirmed {
            return;
        }
        crate::ble::gatt_note(format!("mapping_notice phase=applied generation={generation} foreground_generation={foreground} actions_available={available}"));
        crate::gatt_note(format!("template_selection source={source} phase=applied generation={generation} foreground_generation={foreground} actions_available={available}"));
        if let Some(event) = event {
            emit(&self.callbacks, event);
        }
    }

    /// Returns the ordinary-key profile selected by the current foreground
    /// identity. `None` means the common mappings remain active.
    pub fn active_button_mapping(&self) -> Option<ButtonMappings> {
        active_button_mapping_from(&lock(&self.state))
    }

    pub fn active_scene_mappings(&self) -> ButtonMappings {
        active_scene_mappings_from(&lock(&self.state))
    }

    pub(crate) fn application_mapping_update(
        &self,
    ) -> (
        Option<ButtonMappings>,
        ButtonMappings,
        u64,
        u64,
        MappingNotice,
    ) {
        let state = lock(&self.state);
        (
            active_button_mapping_from(&state),
            active_scene_mappings_from(&state),
            state.generation,
            state.foreground_generation,
            mapping_notice_from(&state),
        )
    }
    pub fn subscribe(&self, callback: SceneEventCallback) {
        self.callbacks
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .push(callback);
    }

    pub fn refresh_foreground(&self) {
        #[cfg(windows)]
        if own_process_is_foreground() {
            self.cancel_task_switch("own_window_foreground");
            return;
        }
        self.try_enqueue(Work::RefreshForeground, "foreground_queue_full");
    }

    /// Restore only after the explicitly focused template menu releases its keys.
    pub fn restore_target_foreground(&self) -> Result<(), ApplicationControlError> {
        let started = Instant::now();
        let (token, generation) = {
            let state = lock(&self.state);
            (state.menu_return_target.clone(), state.generation)
        };
        let Some(token) = token else {
            crate::gatt_note(format!("template_menu phase=restore_completed generation={generation} terminal_result=failed reason=return_target_unavailable elapsed_ms=0"));
            return Err(ApplicationControlError::NoForegroundWindow);
        };
        // The host calls this from its UI thread. Returning to our own main
        // window must stay on that thread; waiting on a worker would prevent
        // the target input queue from processing its foreground activation.
        let result = if token.process_id() == std::process::id() {
            self.application.restore_foreground(&token)
        } else {
            let (reply, receiver) = mpsc::sync_channel(1);
            if self
                .sender
                .try_send(Work::RestoreForeground {
                    token,
                    generation,
                    expires_at: started + Duration::from_secs(1),
                    reply,
                })
                .is_err()
            {
                crate::gatt_note(format!("template_menu phase=restore_completed generation={generation} terminal_result=failed reason=worker_unavailable elapsed_ms={}", started.elapsed().as_millis()));
                return Err(ApplicationControlError::WindowOperationFailed);
            }
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(result) => result,
                Err(error) => {
                    crate::gatt_note(format!("template_menu phase=restore_completed generation={generation} terminal_result=failed reason={} elapsed_ms={}", if matches!(error, mpsc::RecvTimeoutError::Timeout) { "worker_timeout" } else { "worker_disconnected" }, started.elapsed().as_millis()));
                    return Err(ApplicationControlError::WindowOperationFailed);
                }
            }
        };
        crate::gatt_note(format!("template_menu phase=restore_completed generation={generation} terminal_result={} reason={} elapsed_ms={}", if result.is_ok() { "passed" } else { "failed" }, result.as_ref().err().map(application_error_reason).unwrap_or("foreground_verified"), started.elapsed().as_millis()));
        if result.is_ok() {
            let mut state = lock(&self.state);
            if state.generation == generation {
                state.menu_return_target = None;
                state.closing_panel = None;
            }
            drop(state);
            self.refresh_foreground();
        }
        result
    }

    pub fn notify_voice_active(&self, active: bool) {
        let changed = {
            let mut state = lock(&self.state);
            if state.voice_active == active {
                false
            } else {
                state.voice_active = active;
                if active {
                    cancel_state(&mut state, "voice_active");
                }
                true
            }
        };
        if changed {
            crate::ble::gatt_note(format!("scene_voice active={active} panel_closed={active}"));
            self.emit_snapshot();
            if !active {
                self.refresh_foreground();
            }
        }
    }

    pub fn handle_edge(&self, edge: ButtonEdge) {
        let mut emit = false;
        {
            let mut state = lock(&self.state);
            if edge.is_pressed {
                if state.held.insert(edge.button) {
                    // Keep canceled-cycle gestures drained through their UP; only a
                    // fresh physical DOWN starts another independent action.
                    state.task_draining.remove(&edge.button);
                    state.handled_in_cycle.remove(&edge.button);
                    if edge.button == RemoteButton::Menu {
                        // The press that opens the panel has no panel generation.
                        // Only a new press inside the verified interactive menu counts.
                        state.menu_press_generation =
                            default_intent_available(&state).then_some(state.generation);
                    }
                }
            } else {
                state.held.remove(&edge.button);
                if edge.button == RemoteButton::Menu {
                    if state.menu_press_generation.take().is_some() {
                        crate::gatt_note(format!(
                            "template_default_intent phase=release generation={} long_consumed={}",
                            state.generation,
                            state.handled_in_cycle.contains(&RemoteButton::Menu)
                        ));
                    }
                }
                state.menu_native_routed.remove(&edge.button);
                emit |= finish_template_menu(&mut state);
                if state.held.is_empty() && state.waiting_for_release {
                    state.waiting_for_release = false;
                    state.status = None;
                    emit = true;
                }
            }
        }
        if emit {
            self.emit_snapshot();
        }
    }

    /// Called only by the host after checking this menu's actual foreground HWND.
    pub fn set_template_menu_focus(&self, focused: bool) {
        {
            let mut state = lock(&self.state);
            if !focused {
                state.menu_return_target = None;
                state.closing_panel = None;
            }
            if !is_template_menu(&state) {
                return;
            }
            state.template_menu_focused = focused;
            if !focused {
                state.menu_press_generation = None;
                state.menu_native_held.clear();
                state.menu_native_routed.clear();
                state.menu_close_after_release = None;
                state.panel = None;
                state.status = Some("template_menu_focus_lost".to_owned());
                state.waiting_for_release = !state.held.is_empty();
            }
        }
        crate::ble::gatt_note(format!("template_menu phase=focus verified={focused}"));
        self.emit_snapshot();
    }

    /// Native keys belong to our focused menu, not to an inferred remote device.
    pub fn template_menu_key(&self, generation: u64, button: RemoteButton, down: bool) -> bool {
        {
            let mut state = lock(&self.state);
            let draining_up = !down && state.menu_native_held.contains(&button);
            if !is_template_menu(&state)
                || !state.template_menu_focused
                || (!draining_up && (state.generation != generation || state.voice_active))
            {
                return false;
            }
            if down {
                state.menu_native_held.insert(button);
                if state.menu_close_after_release.is_none() {
                    match button {
                        RemoteButton::Up
                        | RemoteButton::Left
                        | RemoteButton::Down
                        | RemoteButton::Right => {
                            let panel = state.panel.clone().unwrap();
                            route_panel(
                                &mut state,
                                panel,
                                FiredGesture {
                                    button,
                                    trigger: ButtonTrigger::Single,
                                },
                            );
                        }
                        _ => {}
                    }
                }
            } else if state.menu_native_held.remove(&button) {
                if state.menu_close_after_release.is_none() {
                    match button {
                        RemoteButton::Ok if state.preference_request.is_none() => {
                            state.menu_close_after_release = Some(true)
                        }
                        RemoteButton::Back => state.menu_close_after_release = Some(false),
                        _ => {}
                    }
                }
                finish_template_menu(&mut state);
            }
        }
        crate::ble::gatt_note(format!(
            "template_menu phase=native_key button={button:?} down={down}"
        ));
        self.emit_snapshot();
        true
    }

    /// Keep the focused menu alive through a held native key's real release.
    pub fn prepare_template_menu_exit(&self) -> bool {
        self.cancel_task_switch("normal_exit");
        let (ready, changed) = {
            let mut state = lock(&self.state);
            let selection_pending = state.pending_selection.is_some();
            if selection_pending {
                cancel_state(&mut state, "normal_exit");
            }
            let preference_pending = state.preference_request.is_some();
            if !is_template_menu(&state) {
                (!preference_pending, selection_pending)
            } else {
                state.menu_close_after_release = Some(false);
                state.menu_press_generation = None;
                finish_template_menu(&mut state);
                (!is_template_menu(&state) && !preference_pending, true)
            }
        };
        if changed {
            self.emit_snapshot();
        }
        ready
    }

    pub fn template_menu_restore_failed(&self) {
        {
            let mut state = lock(&self.state);
            let Some(panel) = state.closing_panel.take() else {
                return;
            };
            // This is still the same visible menu, not a new open operation.
            state.panel = Some(panel);
            state.template_menu_focused = true;
            state.status = Some("template_menu_restore_failed".to_owned());
            crate::gatt_note(format!("template_menu phase=restore_failed_menu_retained generation={} selection_preserved=true", state.generation));
        }
        self.emit_snapshot();
    }

    pub fn set_update_default(&self, generation: u64, enabled: bool) -> bool {
        let changed = {
            let mut state = lock(&self.state);
            if state.generation != generation || !default_intent_available(&state) {
                return false;
            }
            set_default_intent(&mut state, enabled, "control");
            true
        };
        self.emit_snapshot();
        changed
    }

    pub fn complete_default_save(&self, request_id: u64, saved: bool) {
        {
            let mut state = lock(&self.state);
            let Some((id, application, _)) = state.default_request.as_ref() else {
                return;
            };
            if *id != request_id {
                return;
            }
            let applies_here = state
                .token
                .as_ref()
                .is_some_and(|t| t.application_id().eq_ignore_ascii_case(application));
            state.default_request = None;
            if applies_here {
                state.default_save_status = Some(if saved { "saved" } else { "failed" }.into());
            }
        }
        crate::gatt_note(format!(
            "template_default phase=completed request_id={request_id} saved={saved}"
        ));
        self.emit_snapshot();
    }

    pub fn complete_menu_preference_save(&self, request_id: u64, saved: bool) {
        {
            let mut state = lock(&self.state);
            let Some((id, generation, enabled)) = state.preference_request else {
                return;
            };
            if id != request_id {
                return;
            }
            state.preference_request = None;
            if saved {
                state.configuration.menu_update_default = enabled;
            }
            // A closed/replaced menu is never reopened or rewritten by an old response.
            if state.generation == generation {
                state.update_default = state.configuration.menu_update_default;
            }
            state.preference_error = !saved;
        }
        crate::gatt_note(format!(
            "template_default_intent phase=persisted request_id={request_id} saved={saved}"
        ));
        self.emit_snapshot();
    }

    fn handle_task_gesture(&self, routed: RoutedGesture) -> Option<GestureDisposition> {
        let mut state = lock(&self.state);
        let button = routed.gesture.button;
        // Foreground events and selected-device RawInput run on separate queues.
        // After closing a task session, never append a new template action to
        // the next already-native navigation press. This does not swallow any
        // native key, infer its source, or delay a newly captured press.
        if state.task_native_exit.remove(&button) && routed.native_delivered {
            crate::gatt_note(format!(
                "task_switch phase=native_exit_reused button={button:?} injection=none"
            ));
            return Some(GestureDisposition::Blocked);
        }

        if state.task_draining.contains(&button) {
            return Some(GestureDisposition::Blocked);
        }
        if state.panel.is_some() {
            return None;
        }
        let mapping = active_button_mapping_from(&state)
            .unwrap_or_else(|| state.configuration.common_mappings.clone());
        let action = if mapping.enabled {
            mapping.action_for(button, routed.gesture.trigger)
        } else {
            ButtonAction::Disabled
        };
        if state.task_switch.is_none() && !matches!(action, ButtonAction::TaskSwitch { .. }) {
            return None;
        }
        if state.voice_active || state.waiting_for_release {
            return Some(GestureDisposition::Blocked);
        }
        // A configured keyboard shortcut is an ordinary mapping, not a task-
        // navigation command. Let the existing mapper execute it once, including
        // while Shell is staging. The published profile retains the entering
        // template's TV gestures; external changes and drained UPs are handled
        // before this branch. Only special task actions/navigation need a proven
        // task-window target.
        if button == RemoteButton::Tv && matches!(action, ButtonAction::Shortcut { .. }) {
            crate::gatt_note(format!(
                "task_switch phase=ordinary_mapping button=Tv trigger={:?} route=mapper",
                routed.gesture.trigger
            ));
            return None;
        }
        let result = if let Some(session) = state.task_switch.clone() {
            let observed = self.application.identify_foreground();
            log_task_foreground("action", &session, &observed, state.generation);
            let current = match observed {
                Ok(current)
                    if current.is_task_switcher()
                        && session
                            .target
                            .as_ref()
                            .is_none_or(|t| t.same_window(&current)) =>
                {
                    current
                }
                Ok(current) if session.target.is_none() && current.is_task_staging() => {
                    crate::gatt_note(
                        "task_switch phase=waiting reason=shell_staging injection=none".to_owned(),
                    );
                    return Some(GestureDisposition::Blocked);
                }
                Ok(current) if session.target.is_none() && session.origin.same_window(&current) => {
                    crate::gatt_note(
                        "task_switch phase=waiting reason=shell_not_foreground".to_owned(),
                    );
                    return Some(GestureDisposition::Blocked);
                }
                _ => {
                    // A native Enter can finish the shell before Raw Input arrives.
                    // It is already delivered: cancel locally, never inject into the new app.
                    clear_task_switch(&mut state, "foreground_changed");
                    drop(state);
                    self.emit_snapshot();
                    self.refresh_foreground();
                    return Some(GestureDisposition::Blocked);
                }
            };
            state.task_switch.as_mut().unwrap().target = Some(current.clone());
            let (chord, closing, native_covers, next_view, action_kind) = if button
                == RemoteButton::Tv
            {
                if routed.native_delivered {
                    crate::gatt_note(
                        "task_switch phase=rejected reason=unowned_tv_action".to_owned(),
                    );
                    return Some(GestureDisposition::Blocked);
                }
                match action {
                    ButtonAction::TaskSwitch { view } => {
                        let keys = match view {
                            TaskSwitchView::Applications => {
                                vec![KeyCode::Control, KeyCode::Alt, KeyCode::Tab]
                            }
                            TaskSwitchView::Desktops => vec![KeyCode::LeftWindows, KeyCode::Tab],
                        };
                        (
                            KeyChord { keys },
                            false,
                            false,
                            Some(view),
                            "configured_task_switch",
                        )
                    }
                    ButtonAction::Disabled => {
                        crate::gatt_note(
                            "task_switch phase=skipped button=Tv reason=action_disabled".to_owned(),
                        );
                        return Some(GestureDisposition::Blocked);
                    }
                    _ => {
                        crate::gatt_note(
                            "task_switch phase=rejected button=Tv reason=non_keyboard_action"
                                .to_owned(),
                        );
                        return Some(GestureDisposition::Blocked);
                    }
                }
            } else {
                if routed.gesture.trigger != ButtonTrigger::Single {
                    return Some(GestureDisposition::Blocked);
                }
                let key = match button {
                    RemoteButton::Up => KeyCode::Up,
                    RemoteButton::Down => KeyCode::Down,
                    RemoteButton::Left => KeyCode::Left,
                    RemoteButton::Right => KeyCode::Right,
                    RemoteButton::Ok => KeyCode::Enter,
                    RemoteButton::Back | RemoteButton::Power => KeyCode::Escape,
                    _ => return Some(GestureDisposition::Blocked),
                };
                let closing = matches!(key, KeyCode::Enter | KeyCode::Escape);
                let native_covers = routed.native_delivered
                    && matches!(
                        button,
                        RemoteButton::Up
                            | RemoteButton::Down
                            | RemoteButton::Left
                            | RemoteButton::Right
                            | RemoteButton::Ok
                    );
                (
                    KeyChord { keys: vec![key] },
                    closing,
                    native_covers,
                    None,
                    "navigation",
                )
            };
            let result = if native_covers {
                Ok(())
            } else {
                self.application.send_task_keys(&current, &chord)
            };
            crate::gatt_note(format!("task_switch phase=navigation button={button:?} trigger={:?} action={action_kind} keys={:?} native_reused={native_covers} result={} view={:?} error={:?}", routed.gesture.trigger, chord.keys, if result.is_ok() { "ok" } else { "rejected" }, session.view, result.as_ref().err()));
            if closing || result.is_err() {
                clear_task_switch(
                    &mut state,
                    if result.is_ok() {
                        "completed"
                    } else {
                        "input_rejected"
                    },
                );
            } else if let Some(view) = next_view {
                // Any system toggle is the chosen shortcut's behavior, never a
                // physical-TV cancellation rule. Reconfirm its resulting target.
                state.task_switch = Some(TaskSwitchSession {
                    origin: session.origin,
                    target: None,
                    view,
                    started_at: Instant::now(),
                    tv_actions: session.tv_actions,
                });
                state.status = Some("task_switch_waiting".to_owned());
            }
            result
        } else {
            let ButtonAction::TaskSwitch { view } = action else {
                unreachable!()
            };
            let origin = state.token.clone()?;
            if routed.native_delivered
                || self
                    .application
                    .identify_foreground()
                    .as_ref()
                    .map_or(true, |current| !origin.same_window(current))
            {
                crate::gatt_note(
                    "task_switch phase=rejected reason=unowned_or_stale_launch".to_owned(),
                );
                return Some(GestureDisposition::Blocked);
            }
            let keys = match view {
                TaskSwitchView::Applications => vec![KeyCode::Control, KeyCode::Alt, KeyCode::Tab],
                TaskSwitchView::Desktops => vec![KeyCode::LeftWindows, KeyCode::Tab],
            };
            let result = self.application.send_task_keys(&origin, &KeyChord { keys });
            crate::gatt_note(format!(
                "task_switch phase=launch view={view:?} result={} modifiers_retained=0",
                if result.is_ok() { "ok" } else { "rejected" }
            ));
            if result.is_ok() {
                state.task_switch = Some(TaskSwitchSession {
                    origin,
                    target: None,
                    view,
                    started_at: Instant::now(),
                    tv_actions: mapping.actions(RemoteButton::Tv),
                });
                state.status = Some("task_switch_waiting".to_owned());
            } else {
                state.status = Some("task_switch_launch_failed".to_owned());
            }
            result
        };
        drop(state);
        self.emit_snapshot();
        self.refresh_foreground();
        Some(if result.is_ok() {
            GestureDisposition::Handled
        } else {
            GestureDisposition::Blocked
        })
    }

    pub(crate) fn cancel_task_switch(&self, reason: &str) {
        let changed = {
            let mut state = lock(&self.state);
            clear_task_switch(&mut state, reason)
        };
        if changed {
            self.emit_snapshot();
        }
    }

    pub fn handle_gesture(&self, routed: RoutedGesture) -> GestureDisposition {
        if let Some(disposition) = self.handle_task_gesture(routed) {
            return disposition;
        }
        let opening_generation = {
            let state = lock(&self.state);
            (routed.gesture.button == RemoteButton::Menu
                && routed.gesture.trigger == ButtonTrigger::Single
                && !routed.native_delivered
                && state.configuration.menu_template_switch_enabled
                && state.panel.is_none()
                && !state.voice_active
                && !state.waiting_for_release
                && !state.handled_in_cycle.contains(&RemoteButton::Menu))
            .then_some(state.generation)
        };
        // Public process identity can cross process boundaries. Never hold the
        // scene lock while querying it, and never query for ordinary keys.
        let opening_target = opening_generation
            .map(|generation| (generation, self.application.identify_foreground()));
        let disposition = {
            let mut state = lock(&self.state);
            let menu_key = state.configuration.menu_template_switch_enabled
                && routed.gesture.button == RemoteButton::Menu;
            if state.panel.is_none() && !menu_key {
                return GestureDisposition::PassThrough;
            }
            if state.voice_active || state.waiting_for_release {
                return GestureDisposition::Blocked;
            }
            if routed.native_delivered {
                if is_template_menu(&state) {
                    state.menu_native_routed.insert(routed.gesture.button);
                }
                GestureDisposition::Blocked
            } else if is_template_menu(&state) {
                if !state.template_menu_focused
                    || state.menu_native_routed.contains(&routed.gesture.button)
                    || state.menu_native_held.contains(&routed.gesture.button)
                {
                    GestureDisposition::Blocked
                } else {
                    let panel = state.panel.clone().unwrap();
                    route_panel(&mut state, panel, routed.gesture)
                }
            } else if menu_key
                && routed.gesture.trigger == ButtonTrigger::Single
                && state.handled_in_cycle.insert(RemoteButton::Menu)
            {
                let Some((generation, target)) = opening_target else {
                    return GestureDisposition::Blocked;
                };
                if generation != state.generation {
                    crate::gatt_note(format!("template_menu phase=open_ignored captured_generation={generation} current_generation={} reason=state_changed", state.generation));
                    return GestureDisposition::Blocked;
                }
                match target {
                    Ok(target) => {
                        let own = target.process_id() == std::process::id();
                        open_panel(&mut state, ScenePanel::Template);
                        state.menu_return_target = Some(target);
                        crate::gatt_note(format!("template_menu phase=return_target_captured generation={} role={} terminal_result=passed", state.generation, if own { "own_window" } else { "external_window" }));
                        GestureDisposition::Handled
                    }
                    Err(error) => {
                        state.status = Some("template_menu_return_target_unavailable".to_owned());
                        crate::gatt_note(format!("template_menu phase=return_target_captured generation={} terminal_result=failed reason={}", state.generation, application_error_reason(&error)));
                        GestureDisposition::Blocked
                    }
                }
            } else {
                GestureDisposition::Blocked
            }
        };
        self.emit_snapshot();
        disposition
    }

    fn try_enqueue(&self, work: Work, reason: &str) -> bool {
        match self.sender.try_send(work) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                let mut state = lock(&self.state);
                state.status = Some(reason.to_owned());
                crate::ble::gatt_note(format!("scene_enqueue result=blocked reason={reason}"));
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                lock(&self.state).status = Some("program_monitor_stopped".to_owned());
                false
            }
        }
    }

    fn emit_snapshot(&self) {
        emit_snapshot_for(&self.state, &self.callbacks);
    }
}

impl Drop for SceneController {
    fn drop(&mut self) {
        #[cfg(windows)]
        lock(&self.foreground_watcher).take();
        let _ = self.sender.send(Work::Shutdown);
        if let Some(worker) = lock(&self.worker).take() {
            let _ = worker.join();
        }
    }
}

fn route_panel(
    state: &mut State,
    mut panel: PanelState,
    gesture: FiredGesture,
) -> GestureDisposition {
    if gesture.button == RemoteButton::Menu && gesture.trigger == ButtonTrigger::Long {
        let valid_press = state.menu_press_generation == Some(state.generation)
            && state.held.contains(&RemoteButton::Menu);
        if !valid_press
            || !default_intent_available(state)
            || !state.handled_in_cycle.insert(RemoteButton::Menu)
        {
            crate::gatt_note(format!(
                "template_default_intent phase=rejected generation={} valid_press={valid_press}",
                state.generation
            ));
            return GestureDisposition::Blocked;
        }
        set_default_intent(state, !state.update_default, "remote_long");
        return GestureDisposition::Handled;
    }
    if gesture.trigger != ButtonTrigger::Single || state.menu_close_after_release.is_some() {
        return GestureDisposition::Blocked;
    }
    match gesture.button {
        RemoteButton::Ok | RemoteButton::Back | RemoteButton::Power | RemoteButton::Menu => {
            if gesture.button == RemoteButton::Ok && state.preference_request.is_some() {
                return GestureDisposition::Blocked;
            }
            if !state.handled_in_cycle.insert(gesture.button) {
                return GestureDisposition::Blocked;
            }
            state.menu_close_after_release = Some(gesture.button == RemoteButton::Ok);
            finish_template_menu(state);
            return GestureDisposition::Handled;
        }
        _ => {}
    }
    let count = menu_items(state, ScenePanel::Template).len();
    if count == 0 {
        return GestureDisposition::Blocked;
    }
    match gesture.button {
        RemoteButton::Up | RemoteButton::Left => {
            panel.selected = if panel.selected == 0 {
                count - 1
            } else {
                panel.selected - 1
            }
        }
        RemoteButton::Down | RemoteButton::Right => panel.selected = (panel.selected + 1) % count,
        _ => return GestureDisposition::Blocked,
    }
    state.panel = Some(panel);
    GestureDisposition::Handled
}

fn open_panel(state: &mut State, kind: ScenePanel) {
    let count = menu_items(state, kind).len();
    if count == 0 {
        state.status = Some("menu_empty".to_owned());
        return;
    }
    if kind == ScenePanel::Template {
        state.menu_return_target = None;
        state.closing_panel = None;
        state.generation = state.generation.saturating_add(1);
        state.menu_press_generation = None;
        state.template_menu_focused = false;
        state.menu_native_held.clear();
        state.menu_native_routed.clear();
        state.menu_close_after_release = None;
    }
    state.menu_application_id = state.token.as_ref().map(|t| t.application_id().to_owned());
    state.update_default = state.configuration.menu_update_default;
    state.panel = Some(PanelState { kind, selected: 0 });
    state.status = None;
}

fn program_worker(
    receiver: Receiver<Work>,
    state: Arc<Mutex<State>>,
    callbacks: Arc<RwLock<Vec<SceneEventCallback>>>,
    application: Arc<dyn ApplicationControlBackend>,
) {
    loop {
        let wait = {
            let current = lock(&state);
            current
                .task_switch
                .as_ref()
                .filter(|s| s.target.is_none())
                .map(|s| TASK_TARGET_TIMEOUT.saturating_sub(s.started_at.elapsed()))
        };
        let work = match wait {
            Some(wait) => match receiver.recv_timeout(wait) {
                Ok(work) => work,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    refresh_foreground_state(&state, &callbacks, application.as_ref());
                    expire_unconfirmed_task(&state, &callbacks, Instant::now());
                    continue;
                }
            },
            None => match receiver.recv() {
                Ok(work) => work,
                Err(_) => break,
            },
        };
        match work {
            Work::Shutdown => break,
            Work::RefreshForeground => {
                refresh_foreground_state(&state, &callbacks, application.as_ref())
            }
            Work::RestoreForeground {
                token,
                generation,
                expires_at,
                reply,
            } => {
                let current = {
                    let state = lock(&state);
                    state.generation == generation
                        && state
                            .menu_return_target
                            .as_ref()
                            .is_some_and(|target| target.same_window(&token))
                };
                let result = if !current || Instant::now() >= expires_at {
                    crate::gatt_note(format!(
                        "template_menu phase=restore_ignored generation={generation} reason={}",
                        if current {
                            "expired_request"
                        } else {
                            "cancelled_request"
                        }
                    ));
                    Err(ApplicationControlError::StaleToken)
                } else {
                    #[cfg(all(windows, not(test)))]
                    let own_foreground = own_process_is_foreground();
                    #[cfg(any(not(windows), test))]
                    let own_foreground = true;
                    if own_foreground {
                        application.restore_foreground(&token)
                    } else {
                        crate::gatt_note(format!("template_menu phase=restore_ignored generation={generation} reason=external_foreground"));
                        Err(ApplicationControlError::WindowOperationFailed)
                    }
                };
                let _ = reply.try_send(result);
            }
        }
        expire_unconfirmed_task(&state, &callbacks, Instant::now());
    }
}

fn refresh_foreground_state(
    state: &Arc<Mutex<State>>,
    callbacks: &Arc<RwLock<Vec<SceneEventCallback>>>,
    application: &dyn ApplicationControlBackend,
) {
    #[cfg(windows)]
    if own_process_is_foreground() {
        let changed = clear_task_switch(&mut lock(state), "own_window_foreground");
        if changed {
            emit_snapshot_for(state, callbacks);
        }
        return;
    }
    let generation = lock(state).generation;
    let observed = application.identify_foreground();
    apply_foreground_observation(state, callbacks, generation, observed);
}

fn apply_foreground_observation(
    state: &Arc<Mutex<State>>,
    callbacks: &Arc<RwLock<Vec<SceneEventCallback>>>,
    generation: u64,
    observed: Result<WindowToken, ApplicationControlError>,
) {
    let mut changed = false;
    {
        let mut state = lock(state);
        if state.generation != generation {
            if state.task_switch.is_some() {
                crate::gatt_note(format!("task_switch phase=observation_ignored captured_generation={generation} current_generation={}", state.generation));
            }
            return;
        }
        if let Some(session) = state.task_switch.as_ref() {
            log_task_foreground("observer", session, &observed, generation);
        }
        match observed {
            Ok(token) => {
                if let Some(session) = state.task_switch.as_ref() {
                    let newest = session
                        .target
                        .as_ref()
                        .unwrap_or(&session.origin)
                        .generation();
                    if token.generation() < newest {
                        crate::gatt_note(
                            "task_switch phase=observation_ignored reason=older_window_generation"
                                .to_owned(),
                        );
                        return;
                    }
                    if session.target.is_none() && token.is_task_staging() {
                        state.status = Some("task_switch_waiting".to_owned());
                        crate::gatt_note("task_switch phase=foreground_transition identity=shell_staging policy=await_exact_target injection=none".to_owned());
                        return;
                    }
                    if token.is_task_switcher()
                        && session
                            .target
                            .as_ref()
                            .is_none_or(|target| target.same_window(&token))
                    {
                        state.task_switch.as_mut().unwrap().target = Some(token);
                        state.status = Some("task_switch_active".to_owned());
                        crate::gatt_note(
                            "task_switch phase=foreground_verified identity=public_shell_window"
                                .to_owned(),
                        );
                        return;
                    }
                    if session.target.is_none() && session.origin.same_window(&token) {
                        return;
                    }
                    changed |= clear_task_switch(&mut state, "foreground_changed");
                }
                if token.process_id() == std::process::id() {
                    return;
                }
                let program_changed = state.token.as_ref().is_none_or(|current| {
                    !current
                        .application_id()
                        .eq_ignore_ascii_case(token.application_id())
                });
                let window_changed = state
                    .token
                    .as_ref()
                    .is_none_or(|current| current.generation() != token.generation());
                if program_changed {
                    cancel_state(&mut state, "program_changed");
                    if state.configuration.button_mapping_follow_enabled {
                        state.manual_selection = None;
                    }
                    state.default_save_status = None;
                    changed = true;
                } else if window_changed && state.panel.is_some() {
                    // External focus loss cancels our menu; no foreground restoration.
                    state.panel = None;
                    state.template_menu_focused = false;
                    cancel_state(&mut state, "menu_focus_lost");
                    changed = true;
                } else if window_changed && state.pending_selection.is_some() {
                    cancel_state(&mut state, "foreground_changed");
                    changed = true;
                }
                state.foreground_generation = token.generation();
                state.token = Some(token);
                state.menu_return_target = None;
                state.closing_panel = None;
            }
            Err(_) => {
                changed |= clear_task_switch(&mut state, "foreground_unavailable");
                if state.token.take().is_some() {
                    cancel_state(&mut state, "foreground_unavailable");
                    if state.configuration.button_mapping_follow_enabled {
                        state.manual_selection = None;
                    }
                    changed = true;
                }
                state.menu_return_target = None;
                state.closing_panel = None;
            }
        }
    }
    if changed {
        crate::gatt_note(
            "template_program phase=changed identity=public_process ui_query=false".to_owned(),
        );
        emit_snapshot_for(state, callbacks);
    }
}

fn expire_unconfirmed_task(
    state: &Arc<Mutex<State>>,
    callbacks: &Arc<RwLock<Vec<SceneEventCallback>>>,
    now: Instant,
) {
    let changed = {
        let mut state = lock(state);
        if state.task_switch.as_ref().is_some_and(|s| {
            s.target.is_none() && now.saturating_duration_since(s.started_at) >= TASK_TARGET_TIMEOUT
        }) {
            crate::gatt_note(
                "task_switch phase=failed reason=target_unconfirmed_timeout injection=none"
                    .to_owned(),
            );
            clear_task_switch(&mut state, "target_unconfirmed_timeout")
        } else {
            false
        }
    };
    if changed {
        emit_snapshot_for(state, callbacks);
    }
}

fn log_task_foreground(
    source: &str,
    session: &TaskSwitchSession,
    observed: &Result<WindowToken, ApplicationControlError>,
    generation: u64,
) {
    match observed {
        Ok(current) => crate::gatt_note(format!("task_switch phase=foreground_check source={source} generation={generation} origin_generation={} observed_generation={} origin_same={} target_known={} target_same={} shell_owned={} public_class={} accepted_class={}", session.origin.generation(), current.generation(), session.origin.same_window(current), session.target.is_some(), session.target.as_ref().is_some_and(|t| t.same_window(current)), current.shell_owned(), current.public_task_class(), current.is_task_switcher())),
        Err(error) => crate::gatt_note(format!("task_switch phase=foreground_check source={source} generation={generation} error={error:?}")),
    }
}

fn clear_task_switch(state: &mut State, reason: &str) -> bool {
    if state.task_switch.take().is_none() {
        return false;
    }
    state.task_draining.extend(state.held.iter().copied());
    state.task_native_exit.extend([
        RemoteButton::Up,
        RemoteButton::Down,
        RemoteButton::Left,
        RemoteButton::Right,
        RemoteButton::Ok,
    ]);
    state.status = Some(format!("task_switch_{reason}"));
    crate::gatt_note(format!(
        "task_switch phase=closed reason={reason} modifiers_retained=0"
    ));
    true
}

fn cancel_state(state: &mut State, reason: &str) {
    if let Some(pending) = state.pending_selection.take() {
        state.manual_selection = pending.previous;
        let _ = pending.reply.try_send(Err("state_changed"));
    }
    clear_task_switch(state, reason);
    // A configuration/voice cancellation may not send a held native Enter back
    // to the target window. Drain the focused menu first; focus loss is separate.
    let drain_menu = is_template_menu(state)
        && state.template_menu_focused
        && (!state.held.is_empty() || !state.menu_native_held.is_empty());
    state.generation = state.generation.saturating_add(1).max(1);
    state.menu_press_generation = None;
    if drain_menu {
        state.menu_close_after_release = Some(false);
    } else {
        state.closing_panel = state.panel.take();
        if state.closing_panel.is_none() {
            state.menu_return_target = None;
        }
        state.template_menu_focused = false;
        state.menu_native_held.clear();
        state.menu_native_routed.clear();
        state.menu_close_after_release = None;
    }
    state.handled_in_cycle.clear();
    if !state.held.is_empty() {
        state.waiting_for_release = true;
    }
    state.status = Some(reason.to_owned());
}

fn is_template_menu(state: &State) -> bool {
    state
        .panel
        .as_ref()
        .is_some_and(|p| p.kind == ScenePanel::Template)
}

fn default_intent_available(state: &State) -> bool {
    is_template_menu(state)
        && state.configuration.menu_template_switch_enabled
        && state.template_menu_focused
        && !state.voice_active
        && !state.waiting_for_release
        && state.menu_close_after_release.is_none()
        && state.preference_request.is_none()
        && state.token.as_ref().is_some_and(|token| {
            state
                .menu_application_id
                .as_ref()
                .is_some_and(|application| token.application_id().eq_ignore_ascii_case(application))
        })
}

fn set_default_intent(state: &mut State, enabled: bool, source: &str) {
    state.update_default = enabled;
    state.preference_error = false;
    state.default_request_sequence = state.default_request_sequence.saturating_add(1);
    let request_id = state.default_request_sequence;
    state.preference_request = Some((request_id, state.generation, enabled));
    state.default_event = Some(SceneEvent::MenuPreferencePersistenceRequested {
        request_id,
        enabled,
    });
    crate::gatt_note(format!(
        "template_default_intent phase=changed source={source} generation={} enabled={enabled} persisted=false",
        state.generation
    ));
}

fn finish_template_menu(state: &mut State) -> bool {
    if !is_template_menu(state) || !state.held.is_empty() || !state.menu_native_held.is_empty() {
        return false;
    }
    let Some(confirm) = state.menu_close_after_release.take() else {
        return false;
    };
    let selected = state.panel.as_ref().unwrap().selected;
    if confirm && state.template_menu_focused && state.configuration.menu_template_switch_enabled {
        if let Some(item) = menu_items(state, ScenePanel::Template).get(selected) {
            let selected = item.template_id.clone();
            // A notice queued while navigating describes the previous profile.
            // Confirmation starts a new application generation before publication.
            state.generation = state.generation.saturating_add(1);
            apply_manual_selection(
                state,
                Some(selected.clone()),
                state.menu_application_id.clone(),
                "menu",
            );
            if state.update_default {
                if let Some(application_id) = state.menu_application_id.clone() {
                    state.default_request_sequence =
                        state.default_request_sequence.saturating_add(1);
                    let request_id = state.default_request_sequence;
                    state.default_request =
                        Some((request_id, application_id.clone(), selected.clone()));
                    state.default_event = Some(SceneEvent::DefaultTemplatePersistenceRequested {
                        request_id,
                        application_id,
                        template_id: selected,
                    });
                    state.default_save_status = Some("saving".into());
                }
            }
        }
    }
    state.closing_panel = state.panel.take();
    state.template_menu_focused = false;
    state.menu_press_generation = None;
    state.handled_in_cycle.clear();
    state.menu_native_routed.clear();
    state.status = Some(
        if confirm {
            "manual_template_selected"
        } else {
            "template_menu_cancelled"
        }
        .to_owned(),
    );
    true
}

fn active_template_id(state: &State) -> Option<String> {
    selected_template(state).map(str::to_owned)
}

fn template_exists(configuration: &MappingConfiguration, id: &str) -> bool {
    configuration.template_mappings(id).is_some()
}

fn apply_manual_selection(
    state: &mut State,
    template_id: Option<String>,
    application_id: Option<String>,
    source: &'static str,
) {
    let common = template_id.is_none();
    state.manual_selection = Some(ManualTemplateSelection {
        template_id,
        application_id,
        source,
    });
    state.default_save_status = None;
    state.default_request = None;
    crate::gatt_note(format!("template_selection source={source} phase=requested common={common} generation={} scope={} persisted=false", state.generation, if state.configuration.button_mapping_follow_enabled { "application" } else { "session" }));
}

fn manual_selection_matches_application(state: &State) -> bool {
    state.manual_selection.as_ref().is_some_and(|selection| {
        match (selection.application_id.as_deref(), state.token.as_ref()) {
            (Some(application), Some(token)) => {
                application.eq_ignore_ascii_case(token.application_id())
            }
            (None, None) => true,
            _ => false,
        }
    })
}

fn effective_manual_selection(state: &State) -> Option<&ManualTemplateSelection> {
    state.manual_selection.as_ref().filter(|_| {
        !state.configuration.button_mapping_follow_enabled
            || manual_selection_matches_application(state)
    })
}

fn selection_source(state: &State) -> &'static str {
    if let Some(selection) = effective_manual_selection(state) {
        selection.source
    } else if selected_template(state).is_some() {
        "program_default"
    } else {
        "common"
    }
}

fn selected_template(state: &State) -> Option<&str> {
    if let Some(selection) = effective_manual_selection(state) {
        return selection
            .template_id
            .as_deref()
            .filter(|id| template_exists(&state.configuration, id));
    }
    if !state.configuration.button_mapping_follow_enabled {
        return None;
    }
    let application = state.token.as_ref()?.application_id();
    state
        .configuration
        .application_bindings
        .iter()
        .find(|b| b.application_id.eq_ignore_ascii_case(application))
        .map(|b| b.template_id.as_str())
}
fn menu_items(state: &State, _kind: ScenePanel) -> Vec<SceneMenuItem> {
    state
        .configuration
        .template_catalog()
        .into_iter()
        .map(|t| SceneMenuItem {
            application_id: None,
            running: active_template_id(state).as_deref() == Some(t.id.as_str()),
            template_id: t.id,
            label: t.name,
        })
        .collect()
}
fn active_button_mapping_from(state: &State) -> Option<ButtonMappings> {
    if let Some(session) = state.task_switch.as_ref() {
        let mut mappings = ButtonMappings {
            enabled: true,
            applications: Vec::new(),
            focus_profiles: Default::default(),
            actions: Default::default(),
        };
        for button in [
            RemoteButton::Up,
            RemoteButton::Down,
            RemoteButton::Left,
            RemoteButton::Right,
            RemoteButton::Ok,
            RemoteButton::Back,
            RemoteButton::Power,
        ] {
            mappings.actions.entry(button).or_default().single = recognition_marker();
        }
        mappings
            .actions
            .insert(RemoteButton::Tv, session.tv_actions.clone());
        return Some(mappings);
    }
    state
        .configuration
        .template_mappings(selected_template(state)?)
}
fn recognition_marker() -> ButtonAction {
    ButtonAction::Shortcut {
        chord: KeyChord {
            keys: vec![KeyCode::Escape],
        },
    }
}
fn active_scene_mappings_from(state: &State) -> ButtonMappings {
    let mut mappings = ButtonMappings {
        enabled: false,
        applications: Vec::new(),
        focus_profiles: Default::default(),
        actions: Default::default(),
    };
    if state.configuration.menu_template_switch_enabled {
        mappings.enabled = true;
        mappings
            .actions
            .entry(RemoteButton::Menu)
            .or_default()
            .single = recognition_marker();
        if state.panel.is_some() {
            mappings.actions.entry(RemoteButton::Menu).or_default().long = recognition_marker();
            for button in [
                RemoteButton::Up,
                RemoteButton::Down,
                RemoteButton::Left,
                RemoteButton::Right,
                RemoteButton::Ok,
                RemoteButton::Back,
                RemoteButton::Power,
            ] {
                mappings.actions.entry(button).or_default().single = recognition_marker();
            }
        }
    }
    mappings
}

fn mapping_notice_from(state: &State) -> MappingNotice {
    let active = active_template_id(state);
    let template = active.as_ref().and_then(|id| {
        state
            .configuration
            .template_catalog()
            .into_iter()
            .find(|t| t.id == *id)
    });
    let mappings = active_button_mapping_from(state)
        .unwrap_or_else(|| state.configuration.common_mappings.clone());
    MappingNotice {
        kind: if !mappings.enabled {
            "disabled"
        } else if mappings.mapped_mask() == 0 {
            "unconfigured"
        } else if template.is_some() {
            "direct"
        } else {
            "common"
        }
        .into(),
        template_id: active,
        name: template.map(|t| t.name),
        actions_available: false,
        default_save_status: state.default_save_status.clone(),
    }
}
fn snapshot_from(state: &State) -> SceneSnapshot {
    SceneSnapshot {
        mapping_notice_enabled: state.configuration.mapping_notice_enabled,
        mapping_notice: state.mapping_notice.clone(),
        mapping_notice_revision: state.mapping_notice_revision,
        enabled: state.configuration.button_mapping_follow_enabled,
        generation: state.generation,
        foreground_generation: state.foreground_generation,
        application_id: state.token.as_ref().map(|t| t.application_id().to_owned()),
        template_id: active_template_id(state),
        panel: state.panel.as_ref().map(|p| p.kind),
        update_default: state.update_default,
        preference_pending: state.preference_request.is_some(),
        preference_error: state.preference_error,
        selected_index: state.panel.as_ref().map(|p| p.selected),
        menu_items: state
            .panel
            .as_ref()
            .map_or_else(Vec::new, |p| menu_items(state, p.kind)),
        waiting_for_release: state.waiting_for_release,
        voice_active: state.voice_active,
        status: state.status.clone(),
    }
}

fn emit(callbacks: &Arc<RwLock<Vec<SceneEventCallback>>>, event: SceneEvent) {
    let callbacks = callbacks.read().unwrap_or_else(|p| p.into_inner()).clone();
    for callback in callbacks {
        callback(event.clone());
    }
}

fn emit_snapshot_for(state: &Arc<Mutex<State>>, callbacks: &Arc<RwLock<Vec<SceneEventCallback>>>) {
    // Build the owned snapshot before invoking subscribers. Keeping the guard
    // alive through `emit` deadlocks subscribers that legitimately query the
    // current application-specific button profile from this controller.
    let (snapshot, event) = {
        let mut state = lock(state);
        (snapshot_from(&state), state.default_event.take())
    };
    emit(callbacks, SceneEvent::Snapshot { snapshot });
    if let Some(event) = event {
        emit(callbacks, event);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(windows)]
static FOREGROUND_SENDER: std::sync::OnceLock<Mutex<Option<SyncSender<Work>>>> =
    std::sync::OnceLock::new();

#[cfg(windows)]
struct ForegroundWatcher {
    thread_id: u32,
    worker: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl ForegroundWatcher {
    fn start(sender: SyncSender<Work>) -> Option<Self> {
        use std::sync::mpsc::channel;
        let slot = FOREGROUND_SENDER.get_or_init(|| Mutex::new(None));
        if lock(slot).is_some() {
            return None;
        }
        *lock(slot) = Some(sender);
        let (ready_tx, ready_rx) = channel();
        let worker = std::thread::Builder::new()
            .name("sayall-foreground-hook".to_owned())
            .spawn(move || {
                use windows::Win32::Foundation::HMODULE;
                use windows::Win32::System::Threading::GetCurrentThreadId;
                use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent};
                use windows::Win32::UI::WindowsAndMessaging::{
                    DispatchMessageW, GetMessageW, TranslateMessage, EVENT_SYSTEM_FOREGROUND, MSG,
                    WINEVENT_OUTOFCONTEXT,
                };
                let thread_id = unsafe { GetCurrentThreadId() };
                let hook = unsafe {
                    SetWinEventHook(
                        EVENT_SYSTEM_FOREGROUND,
                        EVENT_SYSTEM_FOREGROUND,
                        Some(HMODULE::default()),
                        Some(foreground_callback),
                        0,
                        0,
                        WINEVENT_OUTOFCONTEXT,
                    )
                };
                let _ = ready_tx.send((thread_id, !hook.is_invalid()));
                if hook.is_invalid() {
                    return;
                }
                let mut message = MSG::default();
                while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
                    unsafe {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                unsafe {
                    let _ = UnhookWinEvent(hook);
                }
            })
            .ok()?;
        let (thread_id, ready) = ready_rx.recv_timeout(Duration::from_secs(1)).ok()?;
        if !ready {
            *lock(slot) = None;
            return None;
        }
        Some(Self {
            thread_id,
            worker: Some(worker),
        })
    }
}

#[cfg(windows)]
unsafe extern "system" fn foreground_callback(
    _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    _hwnd: windows::Win32::Foundation::HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    if _hwnd.0.is_null() {
        return;
    }
    let mut process_id = 0u32;
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
            _hwnd,
            Some(&mut process_id),
        );
    }
    if process_id == std::process::id() {
        return;
    }
    if let Some(slot) = FOREGROUND_SENDER.get() {
        if let Some(sender) = lock(slot).as_ref() {
            let _ = sender.try_send(Work::RefreshForeground);
        }
    }
}

#[cfg(windows)]
fn own_process_is_foreground() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return false;
    }
    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    }
    process_id == std::process::id()
}

#[cfg(windows)]
impl Drop for ForegroundWatcher {
    fn drop(&mut self) {
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        if let Some(slot) = FOREGROUND_SENDER.get() {
            *lock(slot) = None;
        }
        unsafe {
            let _ = PostThreadMessageW(
                self.thread_id,
                WM_QUIT,
                Default::default(),
                Default::default(),
            );
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application_control::ApplicationAdapterKind;
    use crate::button_gestures::{GestureRecognizer, LONG_PRESS_THRESHOLD};
    use crate::templates::{
        ApplicationBinding, BUILTIN_AGENT_TEMPLATE_ID, BUILTIN_CHAT_TEMPLATE_ID,
    };
    fn token(app: &str, window: u64) -> WindowToken {
        WindowToken::from_identity(
            app.into(),
            ApplicationAdapterKind::Generic,
            u32::MAX,
            window,
            window,
        )
    }
    fn state() -> Arc<Mutex<State>> {
        let mut state = State::default();
        state.configuration.menu_template_switch_enabled = true;
        state.configuration.button_mapping_follow_enabled = true;
        state
            .configuration
            .application_bindings
            .push(ApplicationBinding {
                application_id: "a".into(),
                template_id: BUILTIN_AGENT_TEMPLATE_ID.into(),
                menu_order: 0,
                launch_target: None,
            });
        state.token = Some(token("a", 1));
        state.foreground_generation = 1;
        Arc::new(Mutex::new(state))
    }
    fn observe(state: &Arc<Mutex<State>>, value: WindowToken) {
        let generation = lock(state).generation;
        apply_foreground_observation(
            state,
            &Arc::new(RwLock::new(Vec::new())),
            generation,
            Ok(value),
        );
    }
    fn choose(state: &mut State, save: bool) {
        open_panel(state, ScenePanel::Template);
        state.template_menu_focused = true;
        assert!(!state.update_default);
        state.update_default = save;
        state.panel.as_mut().unwrap().selected = 1;
        state.menu_close_after_release = Some(true);
        assert!(finish_template_menu(state));
    }
    fn controller() -> SceneController {
        let (sender, _receiver) = mpsc::sync_channel(PROGRAM_QUEUE_CAPACITY);
        SceneController {
            application: Arc::new(TaskBackend {
                current: Mutex::new(Some(token("a", 1))),
                ..Default::default()
            }),
            state: state(),
            sender,
            callbacks: Arc::new(RwLock::new(Vec::new())),
            worker: Mutex::new(None),
            #[cfg(windows)]
            foreground_watcher: Mutex::new(None),
        }
    }
    #[derive(Default)]
    struct TaskBackend {
        current: Mutex<Option<WindowToken>>,
        sent: Mutex<Vec<KeyChord>>,
        restored: Mutex<Vec<WindowToken>>,
        fail: std::sync::atomic::AtomicBool,
    }
    impl ApplicationControlBackend for TaskBackend {
        fn identify_foreground(&self) -> Result<WindowToken, ApplicationControlError> {
            lock(&self.current)
                .clone()
                .ok_or(ApplicationControlError::NoForegroundWindow)
        }
        fn restore_foreground(&self, target: &WindowToken) -> Result<(), ApplicationControlError> {
            lock(&self.restored).push(target.clone());
            if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
                Err(ApplicationControlError::WindowOperationFailed)
            } else {
                Ok(())
            }
        }
        fn send_task_keys(
            &self,
            target: &WindowToken,
            chord: &KeyChord,
        ) -> Result<(), ApplicationControlError> {
            if self.fail.load(std::sync::atomic::Ordering::SeqCst)
                || lock(&self.current)
                    .as_ref()
                    .is_none_or(|t| !t.same_window(target))
            {
                return Err(ApplicationControlError::StaleToken);
            }
            lock(&self.sent).push(chord.clone());
            Ok(())
        }
    }

    fn menu_controller(backend: Arc<TaskBackend>) -> SceneController {
        let mut controller = controller();
        let (sender, receiver) = mpsc::sync_channel(PROGRAM_QUEUE_CAPACITY);
        controller.sender = sender;
        controller.application = backend;
        let worker_state = Arc::clone(&controller.state);
        let callbacks = Arc::clone(&controller.callbacks);
        let application = Arc::clone(&controller.application);
        *lock(&controller.worker) = Some(std::thread::spawn(move || {
            program_worker(receiver, worker_state, callbacks, application);
        }));
        controller
    }

    #[test]
    fn menu_returns_to_own_opening_window_without_changing_template_program() {
        let backend = Arc::new(TaskBackend::default());
        let main = WindowToken::from_identity(
            "sayall".into(),
            ApplicationAdapterKind::Generic,
            std::process::id(),
            90,
            90,
        );
        *lock(&backend.current) = Some(main.clone());
        let controller = menu_controller(Arc::clone(&backend));
        let original = controller.snapshot();
        assert_eq!(
            menu_gesture(&controller, ButtonTrigger::Single),
            GestureDisposition::Handled
        );
        controller.set_template_menu_focus(true);
        assert_eq!(
            controller.snapshot().application_id,
            original.application_id
        );
        assert_eq!(controller.snapshot().template_id, original.template_id);
        assert_eq!(
            menu_gesture(&controller, ButtonTrigger::Single),
            GestureDisposition::Blocked
        );
        controller.handle_edge(ButtonEdge {
            button: RemoteButton::Menu,
            is_pressed: true,
        });
        controller.handle_edge(ButtonEdge {
            button: RemoteButton::Menu,
            is_pressed: false,
        });
        assert_eq!(
            menu_gesture(&controller, ButtonTrigger::Single),
            GestureDisposition::Handled
        );
        assert!(controller.snapshot().panel.is_none());
        assert!(controller.restore_target_foreground().is_ok());
        assert_eq!(*lock(&backend.restored), vec![main]);
        assert_eq!(
            controller.snapshot().application_id,
            original.application_id
        );
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "requires an interactive Windows desktop; temporarily shows only this test's windows"]
    fn own_menu_return_uses_real_windows_foreground_on_the_calling_ui_thread() {
        use windows::core::w;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetForegroundWindow, SetForegroundWindow, ShowWindow,
            SW_SHOWNORMAL, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
        };
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                let _ = unsafe { DestroyWindow(self.0) };
            }
        }
        let create = || {
            TestWindow(unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!("SayAll menu return test"),
                    WS_OVERLAPPEDWINDOW,
                    60,
                    60,
                    320,
                    180,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap()
            })
        };
        let main = create();
        let menu = create();
        unsafe {
            let _ = ShowWindow(main.0, SW_SHOWNORMAL);
            let requested = SetForegroundWindow(main.0).as_bool();
            assert_eq!(
                GetForegroundWindow(),
                main.0,
                "interactive foreground permission required; requested={requested}"
            );
        }
        let mut controller = controller();
        controller.application = Arc::new(ApplicationController::new());
        assert_eq!(
            menu_gesture(&controller, ButtonTrigger::Single),
            GestureDisposition::Handled
        );
        unsafe {
            let _ = ShowWindow(menu.0, SW_SHOWNORMAL);
            let requested = SetForegroundWindow(menu.0).as_bool();
            assert_eq!(
                GetForegroundWindow(),
                menu.0,
                "menu foreground setup failed; requested={requested}"
            );
        }
        controller.set_template_menu_focus(true);
        {
            let mut state = lock(&controller.state);
            state.menu_close_after_release = Some(false);
            assert!(finish_template_menu(&mut state));
        }
        let started = Instant::now();
        controller.restore_target_foreground().unwrap();
        assert_eq!(unsafe { GetForegroundWindow() }, main.0);
        println!(
            "menu_focus_probe main_to_menu_to_main=passed restore_ms={} same_ui_thread=true",
            started.elapsed().as_millis()
        );
    }

    #[test]
    fn menu_identifies_outside_scene_lock_and_rejects_changed_open_generation() {
        struct InspectBackend {
            state: Arc<Mutex<State>>,
            invalidate: bool,
            calls: std::sync::atomic::AtomicUsize,
        }
        impl ApplicationControlBackend for InspectBackend {
            fn identify_foreground(&self) -> Result<WindowToken, ApplicationControlError> {
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut state = self
                    .state
                    .try_lock()
                    .expect("foreground API must not hold scene lock");
                if self.invalidate {
                    state.generation += 1;
                }
                Ok(token("actual", 90))
            }
            fn restore_foreground(&self, _: &WindowToken) -> Result<(), ApplicationControlError> {
                Ok(())
            }
        }
        for invalidate in [false, true] {
            let mut controller = controller();
            let backend = Arc::new(InspectBackend {
                state: Arc::clone(&controller.state),
                invalidate,
                calls: std::sync::atomic::AtomicUsize::new(0),
            });
            controller.application = backend.clone();
            assert_eq!(
                task_route(
                    &controller,
                    RemoteButton::Down,
                    ButtonTrigger::Single,
                    false
                ),
                GestureDisposition::PassThrough
            );
            assert_eq!(backend.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
            assert_eq!(
                menu_gesture(&controller, ButtonTrigger::Single),
                if invalidate {
                    GestureDisposition::Blocked
                } else {
                    GestureDisposition::Handled
                }
            );
            assert_eq!(backend.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
            assert_eq!(controller.snapshot().panel.is_none(), invalidate);
        }
    }

    #[test]
    fn denied_menu_restore_retains_selection_and_generation_without_reopening() {
        let backend = Arc::new(TaskBackend::default());
        let main = WindowToken::from_identity(
            "sayall".into(),
            ApplicationAdapterKind::Generic,
            std::process::id(),
            90,
            90,
        );
        *lock(&backend.current) = Some(main.clone());
        let controller = menu_controller(Arc::clone(&backend));
        assert_eq!(
            menu_gesture(&controller, ButtonTrigger::Single),
            GestureDisposition::Handled
        );
        {
            let mut state = lock(&controller.state);
            state.template_menu_focused = true;
            state.panel.as_mut().unwrap().selected = 1;
        }
        let before = controller.snapshot();
        {
            let mut state = lock(&controller.state);
            state.menu_close_after_release = Some(false);
            assert!(finish_template_menu(&mut state));
        }
        backend
            .fail
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(controller.restore_target_foreground().is_err());
        controller.template_menu_restore_failed();
        let retained = controller.snapshot();
        assert_eq!(retained.generation, before.generation);
        assert_eq!(retained.selected_index, before.selected_index);
        assert_eq!(retained.panel, before.panel);
        controller.template_menu_restore_failed();
        assert_eq!(controller.snapshot().generation, before.generation);
        {
            let mut state = lock(&controller.state);
            state.menu_close_after_release = Some(false);
            assert!(finish_template_menu(&mut state));
        }
        backend
            .fail
            .store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(controller.restore_target_foreground().is_ok());
        assert_eq!(*lock(&backend.restored), vec![main.clone(), main]);
        assert!(lock(&controller.state).menu_return_target.is_none());
        assert!(lock(&controller.state).closing_panel.is_none());
    }

    #[test]
    fn menu_hide_and_external_focus_discard_return_target_without_resurrection() {
        for cause in ["hide", "external", "unavailable", "voice"] {
            let controller = controller();
            assert_eq!(
                menu_gesture(&controller, ButtonTrigger::Single),
                GestureDisposition::Handled
            );
            controller.set_template_menu_focus(true);
            match cause {
                "external" => observe(&controller.state, token("other", 80)),
                "unavailable" => {
                    let generation = controller.snapshot().generation;
                    apply_foreground_observation(
                        &controller.state,
                        &controller.callbacks,
                        generation,
                        Err(ApplicationControlError::IdentityUnavailable),
                    );
                }
                "voice" => {
                    controller.notify_voice_active(true);
                    // The host acknowledges the completed hide even if focus was already lost.
                    controller.set_template_menu_focus(false);
                    controller.notify_voice_active(false);
                }
                _ => controller.set_template_menu_focus(false),
            }
            assert!(
                lock(&controller.state).menu_return_target.is_none(),
                "{cause}"
            );
            assert!(lock(&controller.state).closing_panel.is_none(), "{cause}");
            controller.template_menu_restore_failed();
            assert!(controller.snapshot().panel.is_none(), "{cause}");
            assert!(matches!(
                controller.restore_target_foreground(),
                Err(ApplicationControlError::NoForegroundWindow)
            ));
        }
    }

    #[test]
    fn each_new_menu_captures_its_own_return_window_and_missing_target_blocks_open() {
        let backend = Arc::new(TaskBackend::default());
        let mut controller = controller();
        controller.application = backend.clone();
        assert_eq!(
            menu_gesture(&controller, ButtonTrigger::Single),
            GestureDisposition::Blocked
        );
        assert!(controller.snapshot().panel.is_none());
        for window in [90, 91] {
            let main = WindowToken::from_identity(
                "sayall".into(),
                ApplicationAdapterKind::Generic,
                std::process::id(),
                window,
                window,
            );
            *lock(&backend.current) = Some(main.clone());
            controller.handle_edge(ButtonEdge {
                button: RemoteButton::Menu,
                is_pressed: true,
            });
            assert_eq!(
                menu_gesture(&controller, ButtonTrigger::Single),
                GestureDisposition::Handled
            );
            controller.handle_edge(ButtonEdge {
                button: RemoteButton::Menu,
                is_pressed: false,
            });
            assert_eq!(lock(&controller.state).menu_return_target, Some(main));
            assert_eq!(controller.snapshot().application_id.as_deref(), Some("a"));
            controller.set_template_menu_focus(false);
        }
    }

    #[test]
    fn expired_or_cancelled_menu_restore_work_never_activates_a_window() {
        for expired in [true, false] {
            let backend = Arc::new(TaskBackend::default());
            *lock(&backend.current) = Some(token("a", 1));
            let controller = menu_controller(Arc::clone(&backend));
            assert_eq!(
                menu_gesture(&controller, ButtonTrigger::Single),
                GestureDisposition::Handled
            );
            let (token, generation) = {
                let state = lock(&controller.state);
                (state.menu_return_target.clone().unwrap(), state.generation)
            };
            if !expired {
                controller.set_template_menu_focus(false);
            }
            let (reply, receiver) = mpsc::sync_channel(1);
            controller
                .sender
                .send(Work::RestoreForeground {
                    token,
                    generation,
                    expires_at: if expired {
                        Instant::now() - Duration::from_secs(1)
                    } else {
                        Instant::now() + Duration::from_secs(1)
                    },
                    reply,
                })
                .unwrap();
            assert!(matches!(
                receiver.recv_timeout(Duration::from_secs(1)).unwrap(),
                Err(ApplicationControlError::StaleToken)
            ));
            assert!(lock(&backend.restored).is_empty());
        }
    }
    // Task-switch remains an explicit selectable action; the factory TV single
    // now sends Tab and must not implicitly opt these route tests into this mode.
    fn task_action_controller() -> SceneController {
        let c = controller();
        {
            let mut state = lock(&c.state);
            let mut mappings = state
                .configuration
                .template_mappings(BUILTIN_AGENT_TEMPLATE_ID)
                .unwrap();
            mappings.actions.get_mut(&RemoteButton::Tv).unwrap().single =
                ButtonAction::TaskSwitch {
                    view: TaskSwitchView::Applications,
                };
            state
                .configuration
                .update_button_mapping_template(BUILTIN_AGENT_TEMPLATE_ID, mappings)
                .unwrap();
        }
        c
    }
    fn task_route(
        c: &SceneController,
        button: RemoteButton,
        trigger: ButtonTrigger,
        native: bool,
    ) -> GestureDisposition {
        c.handle_gesture(RoutedGesture {
            gesture: FiredGesture { button, trigger },
            native_delivered: native,
        })
    }
    fn tv_gesture_edge(
        c: &SceneController,
        recognizer: &mut GestureRecognizer,
        down: bool,
        now: Instant,
    ) -> Vec<GestureDisposition> {
        c.handle_edge(ButtonEdge {
            button: RemoteButton::Tv,
            is_pressed: down,
        });
        let gestures = if down {
            recognizer.press(RemoteButton::Tv, now)
        } else {
            recognizer.release(RemoteButton::Tv, now)
        };
        gestures
            .into_iter()
            .map(|trigger| task_route(c, RemoteButton::Tv, trigger, false))
            .collect()
    }

    #[test]
    fn task_gestures_keep_configured_tv_short_independent_in_both_system_views() {
        for view in [TaskSwitchView::Applications, TaskSwitchView::Desktops] {
            for keys in [vec![KeyCode::Tab], vec![KeyCode::Shift, KeyCode::Tab]] {
                let backend = Arc::new(TaskBackend::default());
                *lock(&backend.current) = Some(token("a", 1));
                let mut c = controller();
                c.application = backend.clone();
                let original = {
                    let mut state = lock(&c.state);
                    let mut mappings = state
                        .configuration
                        .template_mappings(BUILTIN_AGENT_TEMPLATE_ID)
                        .unwrap();
                    let actions = mappings.actions.get_mut(&RemoteButton::Tv).unwrap();
                    actions.single = ButtonAction::Shortcut {
                        chord: KeyChord { keys: keys.clone() },
                    };
                    actions.long = ButtonAction::TaskSwitch { view };
                    state
                        .configuration
                        .update_button_mapping_template(BUILTIN_AGENT_TEMPLATE_ID, mappings.clone())
                        .unwrap();
                    mappings
                };
                let mut recognizer = GestureRecognizer::new();
                recognizer.configure(&original);
                let start = Instant::now();
                assert!(tv_gesture_edge(&c, &mut recognizer, true, start).is_empty());
                let long = recognizer.advance(start + LONG_PRESS_THRESHOLD);
                assert_eq!(long, vec![(RemoteButton::Tv, ButtonTrigger::Long)]);
                assert_eq!(
                    task_route(&c, RemoteButton::Tv, long[0].1, false),
                    GestureDisposition::Handled
                );
                let shell = token("shell", 2).as_task_switcher();
                *lock(&backend.current) = Some(shell.clone());
                observe(&c.state, shell);
                let published = active_button_mapping_from(&lock(&c.state)).unwrap();
                assert_eq!(
                    published.actions(RemoteButton::Tv),
                    original.actions(RemoteButton::Tv),
                    "task mode must preserve configured TV gestures"
                );
                assert!(
                    tv_gesture_edge(&c, &mut recognizer, true, start + LONG_PRESS_THRESHOLD)
                        .is_empty()
                );
                assert!(tv_gesture_edge(
                    &c,
                    &mut recognizer,
                    false,
                    start + LONG_PRESS_THRESHOLD + Duration::from_millis(50)
                )
                .is_empty());
                assert_eq!(
                    lock(&backend.sent).len(),
                    1,
                    "opening long press has no trailing Tab"
                );
                for n in 1..=3 {
                    let now = start + Duration::from_secs(n);
                    let before = lock(&backend.sent).len();
                    assert!(tv_gesture_edge(&c, &mut recognizer, true, now).is_empty());
                    assert_eq!(
                        lock(&backend.sent).len(),
                        before,
                        "DOWN must not cancel before arbitration"
                    );
                    assert_eq!(
                        tv_gesture_edge(
                            &c,
                            &mut recognizer,
                            false,
                            now + Duration::from_millis(40)
                        ),
                        vec![GestureDisposition::PassThrough]
                    );
                    assert_eq!(
                        c.active_button_mapping()
                            .unwrap()
                            .action_for(RemoteButton::Tv, ButtonTrigger::Single),
                        ButtonAction::Shortcut {
                            chord: KeyChord { keys: keys.clone() }
                        }
                    );
                    assert_eq!(
                        lock(&backend.sent).len(),
                        before,
                        "ordinary mapper owns TV injection, not the task injector"
                    );
                    assert!(lock(&c.state).task_switch.is_some());
                }
                assert_eq!(
                    task_route(&c, RemoteButton::Left, ButtonTrigger::Single, false),
                    GestureDisposition::Handled
                );
                assert_eq!(
                    task_route(&c, RemoteButton::Ok, ButtonTrigger::Single, false),
                    GestureDisposition::Handled
                );
                assert!(lock(&c.state).task_switch.is_none());
                assert!(!lock(&backend.sent)
                    .iter()
                    .any(|chord| chord.keys == vec![KeyCode::Escape]));
                observe(&c.state, token("a", 3));
                *lock(&backend.current) = Some(token("a", 3));
                let now = start + Duration::from_secs(5);
                assert!(tv_gesture_edge(&c, &mut recognizer, true, now).is_empty());
                assert_eq!(
                    tv_gesture_edge(&c, &mut recognizer, false, now + Duration::from_millis(40)),
                    vec![GestureDisposition::PassThrough],
                    "ordinary foreground returns to the normal mapper"
                );
            }
        }
    }

    #[test]
    fn task_gestures_do_not_leak_a_held_tv_action_after_external_exit() {
        let backend = Arc::new(TaskBackend::default());
        *lock(&backend.current) = Some(token("a", 1));
        let mut c = controller();
        c.application = backend.clone();
        assert_eq!(
            task_route(&c, RemoteButton::Tv, ButtonTrigger::Long, false),
            GestureDisposition::Handled
        );
        let shell = token("shell", 2).as_task_switcher();
        *lock(&backend.current) = Some(shell.clone());
        observe(&c.state, shell);
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&active_button_mapping_from(&lock(&c.state)).unwrap());
        let start = Instant::now();
        assert!(tv_gesture_edge(&c, &mut recognizer, true, start).is_empty());
        *lock(&backend.current) = Some(token("other", 3));
        observe(&c.state, token("other", 3));
        assert_eq!(
            tv_gesture_edge(
                &c,
                &mut recognizer,
                false,
                start + Duration::from_millis(40)
            ),
            vec![GestureDisposition::Blocked]
        );
        assert_eq!(lock(&backend.sent).len(), 1);
    }

    #[test]
    fn task_switch_launch_navigation_native_confirm_and_external_cancel() {
        let backend = Arc::new(TaskBackend::default());
        *lock(&backend.current) = Some(token("a", 1));
        let mut c = task_action_controller();
        c.application = backend.clone();
        assert_eq!(
            task_route(&c, RemoteButton::Tv, ButtonTrigger::Single, false),
            GestureDisposition::Handled
        );
        assert_eq!(
            lock(&backend.sent)[0].keys,
            vec![KeyCode::Control, KeyCode::Alt, KeyCode::Tab]
        );
        let shell = token("shell", 2).as_task_switcher();
        *lock(&backend.current) = Some(shell.clone());
        observe(&c.state, shell);
        assert!(lock(&c.state).task_switch.is_some());
        assert_eq!(
            task_route(&c, RemoteButton::Left, ButtonTrigger::Single, false),
            GestureDisposition::Handled
        );
        assert_eq!(lock(&backend.sent)[1].keys, vec![KeyCode::Left]);
        assert_eq!(
            task_route(&c, RemoteButton::Right, ButtonTrigger::Single, true),
            GestureDisposition::Handled
        );
        assert_eq!(lock(&backend.sent).len(), 2, "native arrow reused");
        assert_eq!(
            task_route(&c, RemoteButton::Ok, ButtonTrigger::Single, true),
            GestureDisposition::Handled
        );
        assert_eq!(
            lock(&backend.sent).len(),
            2,
            "native Enter is never injected twice"
        );
        assert!(lock(&c.state).task_switch.is_none());
    }
    #[test]
    fn task_switch_failed_launch_does_not_arm_and_custom_chat_actions_never_escape() {
        let backend = Arc::new(TaskBackend::default());
        *lock(&backend.current) = Some(token("a", 1));
        let mut c = task_action_controller();
        c.application = backend.clone();
        backend
            .fail
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            task_route(&c, RemoteButton::Tv, ButtonTrigger::Long, false),
            GestureDisposition::Blocked
        );
        assert!(lock(&c.state).task_switch.is_none());
        assert!(lock(&backend.sent).is_empty());
    }

    #[test]
    fn task_gestures_route_configured_shortcuts_through_mapper_while_target_is_pending() {
        for view in [TaskSwitchView::Applications, TaskSwitchView::Desktops] {
            let backend = Arc::new(TaskBackend::default());
            *lock(&backend.current) = Some(token("a", 1));
            let mut c = controller();
            c.application = backend.clone();
            {
                let mut state = lock(&c.state);
                let mut mappings = state
                    .configuration
                    .template_mappings(BUILTIN_AGENT_TEMPLATE_ID)
                    .unwrap();
                mappings.actions.get_mut(&RemoteButton::Tv).unwrap().long =
                    ButtonAction::TaskSwitch { view };
                state
                    .configuration
                    .update_button_mapping_template(BUILTIN_AGENT_TEMPLATE_ID, mappings)
                    .unwrap();
            }
            assert_eq!(
                task_route(&c, RemoteButton::Tv, ButtonTrigger::Long, false),
                GestureDisposition::Handled
            );
            let mut recognizer = GestureRecognizer::new();
            recognizer.configure(&c.active_button_mapping().unwrap());
            for (n, current) in [
                token("a", 1),
                token("shell", 2).as_task_staging(),
                token("shell", 3).as_task_switcher(),
            ]
            .into_iter()
            .enumerate()
            {
                *lock(&backend.current) = Some(current.clone());
                observe(&c.state, current);
                let now = Instant::now() + Duration::from_secs(n as u64);
                assert!(tv_gesture_edge(&c, &mut recognizer, true, now).is_empty());
                assert_eq!(
                    tv_gesture_edge(&c, &mut recognizer, false, now + Duration::from_millis(40)),
                    vec![GestureDisposition::PassThrough],
                    "ordinary TV shortcut must not depend on task-window recognition"
                );
                assert_eq!(
                    c.active_button_mapping()
                        .unwrap()
                        .action_for(RemoteButton::Tv, ButtonTrigger::Single),
                    ButtonAction::Shortcut {
                        chord: KeyChord {
                            keys: vec![KeyCode::Tab]
                        }
                    }
                );
                assert_eq!(
                    lock(&backend.sent).len(),
                    1,
                    "only the launch uses the guarded task injector; Tab uses the ordinary mapper"
                );
                assert!(lock(&c.state).task_switch.is_some());
            }
        }
    }
    #[test]
    fn task_switch_staging_timeout_is_terminal_without_escape_or_reopening() {
        let backend = Arc::new(TaskBackend::default());
        *lock(&backend.current) = Some(token("a", 1));
        let mut c = controller();
        c.application = backend.clone();
        task_route(&c, RemoteButton::Tv, ButtonTrigger::Long, false);
        let staging = token("shell", 2).as_task_staging();
        *lock(&backend.current) = Some(staging.clone());
        observe(&c.state, staging.clone());
        let deadline =
            lock(&c.state).task_switch.as_ref().unwrap().started_at + TASK_TARGET_TIMEOUT;
        expire_unconfirmed_task(&c.state, &c.callbacks, deadline);
        assert!(lock(&c.state).task_switch.is_none());
        assert_eq!(lock(&backend.sent).len(), 1);
        observe(&c.state, staging);
        assert!(lock(&c.state).task_switch.is_none());
        assert_eq!(lock(&backend.sent).len(), 1);
    }
    #[test]
    fn task_switch_late_staging_observation_cannot_clear_confirmed_newer_target() {
        let backend = Arc::new(TaskBackend::default());
        *lock(&backend.current) = Some(token("a", 1));
        let mut c = task_action_controller();
        c.application = backend;
        assert_eq!(
            task_route(&c, RemoteButton::Tv, ButtonTrigger::Single, false),
            GestureDisposition::Handled
        );
        observe(&c.state, token("shell", 3).as_task_switcher());
        observe(&c.state, token("shell", 2).as_task_staging());
        assert!(lock(&c.state).task_switch.is_some());
        observe(&c.state, token("unrelated", 4));
        assert!(lock(&c.state).task_switch.is_none());
    }
    #[test]
    fn task_gestures_keep_optional_task_action_and_disabled_configuration_honest() {
        let backend = Arc::new(TaskBackend::default());
        *lock(&backend.current) = Some(token("a", 1));
        let mut c = task_action_controller();
        c.application = backend.clone();
        assert_eq!(
            task_route(&c, RemoteButton::Tv, ButtonTrigger::Long, false),
            GestureDisposition::Handled
        );
        let shell = token("shell", 2).as_task_switcher();
        *lock(&backend.current) = Some(shell.clone());
        observe(&c.state, shell);
        assert_eq!(
            task_route(&c, RemoteButton::Tv, ButtonTrigger::Single, false),
            GestureDisposition::Handled
        );
        assert_eq!(
            lock(&backend.sent)[1].keys,
            vec![KeyCode::Control, KeyCode::Alt, KeyCode::Tab]
        );
        assert!(!lock(&backend.sent)
            .iter()
            .any(|c| c.keys == vec![KeyCode::Escape]));
        let shell = token("shell", 3).as_task_switcher();
        *lock(&backend.current) = Some(shell.clone());
        observe(&c.state, shell);
        lock(&c.state)
            .task_switch
            .as_mut()
            .unwrap()
            .tv_actions
            .single = ButtonAction::Disabled;
        assert_eq!(
            task_route(&c, RemoteButton::Tv, ButtonTrigger::Single, false),
            GestureDisposition::Blocked
        );
        assert_eq!(lock(&backend.sent).len(), 2);
    }
    #[test]
    fn task_switch_cancellation_and_unknown_foreground_never_forward_to_chat() {
        for reason in [
            "configuration",
            "voice",
            "disconnect",
            "exit",
            "external",
            "unknown",
        ] {
            let backend = Arc::new(TaskBackend::default());
            *lock(&backend.current) = Some(token("a", 1));
            let mut c = task_action_controller();
            c.application = backend.clone();
            assert_eq!(
                task_route(&c, RemoteButton::Tv, ButtonTrigger::Single, false),
                GestureDisposition::Handled
            );
            match reason {
                "configuration" => {
                    c.set_configuration(MappingConfiguration::default());
                }
                "voice" => c.notify_voice_active(true),
                "disconnect" => c.cancel_task_switch("disconnected"),
                "exit" => {
                    c.prepare_template_menu_exit();
                }
                "external" => observe(&c.state, token("other", 3)),
                _ => {
                    *lock(&backend.current) = None;
                    assert_eq!(
                        task_route(&c, RemoteButton::Ok, ButtonTrigger::Single, false),
                        GestureDisposition::Blocked
                    );
                }
            }
            assert!(lock(&c.state).task_switch.is_none(), "{reason}");
            assert_eq!(
                lock(&backend.sent).len(),
                1,
                "only the launch chord was injected: {reason}"
            );
        }
    }
    #[test]
    fn task_switch_foreground_event_before_remote_native_confirm_cannot_add_new_template_action() {
        let backend = Arc::new(TaskBackend::default());
        *lock(&backend.current) = Some(token("a", 1));
        let mut c = task_action_controller();
        c.application = backend.clone();
        task_route(&c, RemoteButton::Tv, ButtonTrigger::Single, false);
        observe(&c.state, token("shell", 2).as_task_switcher());
        // Windows has consumed native Enter and foreground notification wins the
        // race with the selected remote's delayed RawInput message.
        observe(&c.state, token("chat", 3));
        *lock(&backend.current) = Some(token("chat", 3));
        assert!(lock(&c.state).task_switch.is_none());
        assert_eq!(
            task_route(&c, RemoteButton::Ok, ButtonTrigger::Single, true),
            GestureDisposition::Blocked
        );
        assert_eq!(lock(&backend.sent).len(), 1);
        // A subsequent captured press is a fresh action, not a timing-based ban.
        assert_eq!(
            task_route(&c, RemoteButton::Ok, ButtonTrigger::Single, false),
            GestureDisposition::PassThrough
        );
    }

    #[test]
    fn task_switch_native_completion_on_new_foreground_does_not_repeat_enter() {
        let backend = Arc::new(TaskBackend::default());
        *lock(&backend.current) = Some(token("a", 1));
        let mut c = task_action_controller();
        c.application = backend.clone();
        task_route(&c, RemoteButton::Tv, ButtonTrigger::Single, false);
        let shell = token("shell", 2).as_task_switcher();
        observe(&c.state, shell);
        *lock(&backend.current) = Some(token("chat", 3));
        assert_eq!(
            task_route(&c, RemoteButton::Ok, ButtonTrigger::Single, true),
            GestureDisposition::Blocked
        );
        assert_eq!(lock(&backend.sent).len(), 1);
        assert!(lock(&c.state).task_switch.is_none());
    }

    fn commit_preference(controller: &SceneController, saved: bool) {
        let id = lock(&controller.state).preference_request.unwrap().0;
        controller.complete_menu_preference_save(id, saved);
    }
    fn menu_gesture(controller: &SceneController, trigger: ButtonTrigger) -> GestureDisposition {
        controller.handle_gesture(RoutedGesture {
            gesture: FiredGesture {
                button: RemoteButton::Menu,
                trigger,
            },
            native_delivered: false,
        })
    }
    // Match the production engine's edge-callback -> recognizer -> route order.
    fn menu_edge(
        controller: &SceneController,
        recognizer: &mut GestureRecognizer,
        down: bool,
        now: std::time::Instant,
    ) -> Vec<GestureDisposition> {
        controller.handle_edge(ButtonEdge {
            button: RemoteButton::Menu,
            is_pressed: down,
        });
        let gestures = if down {
            recognizer.press(RemoteButton::Menu, now)
        } else {
            recognizer.release(RemoteButton::Menu, now)
        };
        gestures
            .into_iter()
            .map(|trigger| menu_gesture(controller, trigger))
            .collect()
    }
    fn open_for_menu_hold(controller: &SceneController) -> GestureRecognizer {
        let mut s = lock(&controller.state);
        open_panel(&mut s, ScenePanel::Template);
        s.template_menu_focused = true;
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&active_scene_mappings_from(&s));
        recognizer
    }
    #[test]
    fn opening_press_cannot_toggle_and_only_panel_recognition_adds_long() {
        let controller = controller();
        let config = lock(&controller.state).configuration.clone();
        let outside = active_scene_mappings_from(&lock(&controller.state));
        assert_eq!(
            outside.action_for(RemoteButton::Menu, ButtonTrigger::Long),
            ButtonAction::Disabled
        );
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&outside);
        let now = std::time::Instant::now();
        assert_eq!(
            menu_edge(&controller, &mut recognizer, true, now),
            vec![GestureDisposition::Handled]
        );
        controller.set_template_menu_focus(true);
        assert_eq!(
            menu_gesture(&controller, ButtonTrigger::Long),
            GestureDisposition::Blocked
        );
        assert!(!controller.snapshot().update_default);
        assert!(menu_edge(
            &controller,
            &mut recognizer,
            false,
            now + LONG_PRESS_THRESHOLD
        )
        .is_empty());
        assert!(controller.snapshot().panel.is_some());
        assert_ne!(
            active_scene_mappings_from(&lock(&controller.state))
                .action_for(RemoteButton::Menu, ButtonTrigger::Long),
            ButtonAction::Disabled
        );
        assert_eq!(lock(&controller.state).configuration, config);
    }
    #[test]
    fn two_menu_holds_toggle_once_each_release_is_silent_and_intent_does_not_reconfigure() {
        let controller = controller();
        let mut recognizer = open_for_menu_hold(&controller);
        let mappings = active_scene_mappings_from(&lock(&controller.state));
        let now = std::time::Instant::now();
        for (index, expected) in [true, false].into_iter().enumerate() {
            let start = now + std::time::Duration::from_secs(index as u64 * 3);
            assert!(menu_edge(&controller, &mut recognizer, true, start).is_empty());
            let fired = recognizer.advance(start + LONG_PRESS_THRESHOLD);
            assert_eq!(fired, vec![(RemoteButton::Menu, ButtonTrigger::Long)]);
            assert_eq!(
                menu_gesture(&controller, fired[0].1),
                GestureDisposition::Handled
            );
            assert_eq!(controller.snapshot().update_default, expected);
            commit_preference(&controller, true);
            controller.handle_edge(ButtonEdge {
                button: RemoteButton::Menu,
                is_pressed: true,
            });
            assert_eq!(
                menu_gesture(&controller, ButtonTrigger::Long),
                GestureDisposition::Blocked
            );
            assert!(recognizer
                .advance(start + std::time::Duration::from_secs(2))
                .is_empty());
            assert!(menu_edge(
                &controller,
                &mut recognizer,
                false,
                start + std::time::Duration::from_secs(2)
            )
            .is_empty());
            let s = lock(&controller.state);
            assert!(s.panel.is_some());
            assert!(s.held.is_empty());
            assert!(s.default_event.is_none());
            assert!(s.default_request.is_none());
            assert_eq!(active_scene_mappings_from(&s), mappings);
        }
    }
    #[test]
    fn panel_short_menu_still_cancels_without_changing_or_saving_intent() {
        let controller = controller();
        let mut recognizer = open_for_menu_hold(&controller);
        let now = std::time::Instant::now();
        assert!(menu_edge(&controller, &mut recognizer, true, now).is_empty());
        assert_eq!(
            menu_edge(
                &controller,
                &mut recognizer,
                false,
                now + LONG_PRESS_THRESHOLD / 2
            ),
            vec![GestureDisposition::Handled]
        );
        let s = lock(&controller.state);
        assert!(s.panel.is_none());
        assert!(!s.update_default);
        assert!(s.default_event.is_none());
        assert!(s.manual_selection.is_none());
    }
    #[test]
    fn stale_menu_hold_cannot_change_reopened_cancelled_or_unfocused_panel() {
        for reason in ["focus", "configuration", "disconnect", "exit", "target"] {
            let controller = controller();
            let mut recognizer = open_for_menu_hold(&controller);
            let now = std::time::Instant::now();
            menu_edge(&controller, &mut recognizer, true, now);
            let old_generation = controller.snapshot().generation;
            match reason {
                "focus" => controller.set_template_menu_focus(false),
                "exit" => {
                    assert!(!controller.prepare_template_menu_exit());
                }
                "target" => {
                    lock(&controller.state).token = Some(token("b", 2));
                }
                _ => cancel_state(&mut lock(&controller.state), reason),
            }
            assert_eq!(
                menu_gesture(&controller, ButtonTrigger::Long),
                GestureDisposition::Blocked,
                "{reason}"
            );
            assert!(!controller.set_update_default(old_generation, true));
            {
                let mut s = lock(&controller.state);
                s.held.clear();
                s.waiting_for_release = false;
                s.token = Some(token("a", 1));
                open_panel(&mut s, ScenePanel::Template);
                s.template_menu_focused = true;
            }
            assert_eq!(
                menu_gesture(&controller, ButtonTrigger::Long),
                GestureDisposition::Blocked
            );
            assert!(!controller.snapshot().update_default);
            assert!(!controller.set_update_default(old_generation, true));
        }
    }
    #[test]
    fn control_and_remote_share_intent_but_only_released_confirmation_requests_save() {
        let controller = controller();
        let mut recognizer = open_for_menu_hold(&controller);
        let generation = controller.snapshot().generation;
        assert!(controller.set_update_default(generation, true));
        commit_preference(&controller, true);
        let now = std::time::Instant::now();
        menu_edge(&controller, &mut recognizer, true, now);
        for (_, trigger) in recognizer.advance(now + LONG_PRESS_THRESHOLD) {
            assert_eq!(
                menu_gesture(&controller, trigger),
                GestureDisposition::Handled
            );
        }
        assert!(!controller.snapshot().update_default);
        commit_preference(&controller, true);
        menu_edge(
            &controller,
            &mut recognizer,
            false,
            now + LONG_PRESS_THRESHOLD,
        );
        assert!(controller.set_update_default(generation, true));
        commit_preference(&controller, true);
        assert!(lock(&controller.state).default_event.is_none());
        controller.template_menu_key(generation, RemoteButton::Ok, true);
        assert!(lock(&controller.state).default_request.is_none());
        controller.template_menu_key(generation, RemoteButton::Ok, false);
        assert!(lock(&controller.state).default_request.is_some());
    }
    #[test]
    fn menu_preference_survives_cancel_and_new_controller_without_applying_template() {
        let controller = controller();
        open_for_menu_hold(&controller);
        assert!(controller.set_update_default(controller.snapshot().generation, true));
        assert!(controller.snapshot().preference_pending);
        assert!(!controller.set_update_default(controller.snapshot().generation, false));
        assert_eq!(
            menu_gesture(&controller, ButtonTrigger::Single),
            GestureDisposition::Handled
        );
        assert!(controller.snapshot().panel.is_none());
        commit_preference(&controller, true);
        assert!(controller.snapshot().panel.is_none());
        assert!(lock(&controller.state).manual_selection.is_none());
        assert!(lock(&controller.state).default_request.is_none());
        let saved = lock(&controller.state).configuration.clone();
        let reopened = super::tests::controller();
        reopened.set_configuration(saved);
        open_for_menu_hold(&reopened);
        assert!(reopened.snapshot().update_default);
    }
    #[test]
    fn normal_exit_drains_open_menu_and_pending_preference_before_becoming_ready() {
        let controller = controller();
        open_for_menu_hold(&controller);
        assert!(controller.set_update_default(controller.snapshot().generation, true));
        controller.handle_edge(ButtonEdge {
            button: RemoteButton::Menu,
            is_pressed: true,
        });
        assert!(!controller.prepare_template_menu_exit());
        assert!(controller.snapshot().panel.is_some());
        controller.handle_edge(ButtonEdge {
            button: RemoteButton::Menu,
            is_pressed: false,
        });
        assert!(controller.snapshot().panel.is_none());
        assert!(!controller.prepare_template_menu_exit());
        commit_preference(&controller, true);
        assert!(controller.prepare_template_menu_exit());
        assert!(lock(&controller.state).configuration.menu_update_default);
        assert!(lock(&controller.state).manual_selection.is_none());
    }
    #[test]
    fn failed_preference_restores_saved_value_and_late_response_cannot_change_new_panel() {
        let controller = controller();
        open_for_menu_hold(&controller);
        let generation = controller.snapshot().generation;
        assert!(controller.set_update_default(generation, true));
        let stale_id = lock(&controller.state).preference_request.unwrap().0;
        commit_preference(&controller, false);
        assert!(controller.snapshot().preference_error);
        assert!(!controller.snapshot().update_default);
        assert!(!lock(&controller.state).configuration.menu_update_default);
        assert!(controller.set_update_default(generation, true));
        controller.complete_menu_preference_save(stale_id, true);
        assert!(controller.snapshot().preference_pending);
        controller.set_template_menu_focus(false);
        open_for_menu_hold(&controller);
        let new_generation = controller.snapshot().generation;
        assert!(!controller.snapshot().update_default);
        commit_preference(&controller, true);
        assert_eq!(controller.snapshot().generation, new_generation);
        assert!(!controller.snapshot().update_default);
        assert!(lock(&controller.state).configuration.menu_update_default);
        open_for_menu_hold(&controller);
        assert!(controller.snapshot().update_default);
    }
    #[test]
    fn repeated_direct_gestures_pass_through_and_same_template_notice_is_deduplicated() {
        let controller = controller();
        for _ in 0..7 {
            for button in [
                RemoteButton::Left,
                RemoteButton::Back,
                RemoteButton::VolumeUp,
            ] {
                assert_eq!(
                    controller.handle_gesture(RoutedGesture {
                        gesture: FiredGesture {
                            button,
                            trigger: ButtonTrigger::Single
                        },
                        native_delivered: false
                    }),
                    GestureDisposition::PassThrough
                );
            }
        }
        let (g, f, n) = controller.mapping_notice_candidate();
        controller.confirm_mapping_notice(g, f, n, true);
        let revision = controller.snapshot().mapping_notice_revision;
        observe(&controller.state, token("a", 2));
        let (g, f, n) = controller.mapping_notice_candidate();
        controller.confirm_mapping_notice(g, f, n, true);
        assert_eq!(controller.snapshot().mapping_notice_revision, revision);
    }
    #[test]
    fn failed_default_save_keeps_temporary_selection_and_ignores_stale_result() {
        let controller = controller();
        choose(&mut lock(&controller.state), true);
        let request = lock(&controller.state).default_request.as_ref().unwrap().0;
        controller.complete_default_save(request, false);
        assert_eq!(
            active_template_id(&lock(&controller.state)).as_deref(),
            Some(BUILTIN_CHAT_TEMPLATE_ID)
        );
        assert_eq!(
            lock(&controller.state).default_save_status.as_deref(),
            Some("failed")
        );
        choose(&mut lock(&controller.state), false);
        controller.complete_default_save(request, true);
        assert!(lock(&controller.state).default_save_status.is_none());
    }
    #[test]
    fn temporary_selection_survives_same_program_windows_and_own_windows_only() {
        let state = state();
        choose(&mut lock(&state), false);
        let generation = lock(&state).generation;
        observe(&state, token("a", 2));
        assert_eq!(
            active_template_id(&lock(&state)).as_deref(),
            Some(BUILTIN_CHAT_TEMPLATE_ID)
        );
        assert_eq!(lock(&state).generation, generation);
        let own = WindowToken::from_identity(
            "sayall".into(),
            ApplicationAdapterKind::Generic,
            std::process::id(),
            3,
            3,
        );
        observe(&state, own);
        assert_eq!(lock(&state).token.as_ref().unwrap().application_id(), "a");
        observe(&state, token("b", 4));
        observe(&state, token("a", 5));
        assert_eq!(
            active_template_id(&lock(&state)).as_deref(),
            Some(BUILTIN_AGENT_TEMPLATE_ID)
        );
    }
    #[test]
    fn manual_selection_survives_program_changes_when_program_defaults_disabled() {
        let state = state();
        lock(&state).configuration.button_mapping_follow_enabled = false;
        choose(&mut lock(&state), false);
        let selected = active_button_mapping_from(&lock(&state)).unwrap();
        observe(&state, token("a", 2));
        observe(&state, token("b", 3));
        assert_eq!(
            active_button_mapping_from(&lock(&state)),
            Some(selected.clone()),
            "manual mode must keep the executable mapping across external applications"
        );
        observe(&state, token("a", 4));
        assert_eq!(active_button_mapping_from(&lock(&state)), Some(selected));
        assert_eq!(
            active_template_id(&lock(&state)).as_deref(),
            Some(BUILTIN_CHAT_TEMPLATE_ID)
        );
    }
    #[test]
    fn manual_mode_keeps_mapping_through_missing_foreground_identity() {
        let state = state();
        lock(&state).configuration.button_mapping_follow_enabled = false;
        choose(&mut lock(&state), false);
        let selected = active_button_mapping_from(&lock(&state)).unwrap();
        let generation = lock(&state).generation;
        apply_foreground_observation(
            &state,
            &Arc::new(RwLock::new(Vec::new())),
            generation,
            Err(ApplicationControlError::NoForegroundWindow),
        );
        assert_eq!(active_button_mapping_from(&lock(&state)), Some(selected));
    }
    #[test]
    fn manual_mode_configuration_refresh_and_menu_preference_keep_current_mapping() {
        let controller = controller();
        lock(&controller.state)
            .configuration
            .button_mapping_follow_enabled = false;
        choose(&mut lock(&controller.state), false);
        let selected = controller.active_button_mapping().unwrap();
        let mut configuration = lock(&controller.state).configuration.clone();
        configuration.menu_template_switch_enabled = false;
        let bindings = configuration.application_bindings.clone();
        controller.set_configuration(configuration.clone());
        controller.set_configuration(configuration);
        assert_eq!(controller.active_button_mapping(), Some(selected));
        assert_eq!(
            lock(&controller.state).configuration.application_bindings,
            bindings
        );
    }
    #[test]
    fn button_selection_requires_mapping_ack_and_rejects_late_ack_after_timeout() {
        let controller = controller();
        let before = controller.active_button_mapping();
        let callbacks = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&callbacks);
        controller.subscribe(Arc::new(move |event| {
            if let SceneEvent::Snapshot { snapshot } = event {
                lock(&observed).push((snapshot.generation, snapshot.foreground_generation));
            }
        }));
        assert!(controller
            .select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
            .is_err());
        assert_eq!(controller.active_button_mapping(), before);
        let (generation, foreground) = lock(&callbacks)[0];
        let mut notice = controller.mapping_notice_candidate().2;
        notice.template_id = Some(BUILTIN_CHAT_TEMPLATE_ID.into());
        controller.confirm_mapping_notice(generation, foreground, notice, true);
        assert!(controller.snapshot().mapping_notice.is_none());
        assert_eq!(controller.active_button_mapping(), before);
    }
    #[test]
    fn foreground_change_cancels_unconfirmed_button_selection() {
        let controller = controller();
        let state = Arc::clone(&controller.state);
        controller.subscribe(Arc::new(move |event| {
            if matches!(event, SceneEvent::Snapshot { .. }) {
                observe(&state, token("b", 9));
            }
        }));
        assert!(controller
            .select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
            .is_err());
        assert!(controller.active_button_mapping().is_none());
    }
    #[test]
    fn normal_exit_cancels_unconfirmed_button_selection_before_shutdown() {
        let controller = Arc::new(controller());
        let before = controller.active_button_mapping();
        let weak = Arc::downgrade(&controller);
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        controller.subscribe(Arc::new(move |event| {
            if matches!(event, SceneEvent::Snapshot { .. })
                && !cancelled.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                let controller = weak.upgrade().unwrap();
                assert!(controller.prepare_template_menu_exit());
                assert!(lock(&controller.state).pending_selection.is_none());
            }
        }));
        assert!(controller
            .select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
            .is_err());
        assert_eq!(controller.active_button_mapping(), before);
    }
    fn acknowledged_controller() -> Arc<SceneController> {
        let controller = Arc::new(controller());
        let weak = Arc::downgrade(&controller);
        controller.subscribe(Arc::new(move |event| {
            if matches!(event, SceneEvent::Snapshot { .. }) {
                let controller = weak.upgrade().unwrap();
                let (generation, foreground, notice) = controller.mapping_notice_candidate();
                controller.confirm_mapping_notice(generation, foreground, notice, true);
            }
        }));
        controller
    }
    #[test]
    fn button_selection_and_remote_menu_choose_the_same_executable_mapping() {
        let controller = acknowledged_controller();
        lock(&controller.state)
            .configuration
            .button_mapping_follow_enabled = false;
        let bindings = lock(&controller.state)
            .configuration
            .application_bindings
            .clone();
        let snapshot = controller
            .select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
            .unwrap();
        assert_eq!(
            snapshot.mapping_notice.unwrap().template_id.as_deref(),
            Some(BUILTIN_CHAT_TEMPLATE_ID)
        );
        let from_button = controller.active_button_mapping();
        let menu_state = state();
        lock(&menu_state)
            .configuration
            .button_mapping_follow_enabled = false;
        choose(&mut lock(&menu_state), false);
        assert_eq!(from_button, active_button_mapping_from(&lock(&menu_state)));
        observe(&controller.state, token("b", 3));
        assert_eq!(controller.active_button_mapping(), from_button);
        let configuration = lock(&controller.state).configuration.clone();
        controller.set_configuration(configuration);
        assert_eq!(controller.active_button_mapping(), from_button);
        assert_eq!(
            lock(&controller.state).configuration.application_bindings,
            bindings
        );
        assert!(lock(&controller.state).default_request.is_none());
        // An unchanged effective profile still needs to acknowledge this request.
        assert!(controller
            .select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
            .is_ok());
    }
    #[test]
    fn explicit_common_selection_overrides_default_until_external_program_change() {
        let controller = acknowledged_controller();
        assert!(controller.active_button_mapping().is_some());
        let result = controller.select_template(None).unwrap();
        assert!(result.mapping_notice.unwrap().template_id.is_none());
        assert!(controller.active_button_mapping().is_none());
        observe(&controller.state, token("a", 2));
        assert!(controller.active_button_mapping().is_none());
        observe(&controller.state, token("b", 3));
        observe(&controller.state, token("a", 4));
        assert_eq!(
            active_template_id(&lock(&controller.state)).as_deref(),
            Some(BUILTIN_AGENT_TEMPLATE_ID)
        );
    }
    #[test]
    fn reenabling_follow_keeps_only_the_current_program_manual_override() {
        for switch_program in [false, true] {
            let controller = acknowledged_controller();
            lock(&controller.state)
                .configuration
                .button_mapping_follow_enabled = false;
            controller
                .select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
                .unwrap();
            if switch_program {
                observe(&controller.state, token("b", 3));
            }
            let mut configuration = lock(&controller.state).configuration.clone();
            configuration.button_mapping_follow_enabled = true;
            controller.set_configuration(configuration);
            if switch_program {
                assert!(controller.active_button_mapping().is_none());
                observe(&controller.state, token("a", 4));
                assert_eq!(
                    active_template_id(&lock(&controller.state)).as_deref(),
                    Some(BUILTIN_AGENT_TEMPLATE_ID)
                );
            } else {
                assert_eq!(
                    active_template_id(&lock(&controller.state)).as_deref(),
                    Some(BUILTIN_CHAT_TEMPLATE_ID)
                );
            }
        }
    }
    #[test]
    fn invalid_or_busy_button_selection_never_changes_the_current_mapping() {
        let controller = acknowledged_controller();
        let before = controller.active_button_mapping();
        assert!(controller
            .select_template(Some("missing-template"))
            .is_err());
        lock(&controller.state).voice_active = true;
        assert!(controller
            .select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
            .is_err());
        lock(&controller.state).voice_active = false;
        lock(&controller.state).held.insert(RemoteButton::Back);
        assert!(controller
            .select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
            .is_err());
        assert_eq!(controller.active_button_mapping(), before);
        assert!(lock(&controller.state).manual_selection.is_none());
    }
    #[test]
    fn restarting_runtime_keeps_preferences_and_defaults_but_not_manual_selection() {
        let controller = acknowledged_controller();
        lock(&controller.state)
            .configuration
            .button_mapping_follow_enabled = false;
        lock(&controller.state).configuration.menu_update_default = true;
        controller
            .select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
            .unwrap();
        let configuration = lock(&controller.state).configuration.clone();
        let json = serde_json::to_string(&configuration).unwrap();
        let restarted = State {
            configuration: serde_json::from_str(&json).unwrap(),
            token: Some(token("a", 2)),
            ..Default::default()
        };
        assert!(restarted.configuration.menu_update_default);
        assert_eq!(
            restarted.configuration.application_bindings,
            configuration.application_bindings
        );
        assert!(active_button_mapping_from(&restarted).is_none());
    }
    #[cfg(windows)]
    mod mapping_worker_tests {
        use super::*;
        use crate::button_mapping::{ButtonMappingRuntime, EngineMessage, MappingInjector};
        use crate::raw_input::RawInputSnapshot;
        use crate::send_input::{MouseClickKind, MoveDirection, ScrollDirection};

        #[derive(Default)]
        struct RecordedActions(Mutex<Vec<KeyChord>>);
        impl MappingInjector for RecordedActions {
            fn tap(&self, chord: &KeyChord) -> Result<(), String> {
                lock(&self.0).push(chord.clone());
                Ok(())
            }
            fn scroll(&self, _: ScrollDirection, _: u16) -> Result<(), String> {
                panic!("unexpected scroll")
            }
            fn mouse_click(&self, _: MouseClickKind) -> Result<(), String> {
                panic!("unexpected click")
            }
            fn mouse_move(&self, _: MoveDirection, _: u16) -> Result<(), String> {
                panic!("unexpected move")
            }
            fn launch_app(&self, _: &str) -> Result<(), String> {
                panic!("unexpected launch")
            }
            fn focus_frontmost(&self) -> Result<(), String> {
                panic!("unexpected focus")
            }
            fn launch_app_and_focus(
                &self,
                target: &str,
                _: Option<&crate::focus::AppFocusProfile>,
            ) -> Result<(), String> {
                self.launch_app(target)
            }
            fn learn_focus_target(&self) -> Result<crate::focus::RecordedFocusTarget, String> {
                panic!("unexpected focus learning")
            }
        }
        fn mappings(key: KeyCode) -> ButtonMappings {
            let mut mappings = ButtonMappings::default();
            mappings.enabled = true;
            mappings.actions.insert(
                RemoteButton::Back,
                ButtonActions {
                    single: ButtonAction::Shortcut {
                        chord: KeyChord { keys: vec![key] },
                    },
                    ..Default::default()
                },
            );
            mappings
        }
        fn press(runtime: &ButtonMappingRuntime, button: RemoteButton) {
            for is_pressed in [true, false] {
                runtime
                    .sender()
                    .send(EngineMessage::DriverEdge(ButtonEdge { button, is_pressed }))
                    .unwrap();
            }
            assert!(runtime.wait_for_idle(Duration::from_secs(2)));
        }
        #[test]
        fn delayed_snapshot_cannot_overwrite_a_newer_acknowledged_mapping() {
            use std::sync::atomic::{AtomicBool, Ordering};

            let _gate = crate::key_gate::GATE_TEST_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let _key_gate = crate::key_gate::KeyGate::start();
            let controller = Arc::new(controller());
            let recorded = Arc::new(RecordedActions::default());
            let runtime = Arc::new(ButtonMappingRuntime::new(
                recorded.clone(),
                Arc::new(crate::UsageCounters::default()),
                Arc::new(Mutex::new(RawInputSnapshot::default())),
            ));
            runtime.set_input_context(crate::RemoteModel::Rc001, true);
            {
                let mut state = lock(&controller.state);
                state
                    .configuration
                    .update_button_mapping_template(BUILTIN_AGENT_TEMPLATE_ID, mappings(KeyCode::A))
                    .unwrap();
                state
                    .configuration
                    .update_button_mapping_template(BUILTIN_CHAT_TEMPLATE_ID, mappings(KeyCode::B))
                    .unwrap();
            }
            let (new_snapshot, new_snapshot_received) = mpsc::channel();
            controller.subscribe(Arc::new(move |event| {
                if let SceneEvent::Snapshot { snapshot } = event {
                    if snapshot.template_id.as_deref() == Some(BUILTIN_CHAT_TEMPLATE_ID) {
                        new_snapshot.send(()).unwrap();
                    }
                }
            }));
            let (old_read, old_read_received) = mpsc::channel();
            let (resume_old, resume_old_received) = mpsc::channel();
            let pause_once = AtomicBool::new(true);
            let resume_old_received = Mutex::new(resume_old_received);
            crate::subscribe_button_profile_with_hook(&controller, &runtime, move || {
                if pause_once.swap(false, Ordering::SeqCst) {
                    old_read.send(()).unwrap();
                    lock(&resume_old_received)
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap();
                }
            });
            let (new_queued, new_queued_received) = mpsc::channel();
            controller.subscribe(Arc::new(move |event| {
                if let SceneEvent::Snapshot { snapshot } = event {
                    if snapshot.template_id.as_deref() == Some(BUILTIN_CHAT_TEMPLATE_ID) {
                        new_queued.send(()).unwrap();
                    }
                }
            }));
            let old_controller = controller.clone();
            let old = std::thread::spawn(move || old_controller.emit_snapshot());
            old_read_received
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            let new_controller = controller.clone();
            let new = std::thread::spawn(move || {
                new_controller.select_template(Some(BUILTIN_CHAT_TEMPLATE_ID))
            });
            new_snapshot_received
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            // Before the fix B queues and acknowledges while A is paused after
            // reading. After the fix B waits for A's complete submission.
            if new_queued_received
                .recv_timeout(Duration::from_millis(200))
                .is_ok()
            {
                assert!(runtime.wait_for_idle(Duration::from_secs(2)));
            }
            resume_old.send(()).unwrap();
            old.join().unwrap();
            let acknowledged = new.join().unwrap().unwrap();
            assert_eq!(
                acknowledged.mapping_notice.unwrap().template_id.as_deref(),
                Some(BUILTIN_CHAT_TEMPLATE_ID)
            );
            assert!(runtime.wait_for_idle(Duration::from_secs(2)));
            assert_eq!(
                controller
                    .snapshot()
                    .mapping_notice
                    .unwrap()
                    .template_id
                    .as_deref(),
                Some(BUILTIN_CHAT_TEMPLATE_ID)
            );
            press(&runtime, RemoteButton::Back);
            assert_eq!(
                lock(&recorded.0).as_slice(),
                &[KeyChord {
                    keys: vec![KeyCode::B]
                }]
            );
        }
        #[test]
        fn manual_selection_drives_the_real_mapping_worker_across_menu_and_program_changes() {
            let _gate = crate::key_gate::GATE_TEST_LOCK
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let _key_gate = crate::key_gate::KeyGate::start();
            let controller = Arc::new(controller());
            let recorded = Arc::new(RecordedActions::default());
            let runtime = Arc::new(ButtonMappingRuntime::new(
                recorded.clone(),
                Arc::new(crate::UsageCounters::default()),
                Arc::new(Mutex::new(RawInputSnapshot::default())),
            ));
            crate::subscribe_button_profile(&controller, &runtime);
            let weak = Arc::downgrade(&controller);
            runtime.subscribe_button_edges(Arc::new(move |edge| {
                weak.upgrade().unwrap().handle_edge(edge)
            }));
            let weak = Arc::downgrade(&controller);
            runtime.set_gesture_handler(Some(Arc::new(move |gesture| {
                weak.upgrade().unwrap().handle_gesture(gesture)
            })));
            runtime.set_input_context(crate::RemoteModel::Rc001, true);
            let mut configuration = lock(&controller.state).configuration.clone();
            configuration.button_mapping_follow_enabled = false;
            configuration.common_mappings = mappings(KeyCode::C);
            configuration
                .update_button_mapping_template(BUILTIN_AGENT_TEMPLATE_ID, mappings(KeyCode::A))
                .unwrap();
            configuration
                .update_button_mapping_template(BUILTIN_CHAT_TEMPLATE_ID, mappings(KeyCode::B))
                .unwrap();
            runtime.set_mappings(configuration.common_mappings.clone());
            controller.set_configuration(configuration);
            controller
                .select_template(Some(BUILTIN_AGENT_TEMPLATE_ID))
                .unwrap();
            press(&runtime, RemoteButton::Back);
            let observe_actual = |application, window| {
                let generation = controller.snapshot().generation;
                apply_foreground_observation(
                    &controller.state,
                    &controller.callbacks,
                    generation,
                    Ok(token(application, window)),
                );
                assert!(runtime.wait_for_idle(Duration::from_secs(2)));
            };
            observe_actual("b", 2);
            press(&runtime, RemoteButton::Menu);
            controller.set_template_menu_focus(true);
            let generation = controller.snapshot().generation;
            assert!(controller.template_menu_key(generation, RemoteButton::Down, true));
            assert!(controller.template_menu_key(generation, RemoteButton::Down, false));
            assert!(controller.template_menu_key(generation, RemoteButton::Ok, true));
            assert!(controller.template_menu_key(generation, RemoteButton::Ok, false));
            assert!(runtime.wait_for_idle(Duration::from_secs(2)));
            press(&runtime, RemoteButton::Back);
            observe_actual("c", 3);
            press(&runtime, RemoteButton::Back);
            observe_actual("a", 4);
            let mut configuration = lock(&controller.state).configuration.clone();
            configuration.button_mapping_follow_enabled = true;
            controller.set_configuration(configuration);
            assert!(runtime.wait_for_idle(Duration::from_secs(2)));
            press(&runtime, RemoteButton::Back);
            controller.select_template(None).unwrap();
            press(&runtime, RemoteButton::Back);
            assert_eq!(
                lock(&recorded.0)
                    .iter()
                    .map(|chord| chord.keys.clone())
                    .collect::<Vec<_>>(),
                vec![
                    vec![KeyCode::A],
                    vec![KeyCode::B],
                    vec![KeyCode::B],
                    vec![KeyCode::A],
                    vec![KeyCode::C]
                ]
            );
        }
    }
    #[test]
    fn cold_start_manual_menu_does_not_require_an_external_application() {
        let controller = controller();
        {
            let mut state = lock(&controller.state);
            state.token = None;
            state.configuration.button_mapping_follow_enabled = false;
        }
        assert!(controller
            .active_scene_mappings()
            .actions
            .contains_key(&RemoteButton::Menu));
        assert_eq!(
            menu_gesture(&controller, ButtonTrigger::Single),
            GestureDisposition::Handled
        );
        controller.set_template_menu_focus(true);
        let generation = controller.snapshot().generation;
        assert!(!controller.set_update_default(generation, true));
        assert!(controller.template_menu_key(generation, RemoteButton::Ok, true));
        assert!(controller.template_menu_key(generation, RemoteButton::Ok, false));
        assert!(controller.active_button_mapping().is_some());
    }
    #[test]
    fn menu_captures_original_program_and_waits_for_both_release_channels() {
        let state = state();
        let mut state = lock(&state);
        open_panel(&mut state, ScenePanel::Template);
        state.template_menu_focused = true;
        state.update_default = true;
        state.panel.as_mut().unwrap().selected = 1;
        state.held.insert(RemoteButton::Ok);
        state.menu_native_held.insert(RemoteButton::Ok);
        state.menu_close_after_release = Some(true);
        assert!(!finish_template_menu(&mut state));
        state.held.clear();
        assert!(!finish_template_menu(&mut state));
        state.menu_native_held.clear();
        assert!(finish_template_menu(&mut state));
        assert!(
            matches!(&state.default_event,Some(SceneEvent::DefaultTemplatePersistenceRequested{application_id,template_id,..}) if application_id=="a" && template_id==BUILTIN_CHAT_TEMPLATE_ID)
        );
        open_panel(&mut state, ScenePanel::Template);
        assert!(!state.update_default);
    }
    #[test]
    fn stale_observation_cannot_replace_config_generation_and_focus_loss_never_selects() {
        let state = state();
        let stale = lock(&state).generation;
        choose(&mut lock(&state), false);
        apply_foreground_observation(
            &state,
            &Arc::new(RwLock::new(Vec::new())),
            stale,
            Ok(token("b", 9)),
        );
        assert_eq!(lock(&state).token.as_ref().unwrap().application_id(), "a");
        {
            let mut s = lock(&state);
            open_panel(&mut s, ScenePanel::Template);
            s.template_menu_focused = true;
            s.panel.as_mut().unwrap().selected = 2;
        }
        observe(&state, token("a", 2));
        assert!(lock(&state).panel.is_none());
        assert_eq!(
            active_template_id(&lock(&state)).as_deref(),
            Some(BUILTIN_CHAT_TEMPLATE_ID)
        );
    }
    #[test]
    fn ordinary_keys_have_no_scene_route_or_worker_work_even_with_menu_enabled() {
        let state = state();
        let markers = active_scene_mappings_from(&lock(&state));
        assert_eq!(markers.actions.len(), 1);
        assert!(markers.actions.contains_key(&RemoteButton::Menu));
        let profile = active_button_mapping_from(&lock(&state)).unwrap();
        for button in [
            RemoteButton::Left,
            RemoteButton::Right,
            RemoteButton::Back,
            RemoteButton::VolumeUp,
            RemoteButton::VolumeDown,
        ] {
            assert!(matches!(
                profile.action_for(button, ButtonTrigger::Single),
                ButtonAction::Shortcut { .. }
            ));
            assert_eq!(
                profile.action_for(button, ButtonTrigger::Double),
                ButtonAction::Disabled
            );
            assert_eq!(
                profile.action_for(button, ButtonTrigger::Long),
                ButtonAction::Disabled
            );
        }
    }
    #[test]
    fn menu_wraps_and_cancel_retains_native_release_barrier() {
        let state = state();
        let mut s = lock(&state);
        open_panel(&mut s, ScenePanel::Template);
        s.template_menu_focused = true;
        let len = menu_items(&s, ScenePanel::Template).len();
        let p = s.panel.clone().unwrap();
        route_panel(
            &mut s,
            p,
            FiredGesture {
                button: RemoteButton::Up,
                trigger: ButtonTrigger::Single,
            },
        );
        assert_eq!(s.panel.as_ref().unwrap().selected, len - 1);
        let p = s.panel.clone().unwrap();
        route_panel(
            &mut s,
            p,
            FiredGesture {
                button: RemoteButton::Down,
                trigger: ButtonTrigger::Single,
            },
        );
        assert_eq!(s.panel.as_ref().unwrap().selected, 0);
        s.menu_native_held.insert(RemoteButton::Ok);
        cancel_state(&mut s, "exit");
        assert!(s.panel.is_some());
        assert_eq!(s.menu_close_after_release, Some(false));
        s.menu_native_held.clear();
        assert!(finish_template_menu(&mut s));
        assert!(s.manual_selection.is_none());
    }
}
