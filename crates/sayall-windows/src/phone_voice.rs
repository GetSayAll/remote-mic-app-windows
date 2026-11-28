//! 手机遥控的语音会话：把「按下 / 释放」做成平台层可复用的两个入口。
//!
//! 为什么不是让手机遥控自己写一遍：遥控器语音键那套顺序是**实测调出来的**，抄一份就会
//! 有两份实现跟着微信输入法的时序各自漂移。所以这里复用的是同一组动作，只把与遥控器
//! 绑定、手机不需要的步骤（F5 抑制器武装、报告层合成门禁、微信恢复阶梯）留给 BLE 侧。
//!
//! 保留的步骤，每一步都有具体原因：
//!
//! 1. **输入法激活**（`ensure_session_ime`）：语音热键只在目标输入法是当前会话活动输入法
//!    时才生效（2026-09-05 持锁实验）。目标按用户选的工具决定，不按和弦猜。
//! 2. **按下注入**：`press` / `release` 严格配对。失败必须回报，不能假装成功。
//! 3. **音频会话**：`begin_session` / `finish_session` 用同一个代次；代次不配对会让收尾
//!    落到上一段上。
//!
//! 刻意**不做**的（手机遥控与遥控器的差别）：
//!
//! - 不武装 F5 抑制器、不解粘 F5：手机不会像遥控器那样额外上报一个 F5。
//! - 不走报告层合成：那条路依赖遥控器设备在 `WUDFHost` 里被改写，手机没有对应设备。
//! - 不启动微信恢复阶梯：那是为「遥控器按键被微信拒收」设计的，手机走的是注入键，
//!   与遥控器同一条路，但阶梯的触发条件是遥控器的帧证据。
//!
//! 日志：沿用 `voice_attempt` / `chord_press` / `audio_begin` 的字段名，并加
//! `source=phone` 区分来源——一次拉日志就能看出是哪条链路出的问题。

use std::sync::{Arc, Mutex};

use sayall_core::settings::VoiceInputTool;

use crate::audio::AudioRuntime;
use crate::ble::gatt_note;
use crate::ime;
use crate::ime::tool_label;
use crate::send_input::KeyChord;
use crate::send_input_windows::SendInputRuntime;

/// 一次语音会话里已经取得的资源。释放时必须成对，所以由调用方持有。
#[derive(Debug, Default)]
pub struct PhoneVoiceSession {
    /// 已按下的触发方式；`None` 表示没有按下（例如用户没配快捷键）。
    held: Option<HeldTrigger>,
    /// 本次会话使用的虚拟 HID 触发器（若走设备层）。释放时成对调用。
    virtual_hid: Option<Arc<dyn VirtualHidTrigger>>,
    /// 音频代次；`None` 表示音频会话没开起来。
    audio_generation: Option<u64>,
    /// 这一段语音的编号，由调用方给（手机遥控用语音启动的请求编号）。
    session_id: u64,
}

/// 语音键的按下方式。释放时必须按同一种方式成对放开。
#[derive(Debug)]
enum HeldTrigger {
    /// 通过 `SendInput` 注入的「按住说话」和弦。
    Chord(KeyChord),
    /// 通过设备层虚拟 HID 发出的右 Alt。
    VirtualHid,
}

impl PhoneVoiceSession {
    /// 是否已经按下了触发键（收尾时据此决定要不要释放）。
    pub fn is_holding(&self) -> bool {
        self.held.is_some()
    }

    /// 是否已经开了音频会话。
    pub fn has_audio(&self) -> bool {
        self.audio_generation.is_some()
    }

    /// 音频代次。没有在途音频会话时为 None，此时不该再往渲染端塞样本。
    pub fn audio_generation(&self) -> Option<u64> {
        self.audio_generation
    }

    /// 这一段语音的编号（日志与音频代次共用）。
    pub fn session_id(&self) -> u64 {
        self.session_id
    }
}

/// 语音会话的起停失败。只带稳定码与一句可读说明，文案由上层决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneVoiceError {
    pub code: &'static str,
    pub detail: String,
}

impl PhoneVoiceError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

pub mod voice_code {
    /// 注入「按住说话」快捷键失败。
    pub const INJECTION_FAILED: &str = "voice_injection_failed";
    /// 音频会话没开起来。
    pub const AUDIO_UNAVAILABLE: &str = "voice_output_unavailable";
}

/// 设备层虚拟 HID 触发器（右 Alt）。
///
/// 豆包输入法会丢弃带 `LLKHF_INJECTED` 的按键（见
/// `Bugs/2026-09-04-doubao-voice-hold-hotkey.md`），只能由可选 Helper 通过
/// 虚拟 HID 设备发出真实报告。未安装/未启用时没有实现；此时豆包路径必须
/// 返回明确错误，不能退回到注定无效的 `SendInput` 并假装成功。
pub trait VirtualHidTrigger: Send + Sync + std::fmt::Debug {
    /// 按下右 Alt（E0 38）。保持时长由调用方控制。
    fn press(&self) -> Result<(), String>;
    /// 释放右 Alt，必须与 [`Self::press`] 成对。
    fn release(&self) -> Result<(), String>;
}

/// 按下：切输入法 → 注入和弦 → 开音频会话。
///
/// 任何一步失败都会**先把已取得的资源释放掉**再返回错误，调用方不需要做补偿。
pub fn press(
    send_input: &SendInputRuntime,
    audio: &AudioRuntime,
    hotkey: Option<KeyChord>,
    tool: Option<VoiceInputTool>,
    virtual_hid: Option<Arc<dyn VirtualHidTrigger>>,
    session_id: u64,
    generation: u64,
) -> Result<PhoneVoiceSession, PhoneVoiceError> {
    let mut session = PhoneVoiceSession {
        session_id,
        ..PhoneVoiceSession::default()
    };

    // 0) 豆包只接受设备层按键。没有虚拟 HID 时立即失败：不能让调用方以为
    //    「已按下」，也不能退回 SendInput 再静默丢弃。
    let needs_virtual_hid = matches!(tool, Some(VoiceInputTool::Doubao));
    if needs_virtual_hid && virtual_hid.is_none() {
        gatt_note(format!(
            "chord_press result=skipped source=phone session={session_id} reason=virtual_hid_unavailable tool={}",
            tool_label(tool),
        ));
        return Err(PhoneVoiceError::new(
            voice_code::INJECTION_FAILED,
            "豆包会丢弃模拟按键。请改用微信输入法，或打开“支持更多输入工具”并安装增强组件。",
        ));
    }

    // 1) 输入法激活。失败只记录，不阻断：不比现状更差（与遥控器路径同一取舍）。
    if let Some(tool) = tool {
        match ime::ensure_session_ime(tool, ime::ImeSwitchScope::VoicePress) {
            Ok(_) => {}
            Err(error) => {
                gatt_note(format!(
                    "voice_attempt source=phone session={session_id} tool={} ime_switch=failed detail={error}",
                    tool_label(Some(tool)),
                ));
            }
        }
    }

    // 2) 触发语音键。豆包走设备层虚拟 HID（右 Alt）；其余工具沿用 SendInput 和弦。
    if needs_virtual_hid {
        let trigger = virtual_hid.expect("no virtual HID checked above");
        press_virtual_hid(trigger.as_ref(), session_id)?;
        session.held = Some(HeldTrigger::VirtualHid);
        session.virtual_hid = Some(trigger);
    } else {
        match hotkey {
            Some(chord) => {
                if let Err(error) = send_input.press(&chord) {
                    gatt_note(format!(
                        "chord_press result=err source=phone session={session_id} error_domain=send_input error_code=press_failed reason=injection_failed retryable=true"
                    ));
                    return Err(PhoneVoiceError::new(
                        voice_code::INJECTION_FAILED,
                        format!("按住说话快捷键注入失败：{error}"),
                    ));
                }
                gatt_note(format!(
                    "chord_press result=ok source=phone session={session_id} gap_ms={}",
                    crate::send_input::HOLD_CHORD_EVENT_GAP.as_millis(),
                ));
                session.held = Some(HeldTrigger::Chord(chord));
            }
            None => {
                gatt_note(format!(
                    "chord_press result=skipped source=phone session={session_id} reason=no_hotkey"
                ));
            }
        }
    }

    // 3) 音频会话。失败时先放开和弦，否则会留下一个按住的键。
    if let Err(error) = audio.begin_session(generation) {
        gatt_note(format!(
            "audio_begin result=err source=phone session={session_id} error_domain=audio error_code=begin_failed reason=wasapi_rejected retryable=true"
        ));
        release(send_input, audio, &mut session);
        return Err(PhoneVoiceError::new(
            voice_code::AUDIO_UNAVAILABLE,
            error.to_string(),
        ));
    }
    session.audio_generation = Some(generation);
    Ok(session)
}

/// 设备层按下右 Alt。失败返回稳定错误码，文案由上层决定。
fn press_virtual_hid(
    trigger: &dyn VirtualHidTrigger,
    session_id: u64,
) -> Result<(), PhoneVoiceError> {
    if let Err(error) = trigger.press() {
        gatt_note(format!(
            "chord_press result=err source=phone session={session_id} trigger=virtual_hid error_domain=virtual_hid error_code=press_failed reason=injection_failed retryable=true"
        ));
        return Err(PhoneVoiceError::new(
            voice_code::INJECTION_FAILED,
            format!("虚拟 HID 按下失败：{error}"),
        ));
    }
    gatt_note(format!(
        "chord_press result=ok source=phone session={session_id} trigger=virtual_hid"
    ));
    Ok(())
}

/// 释放：先放开和弦，再收尾音频。两步各自独立，任一步失败都不影响另一步。
///
/// 顺序是有意的——**先放开按键**：按键按住的后果（AI 应用一直在听）比音频多收一会儿
/// 更糟，而且前者用户立刻能感觉到。
pub fn release(
    send_input: &SendInputRuntime,
    audio: &AudioRuntime,
    session: &mut PhoneVoiceSession,
) {
    match session.held.take() {
        Some(HeldTrigger::Chord(chord)) => {
            if let Err(error) = send_input.release(&chord) {
                gatt_note(format!(
                    "chord_release result=err source=phone error_domain=send_input error_code=release_failed retryable=true detail={error}"
                ));
            }
        }
        Some(HeldTrigger::VirtualHid) => {
            if let Some(trigger) = session.virtual_hid.take() {
                if let Err(error) = trigger.release() {
                    gatt_note(format!(
                        "chord_release result=err source=phone trigger=virtual_hid error_domain=virtual_hid error_code=release_failed retryable=true detail={error}"
                    ));
                }
            }
        }
        None => {
            let _ = session.virtual_hid.take();
        }
    }
    if let Some(generation) = session.audio_generation.take() {
        if let Err(error) = audio.finish_session(generation) {
            gatt_note(format!(
                "audio_finish result=err source=phone error_domain=audio error_code=finish_failed retryable=true detail={error}"
            ));
        }
    }
}

/// 立即中断：与 [`release`] 的区别是音频直接掐断，不放尾音。会话出错或退出时用。
pub fn interrupt(
    send_input: &SendInputRuntime,
    audio: &AudioRuntime,
    session: &mut PhoneVoiceSession,
) {
    match session.held.take() {
        Some(HeldTrigger::Chord(chord)) => {
            if let Err(error) = send_input.release(&chord) {
                gatt_note(format!(
                    "chord_release result=err source=phone reason=interrupt error_domain=send_input error_code=release_failed retryable=true detail={error}"
                ));
            }
        }
        Some(HeldTrigger::VirtualHid) => {
            if let Some(trigger) = session.virtual_hid.take() {
                if let Err(error) = trigger.release() {
                    gatt_note(format!(
                        "chord_release result=err source=phone reason=interrupt trigger=virtual_hid error_domain=virtual_hid error_code=release_failed retryable=true detail={error}"
                    ));
                }
            }
        }
        None => {
            let _ = session.virtual_hid.take();
        }
    }
    if session.audio_generation.take().is_some() {
        let _ = audio.interrupt_session();
    }
}

/// 平台层交给手机遥控的一组句柄。
///
/// 五个都是 `WindowsPlatform` 已经持有的同一份实例——不是拷贝。快捷键与输入工具
/// 特意共享 `Mutex` 句柄：用户中途改设置，下一次按下就取到新值，不需要重连手机
/// （此前这里存的是构造时的快照，导致切换输入工具后手机仍按旧工具注入）。
///
/// `virtual_hid` 由可选 Helper 提供；未安装时为 `None`，豆包路径会明确失败。
#[derive(Clone)]
pub struct PhoneVoiceAccess {
    pub send_input: Arc<SendInputRuntime>,
    pub audio: Arc<AudioRuntime>,
    pub hotkey: Arc<Mutex<Option<KeyChord>>>,
    pub tool: Arc<Mutex<Option<VoiceInputTool>>>,
    pub virtual_hid: Option<Arc<dyn VirtualHidTrigger>>,
}

impl PhoneVoiceAccess {
    /// 按下那一刻的「按住说话快捷键」。
    pub fn hotkey_now(&self) -> Option<KeyChord> {
        lock(&self.hotkey).clone()
    }

    /// 按下那一刻的输入工具。
    pub fn tool_now(&self) -> Option<VoiceInputTool> {
        *lock(&self.tool)
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl std::fmt::Debug for PhoneVoiceAccess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PhoneVoiceAccess")
            .field("hotkey_configured", &self.hotkey_now().is_some())
            .field("tool_configured", &self.tool_now().is_some())
            .field("virtual_hid_available", &self.virtual_hid.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::send_input::KeyCode;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Default)]
    struct FakeVirtualHid {
        presses: AtomicUsize,
        releases: AtomicUsize,
    }

    impl VirtualHidTrigger for FakeVirtualHid {
        fn press(&self) -> Result<(), String> {
            self.presses.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn release(&self) -> Result<(), String> {
            self.releases.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// 回归：手机遥控拿到的是共享句柄，不是构造时的快照。用户切换输入工具后，
    /// 下一次按下必须按新工具注入（2026-10-20 现场：切到微信输入法后手机仍按豆包注入）。
    #[test]
    fn access_reflects_settings_changed_after_construction() {
        let hotkey = Arc::new(Mutex::new(None));
        let tool = Arc::new(Mutex::new(Some(VoiceInputTool::Doubao)));
        let access = PhoneVoiceAccess {
            send_input: Arc::new(SendInputRuntime::new()),
            audio: Arc::new(AudioRuntime::new()),
            hotkey: Arc::clone(&hotkey),
            tool: Arc::clone(&tool),
            virtual_hid: None,
        };

        assert_eq!(access.tool_now(), Some(VoiceInputTool::Doubao));
        assert_eq!(access.hotkey_now(), None);

        *lock(&tool) = Some(VoiceInputTool::Wechat);
        *lock(&hotkey) = Some(KeyChord {
            keys: vec![KeyCode::LeftControl, KeyCode::LeftWindows],
        });

        assert_eq!(access.tool_now(), Some(VoiceInputTool::Wechat));
        assert_eq!(
            access.hotkey_now(),
            Some(KeyChord {
                keys: vec![KeyCode::LeftControl, KeyCode::LeftWindows],
            })
        );
    }

    /// 豆包没有虚拟 HID 时必须在按下阶段失败，不能注入一个注定被丢弃的键、
    /// 更不能让手机收到「voice_start ok」。
    #[test]
    fn doubao_without_virtual_hid_fails_before_injection() {
        let send_input = SendInputRuntime::new();
        let audio = AudioRuntime::new();
        let error = press(
            &send_input,
            &audio,
            Some(KeyChord {
                keys: vec![KeyCode::RightAlt],
            }),
            Some(VoiceInputTool::Doubao),
            None,
            1,
            1,
        )
        .expect_err("豆包缺少虚拟 HID 时必须返回错误");

        assert_eq!(error.code, voice_code::INJECTION_FAILED);
        assert!(error.detail.contains("豆包"));
    }

    /// 有虚拟 HID 时按下走设备层，释放必须成对。
    #[test]
    fn virtual_hid_press_and_release_are_paired() {
        let send_input = SendInputRuntime::new();
        let audio = AudioRuntime::new();
        let hid = Arc::new(FakeVirtualHid::default());

        press_virtual_hid(hid.as_ref(), 1).expect("虚拟 HID 按下应成功");
        assert_eq!(hid.presses.load(Ordering::SeqCst), 1);

        let mut session = PhoneVoiceSession {
            held: Some(HeldTrigger::VirtualHid),
            virtual_hid: Some(hid.clone()),
            ..PhoneVoiceSession::default()
        };
        assert!(session.is_holding());
        release(&send_input, &audio, &mut session);
        assert_eq!(hid.releases.load(Ordering::SeqCst), 1);
        assert!(!session.is_holding());
    }
}
