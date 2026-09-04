//! 微信输入法（WeType）会话级激活。
//!
//! 背景（2026-09-05 持锁实验实锤，Testing\investigation\p-ime-experiment.ps1
//! 与 p-ime-zerodelay.ps1，判据 = ConsentStore 开麦时间戳）：
//! - WeType 的语音热键（Ctrl+Win）**只有当 WeType 是当前会话的活动输入法时
//!   才生效**：会话切到微软拼音时注入和弦 2/2 不开麦，切回 WeType 后 2/2
//!   恢复触发。
//! - Windows 按应用记忆输入法——用户在其他应用用过微软拼音/豆包/英文后，
//!   这些应用的会话里 WeType 不活跃，语音键表现为"无法唤起"。
//! - 与输入框聚焦无关（对照实验：桌面/资源管理器聚焦时 6/6 照常触发，
//!   p-focus-experiment.ps1）。
//!
//! 方案（参考 macOS 版 PreferredInputSourceMonitor 的"保证语音工具是活动
//! 输入源"职责设计）：注入和弦前，用公开 TSF API
//! （ITfInputProcessorProfileMgr::ActivateProfile + TF_IPPMF_FORSESSION
//! 会话级标志——只影响当前应用会话，不改其他应用的输入法记忆）把 WeType
//! 激活为当前会话输入法。实测：激活后**零延迟**立即注入 3/3 触发（无
//! settling 等待，不增加按键延迟）。
//!
//! 护栏：激活失败只记录提示并按原行为注入（绝不比现状更差）；幂等——
//! WeType 已活跃时重复激活无害；仅在配置了按住说话快捷键的语音会话上调用。

use windows::core::GUID;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, TF_IPPMF_FORSESSION, TF_PROFILETYPE_INPUTPROCESSOR,
};

/// TF_InputProcessorProfiles（msctf.dll，公开 COM 类）。
const CLSID_TF_INPUT_PROCESSOR_PROFILES: GUID =
    GUID::from_u128(0x33c53a50_f456_4884_b049_85fd643e_cfed);

/// 微信输入法（WeType）的 TSF CLSID 与 Profile GUID（公开稳定标识；
/// 来自本机输入法列表 `0804:{86598FB9-…}{607FDF85-…}`，2.1.3.18 实测）。
const WETYPE_CLSID: GUID = GUID::from_u128(0x86598fb9_66a2_463e_b9c2_aeb906d477ad);
const WETYPE_PROFILE: GUID = GUID::from_u128(0x607fdf85_fcc8_4dbd_a365_41296f980c9c);
const LANGID_ZH_CN: u16 = 0x0804;

/// 把微信输入法激活为当前会话的活动输入法（幂等，约 1ms）。
/// 返回 Err 时调用方记录提示后仍按原行为注入——激活失败不阻断语音。
pub fn activate_wetype_session() -> Result<(), String> {
    unsafe {
        let manager: ITfInputProcessorProfileMgr =
            CoCreateInstance(&CLSID_TF_INPUT_PROCESSOR_PROFILES, None, CLSCTX_INPROC_SERVER)
                .map_err(|error| format!("创建 TSF 配置管理器失败：{error}"))?;
        manager
            .ActivateProfile(
                TF_PROFILETYPE_INPUTPROCESSOR,
                LANGID_ZH_CN,
                &WETYPE_CLSID,
                &WETYPE_PROFILE,
                HKL::default(),
                TF_IPPMF_FORSESSION,
            )
            .map_err(|error| format!("激活微信输入法会话失败：{error}"))
    }
}
