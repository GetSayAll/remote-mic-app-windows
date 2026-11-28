//! 移动端连接（iOS App / 微信小程序）的宿主接入点。
//!
//! 公开构建只有空实现：快照返回 `unavailable`，前端据此不渲染整个移动端分区。
//! 私有构建在打包前替换 [`hook`] 的文件内容，由它返回真正的连接运行时。
//! 契约见 `docs/architecture/private-component-boundary.md`。

mod hook;
mod runtime;

use std::sync::Arc;

#[allow(unused_imports)]
pub use runtime::{
    NearbyRemoteSnapshot, NearbyRemoteState, PhoneRemoteApproval, PhoneRemoteAvailability,
    PhoneRemoteMembershipState, PhoneRemoteRuntime, PhoneRemoteSnapshot, WebRemoteSnapshot,
    WebRemoteState,
};

/// 私有运行时需要的公开宿主能力。
///
/// 私有实现只能通过这些接口读取会员与平台状态；Windows 额外携带音频、按键
/// 引擎与语音会话句柄，避免私有层复制一套下游。前端拿不到这些类型。
#[allow(dead_code)]
pub struct PhoneRemoteAccess {
    pub platform: Arc<dyn crate::platform::PlatformRuntime>,
    pub membership: Arc<dyn crate::membership::MembershipRuntime>,
    /// 安装包版本（Tauri config 的 version），交给手机协议显示与诊断；不是 Cargo
    /// workspace 的 0.0.0 占位值。
    pub app_version: String,
    /// 出现待确认请求时把主窗口带回前台。回调可能从工作线程调用，实现必须自己
    /// 处理线程边界；空实现表示不打扰用户。
    pub on_approval: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Windows 真机上的音频渲染、按键引擎与语音会话句柄。仿真/公开构建为 `None`。
    #[cfg(windows)]
    pub audio: Option<Arc<sayall_windows::audio::AudioRuntime>>,
    #[cfg(windows)]
    pub button_mapping: Option<Arc<sayall_windows::button_mapping::ButtonMappingRuntime>>,
    #[cfg(windows)]
    pub voice: Option<sayall_windows::phone_voice::PhoneVoiceAccess>,
}

pub fn install(access: PhoneRemoteAccess) -> Arc<dyn PhoneRemoteRuntime> {
    hook::install(access).unwrap_or_else(|| Arc::new(runtime::UnavailablePhoneRemoteRuntime))
}
