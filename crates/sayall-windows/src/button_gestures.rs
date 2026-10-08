//! 普通按键手势识别（单击/双击/长按/按住连续触发），语义对齐 Mac 原版
//! `RemoteButtonGestureRecognizer` + `HIDRemoteScheduler`，并按 Windows 产品
//! 口径扩展（2026-10-05）：「按住连续触发」是显式开关（每键至多一个槽位）。
//!
//! - 双击窗口 300ms、长按阈值 550ms、连续触发起始 350ms（逐键间隔：
//!   返回 50ms、方向/音量 100ms，见 `RemoteButton::repeat_interval`）。
//! - 开关取值与行为（`ButtonActions::hold_repeat`）：
//!   - 关闭 → 单击在按下沿立即触发（无双击/长按时）或经双击窗确认；按住不连续；
//!   - 单击 → 按住连续触发单击动作：无双击时 350ms 起连拍；有双击时按住超过
//!     双击窗（300ms）即确认为单击并起拍，快速点按仍走双击判定；
//!   - 长按 → 长按动作在 550ms 触发一次后按间隔继续连续触发。
//! - 连续触发只对「可连续执行」的动作生效（组合键/打开应用/聚焦等只执行一次）。
//! - 互斥：开关开在单击时该键不得配置长按（界面阻止并存；引擎对脏数据按
//!   长按优先降级）。
//! - 语音键不进入本识别器（保持按下开始/释放结束的实时生命周期）。
//!
//! 纯状态机：不持锁、不触 IO，时间由调用方注入，便于单元测试。
//! 计时器不自行调度：引擎线程以 [`GestureRecognizer::next_deadline`] 作为
//! recv_timeout，超时后调用 [`GestureRecognizer::advance`] 处理到期定时器。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::raw_input::RemoteButton;
use crate::send_input::{ButtonAction, ButtonMappings, ButtonTrigger};

/// 双击判定窗口（第二击按下沿之间的最大间隔），Mac 同款。
pub const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(300);
/// 长按阈值（按住多久触发长按），Mac 同款。
pub const LONG_PRESS_THRESHOLD: Duration = Duration::from_millis(550);
/// 连发起始延迟（按住多久开始重复单击），Mac 同款。
pub const REPEAT_START_DELAY: Duration = Duration::from_millis(350);

/// 单个按键的手势配置（由按键映射推导）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GestureConfig {
    /// 单击动作已配置（未配置时单击触发为空操作）。
    pub single_configured: bool,
    /// 双击列已配置 → 单击等待双击窗口。
    pub double_enabled: bool,
    /// 长按列已配置 → 按住 550ms 触发长按。
    pub long_enabled: bool,
    /// 单击保持连续触发（开关=单击、动作可连续、非锁屏延迟；长按为空）。
    pub repeat_single: Option<Duration>,
    /// 长按触发后继续连续触发（开关=长按、长按动作可连续）。
    pub repeat_long: Option<Duration>,
    /// 会切换交互桌面的单击动作必须等本次实体按键完整释放后执行，确保
    /// 原始 DOWN/UP 先由门控成对处理，不把迟到边沿带到锁屏/解锁阶段。
    pub defer_single_until_release: bool,
}

impl GestureConfig {
    /// 从按键映射推导某键的手势配置。未配置任何动作 → None（识别器忽略该键）。
    pub fn for_button(mappings: &ButtonMappings, button: RemoteButton) -> Option<Self> {
        let actions = mappings.actions(button);
        if !mappings.enabled || !actions.any_configured() {
            return None;
        }
        let single_configured = actions.single != ButtonAction::Disabled;
        let double_enabled = actions.double != ButtonAction::Disabled;
        let long_enabled = actions.long != ButtonAction::Disabled;
        let defer_single_until_release = !double_enabled
            && !long_enabled
            && matches!(&actions.single, ButtonAction::Shortcut { chord }
                if chord.is_lock_workstation());
        let interval = button.repeat_interval();
        // 防御降级：手改文件出现「开关=单击 + 长按已配置」时，长按优先、忽略
        // 单击连续（与界面阻止规则一致，不得双触发）。
        let repeat_single = match actions.hold_repeat {
            Some(ButtonTrigger::Single)
                if !long_enabled
                    && !defer_single_until_release
                    && actions.single.allows_repeat() =>
            {
                interval
            }
            _ => None,
        };
        let repeat_long = match actions.hold_repeat {
            Some(ButtonTrigger::Long) if actions.long.allows_repeat() => interval,
            _ => None,
        };
        Some(Self {
            single_configured,
            double_enabled,
            long_enabled,
            repeat_single,
            repeat_long,
            defer_single_until_release,
        })
    }

    /// 原始单击路径：未配置双击/长按时单击在按下沿立即触发（零延迟，
    /// Mac 同款），开关=单击时按住按间隔连续触发（无连发能力的按键不重复）。
    fn raw_path(&self) -> bool {
        self.single_configured
            && !self.double_enabled
            && !self.long_enabled
            && !self.defer_single_until_release
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ButtonGestureState {
    pressed: bool,
    is_second_press: bool,
    waiting_for_second: bool,
    long_fired: bool,
    /// 本次按住已按「超过双击窗」确认为单击（不再补单击、不进双击窗）。
    hold_concluded: bool,
    long_deadline: Option<Instant>,
    double_deadline: Option<Instant>,
    /// 双击共存下的按住确认计时器（按下 + 双击窗）。
    single_conclude_deadline: Option<Instant>,
    /// 单击路径的连续触发计时器。
    repeat_deadline: Option<Instant>,
    /// 长按路径的连续触发计时器。
    long_repeat_deadline: Option<Instant>,
    /// 上下文点击吞掉本次按压：忽略到释放为止，不触发任何手势。
    swallowing: bool,
}

/// 手势识别器：每键独立状态机。所有方法都不会 panic，未配置的按键被忽略。
#[derive(Debug, Default)]
pub struct GestureRecognizer {
    buttons: BTreeMap<RemoteButton, (GestureConfig, ButtonGestureState)>,
}

impl GestureRecognizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// 按当前映射重建配置。配置变化会重置全部手势状态（进行中的双击窗口、
    /// 长按计时与连发一并取消，不触发任何动作）。
    pub fn configure(&mut self, mappings: &ButtonMappings) {
        self.buttons.clear();
        for button in crate::raw_input::ALL_BUTTONS {
            if let Some(config) = GestureConfig::for_button(mappings, button) {
                self.buttons
                    .insert(button, (config, ButtonGestureState::default()));
            }
        }
    }

    pub fn defers_single_until_release(&self, button: RemoteButton) -> bool {
        self.buttons
            .get(&button)
            .is_some_and(|(config, _)| config.defer_single_until_release)
    }

    /// 按下沿：返回立即触发的手势（原始单击路径）。
    pub fn press(&mut self, button: RemoteButton, now: Instant) -> Vec<ButtonTrigger> {
        let Some((config, state)) = self.buttons.get_mut(&button) else {
            return Vec::new();
        };
        if state.swallowing {
            // 上下文点击已接管本次按压（见 [`Self::swallow_press`]）：忽略到释放为止。
            return Vec::new();
        }
        if state.waiting_for_second {
            // 第二击：取消双击窗口，标记第二击并重启长按计时。
            state.waiting_for_second = false;
            state.is_second_press = true;
            state.double_deadline = None;
        }
        state.pressed = true;
        state.hold_concluded = false;
        if config.long_enabled {
            state.long_deadline = Some(now + LONG_PRESS_THRESHOLD);
        }
        if config.raw_path() {
            // 原始单击路径：按下沿立即触发单击；开关=单击时 350ms 起连续触发。
            if config.repeat_single.is_some() {
                state.repeat_deadline = Some(now + REPEAT_START_DELAY);
            }
            return vec![ButtonTrigger::Single];
        }
        if config.double_enabled && config.repeat_single.is_some() {
            // 开关=单击 + 双击：按住超过双击窗即确认为单击（不补发第一击）。
            state.single_conclude_deadline = Some(now + DOUBLE_CLICK_WINDOW);
            state.repeat_deadline = Some(now + REPEAT_START_DELAY);
        }
        Vec::new()
    }

    /// 释放沿：返回立即触发的手势（双击/单击）。
    pub fn release(&mut self, button: RemoteButton, now: Instant) -> Vec<ButtonTrigger> {
        let Some((config, state)) = self.buttons.get_mut(&button) else {
            return Vec::new();
        };
        if state.swallowing {
            // 上下文点击那一趟的释放：静默收尾并解除吞掉标记。
            *state = ButtonGestureState::default();
            return Vec::new();
        }
        state.pressed = false;
        state.long_deadline = None;
        state.repeat_deadline = None;
        state.single_conclude_deadline = None;
        state.long_repeat_deadline = None;
        if state.hold_concluded {
            // 本次按住已确认为单击（可能已连续触发）：静默收尾，不进双击窗。
            state.hold_concluded = false;
            state.is_second_press = false;
            return Vec::new();
        }
        if state.long_fired {
            // 长按已触发：静默收尾。
            state.long_fired = false;
            state.is_second_press = false;
            return Vec::new();
        }
        if state.is_second_press {
            state.is_second_press = false;
            return vec![ButtonTrigger::Double];
        }
        if config.double_enabled {
            state.waiting_for_second = true;
            state.double_deadline = Some(now + DOUBLE_CLICK_WINDOW);
            return Vec::new();
        }
        if config.raw_path() {
            // 原始路径已在按下沿触发，释放只取消连续触发。
            return Vec::new();
        }
        // 手势路径且未配置双击：立即触发单击。
        vec![ButtonTrigger::Single]
    }

    /// 上下文点击吞掉本次按压：清空该键的挂起状态（双击窗、长按、连发计时），
    /// 随后的释放沿静默收尾、不触发任何手势。只影响该键，其他按键不受影响。
    pub fn swallow_press(&mut self, button: RemoteButton) {
        if let Some((_, state)) = self.buttons.get_mut(&button) {
            *state = ButtonGestureState::default();
            state.swallowing = true;
        }
    }

    /// 处理到期定时器（双击窗口超时/按住确认/长按/连续触发），返回触发的手势。
    pub fn advance(&mut self, now: Instant) -> Vec<(RemoteButton, ButtonTrigger)> {
        self.advance_with_ticks(now).0
    }

    /// 同 [`Self::advance`]，另返回**由连续触发计时器产生的拍**（不含双击窗
    /// 超时补发的单击与首次长按）：引擎据此聚合 `map_repeat` 日志，避免把
    /// 一次性手势记成连续拍（2026-10-05 评审发现 1）。
    #[allow(clippy::type_complexity)]
    pub fn advance_with_ticks(
        &mut self,
        now: Instant,
    ) -> (
        Vec<(RemoteButton, ButtonTrigger)>,
        Vec<(RemoteButton, ButtonTrigger)>,
    ) {
        let mut fired = Vec::new();
        let mut ticks = Vec::new();
        for (button, (config, state)) in &mut self.buttons {
            if state
                .double_deadline
                .is_some_and(|deadline| deadline <= now)
            {
                state.double_deadline = None;
                state.waiting_for_second = false;
                fired.push((*button, ButtonTrigger::Single));
            }
            if state
                .single_conclude_deadline
                .is_some_and(|deadline| deadline <= now)
            {
                state.single_conclude_deadline = None;
                if state.pressed && config.repeat_single.is_some() && !state.hold_concluded {
                    // 按住超过双击判定窗：不是双击（也不是快速点按），确认为
                    // 单击并进入连续触发；第二击同样适用（取消未完成的双击）。
                    state.hold_concluded = true;
                    state.is_second_press = false;
                    state.waiting_for_second = false;
                    fired.push((*button, ButtonTrigger::Single));
                }
            }
            if state.long_deadline.is_some_and(|deadline| deadline <= now) {
                state.long_deadline = None;
                if state.pressed {
                    state.long_fired = true;
                    fired.push((*button, ButtonTrigger::Long));
                    if let Some(interval) = config.repeat_long {
                        // 长按触发后直接按该键间隔续拍。
                        state.long_repeat_deadline = Some(now + interval);
                    }
                }
            }
            if let Some(interval) = config.repeat_long {
                if state
                    .long_repeat_deadline
                    .is_some_and(|deadline| deadline <= now)
                {
                    if state.pressed {
                        state.long_repeat_deadline = Some(now + interval);
                        fired.push((*button, ButtonTrigger::Long));
                        ticks.push((*button, ButtonTrigger::Long));
                    } else {
                        state.long_repeat_deadline = None;
                    }
                }
            }
            if let Some(interval) = config.repeat_single {
                if state
                    .repeat_deadline
                    .is_some_and(|deadline| deadline <= now)
                {
                    // 双击共存时，必须先在双击窗结束时确认为单击才允许起拍。
                    let ready = state.pressed && (!config.double_enabled || state.hold_concluded);
                    // 未就绪（按住中但尚未确认）不留过去时刻的截止时间，否则
                    // next_deadline 会让引擎以 0 超时空转（2026-10-05 评审发现 5）。
                    state.repeat_deadline = if ready { Some(now + interval) } else { None };
                    if ready {
                        fired.push((*button, ButtonTrigger::Single));
                        ticks.push((*button, ButtonTrigger::Single));
                    }
                }
            }
        }
        (fired, ticks)
    }

    /// 最近的定时器截止时间（引擎线程的 recv_timeout 依据）。
    pub fn next_deadline(&self) -> Option<Instant> {
        self.buttons
            .values()
            .flat_map(|(_, state)| {
                [
                    state.long_deadline,
                    state.double_deadline,
                    state.single_conclude_deadline,
                    state.repeat_deadline,
                    state.long_repeat_deadline,
                ]
            })
            .flatten()
            .min()
    }

    /// 取消全部手势状态（监听器停止/设备移除/配置变化时调用），不触发动作。
    pub fn release_all(&mut self) {
        for (_, state) in self.buttons.values_mut() {
            *state = ButtonGestureState::default();
        }
    }

    #[allow(dead_code)]
    pub fn is_pressed(&self, button: RemoteButton) -> bool {
        self.buttons
            .get(&button)
            .is_some_and(|(_, state)| state.pressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::send_input::{ButtonAction, ButtonActions, KeyChord, KeyCode};

    fn mappings_with(
        button: RemoteButton,
        single: Option<KeyCode>,
        double: Option<KeyCode>,
        long: Option<KeyCode>,
    ) -> ButtonMappings {
        mappings_with_repeat(button, single, double, long, None)
    }

    fn mappings_with_repeat(
        button: RemoteButton,
        single: Option<KeyCode>,
        double: Option<KeyCode>,
        long: Option<KeyCode>,
        hold_repeat: Option<ButtonTrigger>,
    ) -> ButtonMappings {
        let action = |keys: Option<KeyCode>| match keys {
            Some(key) => ButtonAction::Shortcut {
                chord: KeyChord { keys: vec![key] },
            },
            None => ButtonAction::Disabled,
        };
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            button,
            ButtonActions {
                single: action(single),
                double: action(double),
                long: action(long),
                hold_repeat,
                ok_context_click: false,
            },
        );
        mappings
    }

    #[test]
    fn raw_single_path_fires_on_press_and_repeats_while_held() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with_repeat(
            RemoteButton::Up,
            Some(KeyCode::Up),
            None,
            None,
            Some(ButtonTrigger::Single),
        ));

        // 按下沿立即触发单击。
        assert_eq!(
            recognizer.press(RemoteButton::Up, t0),
            vec![ButtonTrigger::Single]
        );
        // 350ms 内无连续触发。
        assert_eq!(recognizer.advance(t0 + Duration::from_millis(340)), vec![]);
        // 350ms 起始延迟后按 100ms 间隔连续触发。
        let t1 = t0 + REPEAT_START_DELAY;
        assert_eq!(
            recognizer.advance(t1),
            vec![(RemoteButton::Up, ButtonTrigger::Single)]
        );
        assert_eq!(recognizer.advance(t1 + Duration::from_millis(99)), vec![]);
        assert_eq!(
            recognizer.advance(t1 + Duration::from_millis(100)),
            vec![(RemoteButton::Up, ButtonTrigger::Single)]
        );
        // 释放取消连发。
        assert_eq!(
            recognizer.release(RemoteButton::Up, t1 + Duration::from_millis(200)),
            vec![]
        );
        assert_eq!(recognizer.advance(t1 + Duration::from_millis(400)), vec![]);
    }

    #[test]
    fn lock_workstation_single_waits_for_release() {
        let t0 = Instant::now();
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Power,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::LeftWindows, KeyCode::L],
                    },
                },
                double: ButtonAction::Disabled,
                long: ButtonAction::Disabled,
                hold_repeat: None,
                ok_context_click: false,
            },
        );
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings);

        assert!(recognizer.defers_single_until_release(RemoteButton::Power));
        assert!(recognizer.press(RemoteButton::Power, t0).is_empty());
        assert!(recognizer.next_deadline().is_none());
        assert_eq!(
            recognizer.release(RemoteButton::Power, t0 + Duration::from_millis(80)),
            vec![ButtonTrigger::Single]
        );
    }

    #[test]
    fn focus_input_single_fires_once_per_press_and_never_repeats() {
        let t0 = Instant::now();
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Up,
            ButtonActions {
                single: ButtonAction::FocusInput,
                double: ButtonAction::Disabled,
                long: ButtonAction::Disabled,
                hold_repeat: None,
                ok_context_click: false,
            },
        );
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings);

        // 未配置双击/长按：按下沿立即触发一次。
        assert_eq!(
            recognizer.press(RemoteButton::Up, t0),
            vec![ButtonTrigger::Single]
        );
        // 按住不放不得连发（对标 mac allowsRepeat=false）。
        assert_eq!(recognizer.advance(t0 + Duration::from_secs(2)), vec![]);
        assert_eq!(
            recognizer.release(RemoteButton::Up, t0 + Duration::from_secs(2)),
            vec![]
        );
        // 下一次按压才是下一次聚焦。
        assert_eq!(
            recognizer.press(RemoteButton::Up, t0 + Duration::from_secs(3)),
            vec![ButtonTrigger::Single]
        );
    }

    #[test]
    fn focus_input_single_defers_into_the_double_click_window() {
        let t0 = Instant::now();
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Up,
            ButtonActions {
                single: ButtonAction::FocusInput,
                double: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Tab],
                    },
                },
                long: ButtonAction::Disabled,
                hold_repeat: None,
                ok_context_click: false,
            },
        );
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings);

        // 单击要等双击窗口结束才执行，避免「双击的第一击」被当成一次聚焦。
        assert!(recognizer.press(RemoteButton::Up, t0).is_empty());
        let released = t0 + Duration::from_millis(60);
        assert!(recognizer.release(RemoteButton::Up, released).is_empty());
        // 双击窗口从释放沿开始计时。
        assert_eq!(
            recognizer.advance(released + DOUBLE_CLICK_WINDOW),
            vec![(RemoteButton::Up, ButtonTrigger::Single)]
        );
        // 双击窗口已消费：不再重复触发。
        assert_eq!(
            recognizer.advance(released + DOUBLE_CLICK_WINDOW * 2),
            vec![]
        );
    }

    #[test]
    fn ordinary_non_repeating_single_still_fires_on_press() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with(
            RemoteButton::Power,
            Some(KeyCode::Enter),
            None,
            None,
        ));

        assert!(!recognizer.defers_single_until_release(RemoteButton::Power));
        assert_eq!(
            recognizer.press(RemoteButton::Power, t0),
            vec![ButtonTrigger::Single]
        );
        assert!(recognizer.release(RemoteButton::Power, t0).is_empty());
    }

    #[test]
    fn back_repeats_at_50ms_and_non_repeat_buttons_do_not_repeat() {
        let t0 = Instant::now();
        let mut mappings = mappings_with_repeat(
            RemoteButton::Back,
            Some(KeyCode::Escape),
            None,
            None,
            Some(ButtonTrigger::Single),
        );
        // OK 无连发区间：即便手改文件给上开关也不连续（引擎按无区间降级）。
        mappings.actions.insert(
            RemoteButton::Ok,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Enter],
                    },
                },
                hold_repeat: Some(ButtonTrigger::Single),
                ..ButtonActions::default()
            },
        );
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings);

        assert_eq!(
            recognizer.press(RemoteButton::Back, t0),
            vec![ButtonTrigger::Single]
        );
        assert_eq!(
            recognizer.press(RemoteButton::Ok, t0),
            vec![ButtonTrigger::Single]
        );
        // 返回键 50ms 起连续触发；OK 无区间 → 永不重复。
        let t1 = t0 + REPEAT_START_DELAY + Duration::from_millis(500);
        let fired = recognizer.advance(t1);
        assert_eq!(fired, vec![(RemoteButton::Back, ButtonTrigger::Single)]);
    }

    #[test]
    fn long_press_only_config_fires_single_on_release_and_long_at_threshold() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with(
            RemoteButton::Menu,
            Some(KeyCode::Enter),
            None,
            Some(KeyCode::Escape),
        ));

        // 快速按下/释放 → 释放沿立即单击。
        assert_eq!(recognizer.press(RemoteButton::Menu, t0), vec![]);
        assert_eq!(
            recognizer.release(RemoteButton::Menu, t0 + Duration::from_millis(120)),
            vec![ButtonTrigger::Single]
        );

        // 按住 550ms → 长按触发；随后释放静默收尾。
        assert_eq!(
            recognizer.press(RemoteButton::Menu, t0 + Duration::from_secs(1)),
            vec![]
        );
        assert_eq!(
            recognizer.advance(t0 + Duration::from_secs(1) + LONG_PRESS_THRESHOLD),
            vec![(RemoteButton::Menu, ButtonTrigger::Long)]
        );
        assert_eq!(
            recognizer.release(
                RemoteButton::Menu,
                t0 + Duration::from_secs(1) + Duration::from_millis(700)
            ),
            vec![]
        );
    }

    #[test]
    fn double_click_window_defers_single_and_second_release_fires_double() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with(
            RemoteButton::Ok,
            Some(KeyCode::Enter),
            Some(KeyCode::Space),
            None,
        ));

        // 第一击：释放后进入 300ms 双击窗口，不立即单击。
        recognizer.press(RemoteButton::Ok, t0);
        assert_eq!(
            recognizer.release(RemoteButton::Ok, t0 + Duration::from_millis(80)),
            vec![]
        );
        // 窗口内第二击：按下沿无触发（双击未配置长按）……
        let t1 = t0 + Duration::from_millis(80) + Duration::from_millis(120);
        assert_eq!(recognizer.press(RemoteButton::Ok, t1), vec![]);
        // 第二击释放 → 立即双击。
        assert_eq!(
            recognizer.release(RemoteButton::Ok, t1 + Duration::from_millis(60)),
            vec![ButtonTrigger::Double]
        );

        // 另一轮：只有一击 → 窗口超时后补发单击。
        let t2 = t0 + Duration::from_secs(2);
        recognizer.press(RemoteButton::Ok, t2);
        assert_eq!(
            recognizer.release(RemoteButton::Ok, t2 + Duration::from_millis(50)),
            vec![]
        );
        assert_eq!(
            recognizer.advance(t2 + Duration::from_millis(50) + DOUBLE_CLICK_WINDOW),
            vec![(RemoteButton::Ok, ButtonTrigger::Single)]
        );
    }

    #[test]
    fn second_press_hold_can_still_trigger_long_press() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with(
            RemoteButton::Home,
            Some(KeyCode::Enter),
            Some(KeyCode::Space),
            Some(KeyCode::Escape),
        ));

        recognizer.press(RemoteButton::Home, t0);
        recognizer.release(RemoteButton::Home, t0 + Duration::from_millis(60));
        let t1 = t0 + Duration::from_millis(200);
        recognizer.press(RemoteButton::Home, t1);
        // 第二击按住 550ms → 长按（Mac：第二击重启长按计时）。
        assert_eq!(
            recognizer.advance(t1 + LONG_PRESS_THRESHOLD),
            vec![(RemoteButton::Home, ButtonTrigger::Long)]
        );
        assert_eq!(
            recognizer.release(
                RemoteButton::Home,
                t1 + LONG_PRESS_THRESHOLD + Duration::from_millis(100)
            ),
            vec![]
        );
    }

    #[test]
    fn hold_repeat_off_never_repeats_while_held() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with(
            RemoteButton::Up,
            Some(KeyCode::Up),
            None,
            None,
        ));
        assert_eq!(
            recognizer.press(RemoteButton::Up, t0),
            vec![ButtonTrigger::Single]
        );
        assert_eq!(recognizer.advance(t0 + Duration::from_secs(2)), vec![]);
        assert_eq!(
            recognizer.release(RemoteButton::Up, t0 + Duration::from_secs(2)),
            vec![]
        );
    }

    #[test]
    fn single_hold_repeat_with_double_concludes_at_the_window() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with_repeat(
            RemoteButton::Back,
            Some(KeyCode::Delete),
            Some(KeyCode::Space),
            None,
            Some(ButtonTrigger::Single),
        ));

        // 按住：按下沿不触发（要等双击窗）；到 300ms 确认为单击并进入连续。
        assert_eq!(recognizer.press(RemoteButton::Back, t0), vec![]);
        assert_eq!(
            recognizer.advance(t0 + DOUBLE_CLICK_WINDOW),
            vec![(RemoteButton::Back, ButtonTrigger::Single)],
            "按住超过双击窗应确认为单击"
        );
        // 返回键 50ms 间隔：350ms 起连拍。
        assert_eq!(
            recognizer.advance(t0 + REPEAT_START_DELAY),
            vec![(RemoteButton::Back, ButtonTrigger::Single)]
        );
        assert_eq!(
            recognizer.advance(t0 + REPEAT_START_DELAY + Duration::from_millis(49)),
            vec![]
        );
        assert_eq!(
            recognizer.advance(t0 + REPEAT_START_DELAY + Duration::from_millis(50)),
            vec![(RemoteButton::Back, ButtonTrigger::Single)]
        );
        // 释放静默收尾：不补单击、不进双击窗；随后无任何迟到触发。
        assert_eq!(
            recognizer.release(RemoteButton::Back, t0 + Duration::from_millis(500)),
            vec![]
        );
        assert_eq!(recognizer.advance(t0 + Duration::from_secs(2)), vec![]);

        // 快速点按仍走双击判定：窗口超时补发单击。
        let t2 = t0 + Duration::from_secs(3);
        assert!(recognizer.press(RemoteButton::Back, t2).is_empty());
        assert!(recognizer
            .release(RemoteButton::Back, t2 + Duration::from_millis(80))
            .is_empty());
        assert_eq!(
            recognizer.advance(t2 + Duration::from_millis(80) + DOUBLE_CLICK_WINDOW),
            vec![(RemoteButton::Back, ButtonTrigger::Single)]
        );
    }

    #[test]
    fn second_press_hold_beyond_window_cancels_double_and_concludes_single() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with_repeat(
            RemoteButton::Back,
            Some(KeyCode::Delete),
            Some(KeyCode::Space),
            None,
            Some(ButtonTrigger::Single),
        ));

        // 第一击快速点按 → 双击窗；第二击按住超过窗 → 取消双击、确认为单击。
        assert!(recognizer.press(RemoteButton::Back, t0).is_empty());
        assert!(recognizer
            .release(RemoteButton::Back, t0 + Duration::from_millis(80))
            .is_empty());
        let t1 = t0 + Duration::from_millis(150);
        assert!(recognizer.press(RemoteButton::Back, t1).is_empty());
        assert_eq!(
            recognizer.advance(t1 + DOUBLE_CLICK_WINDOW),
            vec![(RemoteButton::Back, ButtonTrigger::Single)],
            "第二击按住超窗同样确认为单击（不补发第一击）"
        );
        assert_eq!(
            recognizer.advance(t1 + REPEAT_START_DELAY),
            vec![(RemoteButton::Back, ButtonTrigger::Single)],
            "确认后按间隔起拍"
        );
        // 释放不产生双击。
        assert_eq!(
            recognizer.release(RemoteButton::Back, t1 + Duration::from_millis(600)),
            vec![]
        );
    }

    #[test]
    fn hold_repeat_long_fires_once_then_repeats_until_release() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with_repeat(
            RemoteButton::Back,
            Some(KeyCode::Delete),
            None,
            Some(KeyCode::Backspace),
            Some(ButtonTrigger::Long),
        ));

        // 快速点按：释放沿触发单击（长按动作不参与点按）。
        assert!(recognizer.press(RemoteButton::Back, t0).is_empty());
        assert_eq!(
            recognizer.release(RemoteButton::Back, t0 + Duration::from_millis(80)),
            vec![ButtonTrigger::Single]
        );

        // 按住：550ms 长按触发，随后按 50ms 间隔继续长按拍。
        let t1 = t0 + Duration::from_secs(1);
        assert!(recognizer.press(RemoteButton::Back, t1).is_empty());
        assert_eq!(
            recognizer.advance(t1 + LONG_PRESS_THRESHOLD),
            vec![(RemoteButton::Back, ButtonTrigger::Long)]
        );
        assert_eq!(
            recognizer.advance(t1 + LONG_PRESS_THRESHOLD + Duration::from_millis(49)),
            vec![]
        );
        assert_eq!(
            recognizer.advance(t1 + LONG_PRESS_THRESHOLD + Duration::from_millis(50)),
            vec![(RemoteButton::Back, ButtonTrigger::Long)]
        );
        assert_eq!(
            recognizer.advance(t1 + LONG_PRESS_THRESHOLD + Duration::from_millis(100)),
            vec![(RemoteButton::Back, ButtonTrigger::Long)]
        );
        // 释放即停、静默收尾。
        assert_eq!(
            recognizer.release(
                RemoteButton::Back,
                t1 + LONG_PRESS_THRESHOLD + Duration::from_millis(140)
            ),
            vec![]
        );
        assert_eq!(recognizer.advance(t1 + Duration::from_secs(3)), vec![]);
    }

    #[test]
    fn hold_repeat_long_requires_a_repeatable_action() {
        let t0 = Instant::now();
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Back,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Delete],
                    },
                },
                long: ButtonAction::OpenApp {
                    target: "notepad".to_owned(),
                },
                hold_repeat: Some(ButtonTrigger::Long),
                ..ButtonActions::default()
            },
        );
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings);
        assert!(recognizer.press(RemoteButton::Back, t0).is_empty());
        assert_eq!(
            recognizer.advance(t0 + LONG_PRESS_THRESHOLD),
            vec![(RemoteButton::Back, ButtonTrigger::Long)]
        );
        assert_eq!(
            recognizer.advance(t0 + Duration::from_secs(2)),
            vec![],
            "不可连续执行的动作不得重复"
        );
    }

    #[test]
    fn long_press_wins_when_hold_repeat_points_at_single_but_long_is_configured() {
        // 手改文件形态（normalized() 会拒绝）：引擎必须长按优先、不得连拍单击。
        let t0 = Instant::now();
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Back,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Delete],
                    },
                },
                long: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Backspace],
                    },
                },
                hold_repeat: Some(ButtonTrigger::Single),
                ..ButtonActions::default()
            },
        );
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings);
        assert!(recognizer.press(RemoteButton::Back, t0).is_empty());
        assert_eq!(
            recognizer.advance(t0 + LONG_PRESS_THRESHOLD),
            vec![(RemoteButton::Back, ButtonTrigger::Long)]
        );
        assert_eq!(
            recognizer.advance(t0 + Duration::from_millis(700)),
            vec![],
            "防御降级：单击连续被忽略（长按动作本身未开启连续）"
        );
    }

    #[test]
    fn non_repeatable_single_action_ignores_hand_edited_hold_repeat() {
        // 聚焦输入框不可连续执行（normalized() 会拒绝该组合）：引擎降级为不重复。
        let t0 = Instant::now();
        let mut mappings = ButtonMappings::default();
        mappings.actions.insert(
            RemoteButton::Up,
            ButtonActions {
                single: ButtonAction::FocusInput,
                hold_repeat: Some(ButtonTrigger::Single),
                ..ButtonActions::default()
            },
        );
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings);
        assert_eq!(
            recognizer.press(RemoteButton::Up, t0),
            vec![ButtonTrigger::Single]
        );
        assert_eq!(recognizer.advance(t0 + Duration::from_secs(2)), vec![]);
    }

    #[test]
    fn release_all_cancels_pending_windows_and_repeat_without_firing() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with_repeat(
            RemoteButton::Back,
            Some(KeyCode::Escape),
            Some(KeyCode::Space),
            None,
            Some(ButtonTrigger::Single),
        ));
        // 双击窗口待触发时复位：不得补发单击。
        recognizer.press(RemoteButton::Back, t0);
        recognizer.release(RemoteButton::Back, t0 + Duration::from_millis(40));
        recognizer.release_all();
        assert_eq!(
            recognizer.advance(t0 + Duration::from_secs(2)),
            vec![],
            "释放全部状态后双击窗口不应再触发单击"
        );

        // 按住连续触发进行中复位：所有计时取消，不得补拍。
        let t1 = t0 + Duration::from_secs(3);
        assert!(recognizer.press(RemoteButton::Back, t1).is_empty());
        assert_eq!(
            recognizer.advance(t1 + DOUBLE_CLICK_WINDOW),
            vec![(RemoteButton::Back, ButtonTrigger::Single)],
            "按住确认单击"
        );
        recognizer.release_all();
        assert_eq!(
            recognizer.advance(t1 + Duration::from_secs(5)),
            vec![],
            "复位后不得补拍"
        );
    }

    #[test]
    fn unconfigured_and_disabled_buttons_are_ignored() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&ButtonMappings::default());
        assert_eq!(recognizer.press(RemoteButton::Ok, t0), vec![]);
        assert_eq!(recognizer.release(RemoteButton::Ok, t0), vec![]);
        assert_eq!(recognizer.advance(t0 + Duration::from_secs(5)), vec![]);
        assert_eq!(recognizer.next_deadline(), None);

        // 只有禁用动作的按键同样被忽略。
        let mut mappings = ButtonMappings::default();
        mappings
            .actions
            .insert(RemoteButton::Ok, ButtonActions::default());
        recognizer.configure(&mappings);
        assert_eq!(recognizer.press(RemoteButton::Ok, t0), vec![]);
    }

    #[test]
    fn enabled_toggle_off_disables_everything() {
        let t0 = Instant::now();
        let mut mappings = mappings_with(RemoteButton::Up, Some(KeyCode::Up), None, None);
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings);
        mappings.enabled = false;
        recognizer.configure(&mappings);
        assert_eq!(recognizer.press(RemoteButton::Up, t0), vec![]);
        assert_eq!(recognizer.release(RemoteButton::Up, t0), vec![]);
    }

    #[test]
    fn next_deadline_is_the_earliest_timer() {
        let t0 = Instant::now();
        let mut mappings = mappings_with_repeat(
            RemoteButton::Up,
            Some(KeyCode::Up),
            None,
            None,
            Some(ButtonTrigger::Single),
        );
        mappings.actions.insert(
            RemoteButton::Down,
            ButtonActions {
                single: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Down],
                    },
                },
                double: ButtonAction::Shortcut {
                    chord: KeyChord {
                        keys: vec![KeyCode::Space],
                    },
                },
                long: ButtonAction::Disabled,
                hold_repeat: None,
                ok_context_click: false,
            },
        );
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings);
        recognizer.press(RemoteButton::Up, t0);
        recognizer.press(RemoteButton::Down, t0);
        recognizer.release(RemoteButton::Down, t0 + Duration::from_millis(30));
        // Up 连发起始 = t0+350ms；Down 双击窗口 = t0+30+300ms = t0+330ms（更早）。
        assert_eq!(
            recognizer.next_deadline(),
            Some(t0 + Duration::from_millis(330))
        );

        // 双击共存 + 开关=单击：按住确认计时器（双击窗）必须参与 next_deadline，
        // 否则按住确认的单击会迟到（2026-10-05 评审发现 6）。
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with_repeat(
            RemoteButton::Back,
            Some(KeyCode::Delete),
            Some(KeyCode::Space),
            None,
            Some(ButtonTrigger::Single),
        ));
        recognizer.press(RemoteButton::Back, t0);
        assert_eq!(recognizer.next_deadline(), Some(t0 + DOUBLE_CLICK_WINDOW));
    }

    #[test]
    fn advance_ticks_cover_only_repeat_timers() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with_repeat(
            RemoteButton::Back,
            Some(KeyCode::Delete),
            Some(KeyCode::Space),
            None,
            Some(ButtonTrigger::Single),
        ));

        // 快速点按：双击窗超时补发的单击不是连续拍（2026-10-05 评审发现 1）。
        recognizer.press(RemoteButton::Back, t0);
        assert!(recognizer
            .release(RemoteButton::Back, t0 + Duration::from_millis(60))
            .is_empty());
        let (fired, ticks) = recognizer.advance_with_ticks(t0 + Duration::from_millis(360));
        assert_eq!(fired, vec![(RemoteButton::Back, ButtonTrigger::Single)]);
        assert!(ticks.is_empty(), "双击窗补发的单击不得记成连续拍");

        // 按住：300ms 确认为单击（不是拍）、350ms 起拍（是拍）。
        let t1 = t0 + Duration::from_secs(2);
        recognizer.press(RemoteButton::Back, t1);
        let (fired, ticks) = recognizer.advance_with_ticks(t1 + DOUBLE_CLICK_WINDOW);
        assert_eq!(fired, vec![(RemoteButton::Back, ButtonTrigger::Single)]);
        assert!(ticks.is_empty(), "按住确认的首次单击不算连续拍");
        let (fired, ticks) = recognizer.advance_with_ticks(t1 + REPEAT_START_DELAY);
        assert_eq!(fired, vec![(RemoteButton::Back, ButtonTrigger::Single)]);
        assert_eq!(ticks, vec![(RemoteButton::Back, ButtonTrigger::Single)]);
    }

    #[test]
    fn advance_ticks_cover_only_repeat_timers_for_long() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with_repeat(
            RemoteButton::Back,
            None,
            None,
            Some(KeyCode::Backspace),
            Some(ButtonTrigger::Long),
        ));

        // 首次长按是手势本身，不是连续拍；其后按间隔续拍才是。
        recognizer.press(RemoteButton::Back, t0);
        let (fired, ticks) = recognizer.advance_with_ticks(t0 + LONG_PRESS_THRESHOLD);
        assert_eq!(fired, vec![(RemoteButton::Back, ButtonTrigger::Long)]);
        assert!(ticks.is_empty(), "首次长按不得记成连续拍");
        let beat = t0 + LONG_PRESS_THRESHOLD + Duration::from_millis(50);
        let (fired, ticks) = recognizer.advance_with_ticks(beat);
        assert_eq!(fired, vec![(RemoteButton::Back, ButtonTrigger::Long)]);
        assert_eq!(ticks, vec![(RemoteButton::Back, ButtonTrigger::Long)]);
    }

    #[test]
    fn swallow_press_clears_pending_state_and_stays_silent() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with(
            RemoteButton::Ok,
            Some(KeyCode::Enter),
            Some(KeyCode::Space),
            Some(KeyCode::Escape),
        ));

        // 先制造一个挂起的双击窗：轻点一次、松开。
        recognizer.press(RemoteButton::Ok, t0);
        assert!(recognizer
            .release(RemoteButton::Ok, t0 + Duration::from_millis(60))
            .is_empty());

        // 上下文点击吞掉下一次按压：挂起状态被清空，且不产生迟到手势。
        recognizer.swallow_press(RemoteButton::Ok);
        assert!(recognizer
            .press(RemoteButton::Ok, t0 + Duration::from_millis(200))
            .is_empty());
        assert!(recognizer
            .release(RemoteButton::Ok, t0 + Duration::from_millis(260))
            .is_empty());
        assert!(
            recognizer.advance(t0 + Duration::from_secs(1)).is_empty(),
            "被吞掉的按压不得留下迟到的单击/双击"
        );

        // 吞掉标记随释放解除：下一次正常按压回到既有语义（双击窗后补单击）。
        let t1 = t0 + Duration::from_secs(2);
        recognizer.press(RemoteButton::Ok, t1);
        assert!(recognizer
            .release(RemoteButton::Ok, t1 + Duration::from_millis(50))
            .is_empty());
        assert_eq!(
            recognizer.advance(t1 + Duration::from_millis(50) + DOUBLE_CLICK_WINDOW),
            vec![(RemoteButton::Ok, ButtonTrigger::Single)]
        );
    }

    #[test]
    fn swallow_press_suppresses_long_press_until_release() {
        let t0 = Instant::now();
        let mut recognizer = GestureRecognizer::new();
        recognizer.configure(&mappings_with(
            RemoteButton::Ok,
            Some(KeyCode::Enter),
            None,
            Some(KeyCode::Escape),
        ));

        recognizer.swallow_press(RemoteButton::Ok);
        assert!(recognizer.press(RemoteButton::Ok, t0).is_empty());
        assert!(
            recognizer
                .advance(t0 + LONG_PRESS_THRESHOLD + Duration::from_millis(10))
                .is_empty(),
            "被吞掉的按压不得触发长按"
        );
        assert!(recognizer
            .release(RemoteButton::Ok, t0 + Duration::from_millis(800))
            .is_empty());

        // 释放后该键可正常使用（长按阈值仍是唯一条件）。
        let t1 = t0 + Duration::from_secs(2);
        recognizer.press(RemoteButton::Ok, t1);
        assert_eq!(
            recognizer.advance(t1 + LONG_PRESS_THRESHOLD),
            vec![(RemoteButton::Ok, ButtonTrigger::Long)]
        );
    }
}
