//! 微信输入法（WeType）会话级激活。
//!
//! 背景（2026-09-05 持锁实验实锤，Testing\investigation\p-ime-experiment.ps1
//! 与 examples\ime_probe_mta.rs，判据 = ConsentStore 开麦时间戳）：
//! - WeType 的语音热键（Ctrl+Win）**只有当 WeType 是当前会话的活动输入法时
//!   才生效**：会话切到微软拼音时注入和弦不开麦，切回 WeType 后恢复触发。
//! - Windows 按应用记忆输入法——用户在其他应用用过别的输入法后，这些应用
//!   的会话里 WeType 不活跃，语音键表现为"无法唤起"（与输入框聚焦无关：
//!   桌面/资源管理器聚焦 6/6 照常触发，p-focus-experiment.ps1）。
//! - **COM 套间陷阱（2026-09-05 MTA 探针实锤，examples\ime_probe_mta.rs）**：
//!   `ActivateProfile` 从 MTA 线程调用返回 S_OK 但**不生效**；必须从 STA
//!   线程调用才真正切换。早期实验在 PowerShell（STA）里验证通过，部署后
//!   应用从 BLE 工作线程（MTA）调用——修复形同虚设。
//! - **冷切换重绑延迟（2026-09-05 真机日志实锤，kb-live.log 10:31:40 会话）**：
//!   会话从未激活过 WeType 时，`ActivateProfile` 返回后目标应用的输入法
//!   会话重绑是异步的——紧跟的和弦落在旧会话（LWin 穿透、无 0xFC、微信
//!   无反应）；第二次起会话已是 WeType，立即触发。表现为"首次按失败、
//!   第二次起正常"。修复：先用 `GetActiveProfile` 判定当前会话状态——
//!   已是 WeType（热路径）零延迟注入；需要切换（冷路径）才激活并等待
//!   重绑窗口后再返回。
//!
//! 方案（参考 macOS 版 PreferredInputSourceMonitor 的"保证语音工具是活动
//! 输入源"职责设计）：注入和弦前，在临时 STA 线程上用公开 TSF API
//! （ITfInputProcessorProfileMgr + TF_IPPMF_FORSESSION 会话级标志——只影响
//! 当前应用会话，不改其他应用的输入法记忆）确保 WeType 为当前会话输入法。
//!
//! 护栏：激活失败或超时只记录提示并按原行为注入（绝不比现状更差）；幂等
//! ——WeType 已活跃时重复激活无害；STA 线程一次性的（创建→调用→退出，
//! 全部同线程内完成，无跨套间封送、无需消息泵），通信走 channel 有界
//! 等待（500ms）防卡 BLE 工作线程；仅在配置了按住说话快捷键的语音会话上
//! 调用。

use std::sync::mpsc;
use std::time::Duration;

use windows::core::GUID;
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, TF_IPPMF_FORSESSION, TF_PROFILETYPE_INPUTPROCESSOR,
    TF_INPUTPROCESSORPROFILE, GUID_TFCAT_TIP_KEYBOARD,
};

/// TF_InputProcessorProfiles（msctf.dll，公开 COM 类）。
const CLSID_TF_INPUT_PROCESSOR_PROFILES: GUID =
    GUID::from_u128(0x33c53a50_f456_4884_b049_85fd643e_cfed);

/// 微信输入法（WeType）的 TSF CLSID 与 Profile GUID（公开稳定标识；
/// 来自本机输入法列表 `0804:{86598FB9-…}{607FDF85-…}`，2.1.3.18 实测）。
const WETYPE_CLSID: GUID = GUID::from_u128(0x86598fb9_66a2_463e_b9c2_aeb906d477ad);
const WETYPE_PROFILE: GUID = GUID::from_u128(0x607fdf85_fcc8_4dbd_a365_41296f980c9c);
const LANGID_ZH_CN: u16 = 0x0804;
/// STA 激活线程的有界等待：正常 <10ms，500ms 只是防卡上限。
const ACTIVATION_JOIN_TIMEOUT: Duration = Duration::from_millis(500);
/// 冷切换后的会话重绑等待：`ActivateProfile` 返回 ≠ 目标应用完成输入法
/// 重绑（2026-09-05 首按失败实证）；热路径不付此代价。
const SESSION_REBIND_SETTLE: Duration = Duration::from_millis(50);

/// 激活结果：AlreadyActive = 会话已是 WeType（调用方可零延迟注入）；
/// Switched = 本次执行了会话切换（含重绑等待）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeTypeActivation {
    AlreadyActive,
    Switched,
}

/// 确保微信输入法是当前会话的活动输入法（幂等）。
///
/// 必须在 STA 线程上执行（MTA 调用返回 S_OK 但不生效，见模块注释）；
/// 本函数自行创建临时 STA 线程并经 channel 有界等待结果，对调用方
/// （BLE 工作线程，MTA）透明。返回 Err 时调用方记录提示后仍按原行为
/// 注入——激活失败不阻断语音。
pub fn activate_wetype_session() -> Result<WeTypeActivation, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("sayall-ime-activate".to_owned())
        .spawn(move || {
            let outcome = sta_ensure_wetype();
            let _ = sender.send(outcome);
        })
        .map_err(|error| format!("创建激活线程失败：{error}"))?;
    // 有界等待，不 join：超时说明激活异常缓慢，按失败处理继续注入；
    // 线程在后台自然结束（若激活迟到，惠及下一次按键）。
    match receiver.recv_timeout(ACTIVATION_JOIN_TIMEOUT) {
        Ok(result) => result,
        Err(_) => Err("激活微信输入法超时（500ms）".to_owned()),
    }
}

/// 临时 STA 线程体：CoInitializeEx(STA) → 查询活动输入法 →
/// （需要时）ActivateProfile + 重绑等待 → CoUninitialize。
/// 全部调用在本线程内完成（无跨套间封送，无需消息泵）。
fn sta_ensure_wetype() -> Result<WeTypeActivation, String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // S_OK(0) 或 S_FALSE(1)（本线程已初始化）都算成功。
        if hr.is_err() && hr != windows::core::HRESULT(1) {
            return Err(format!("CoInitializeEx(STA) 失败：{hr:?}"));
        }
        let result = (|| {
            let manager: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_INPUT_PROCESSOR_PROFILES, None, CLSCTX_INPROC_SERVER)
                    .map_err(|error| format!("创建 TSF 配置管理器失败：{error}"))?;
            if session_has_wetype(&manager)? {
                return Ok(WeTypeActivation::AlreadyActive);
            }
            manager
                .ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    &WETYPE_CLSID,
                    &WETYPE_PROFILE,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                )
                .map_err(|error| format!("激活微信输入法会话失败：{error}"))?;
            // 冷切换：等目标应用完成输入法会话重绑再放行注入。
            std::thread::sleep(SESSION_REBIND_SETTLE);
            Ok(WeTypeActivation::Switched)
        })();
        CoUninitialize();
        result
    }
}

/// 当前会话的活动输入法是否已是 WeType（查询失败按"不是"处理，
/// 走激活路径——宁可多激活一次也不误判热路径）。
fn session_has_wetype(manager: &ITfInputProcessorProfileMgr) -> Result<bool, String> {
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    let active = unsafe { manager.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile) };
    match active {
        Ok(()) => Ok(profile.clsid == WETYPE_CLSID && profile.guidProfile == WETYPE_PROFILE),
        Err(_) => Ok(false),
    }
}
