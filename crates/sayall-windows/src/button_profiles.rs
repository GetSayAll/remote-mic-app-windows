//! 键位方案的可选宿主接缝。
//!
//! 公开构建只有 [`DisabledButtonProfileHost`]：入口不可用，所有调用都是 no-op。
//! 私有实现由 `sayall-windows-button-profiles` 在构建期注入，并在应用启动时通过
//! [`set_button_profile_host`] 安装。公开层不读取方案文件、不解析 `.sayall`、
//! 不做 App 匹配，也不保存会员权益。
//!
//! 前端只依赖稳定的命令名与 JSON 形状；业务语义全部在私有组件内。

use std::sync::{Arc, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::send_input::ButtonMappings;

/// 私有组件需要的公开宿主能力。
///
/// 公开宿主只负责转发：写覆盖层、读基础映射、读前台可执行文件名。
/// 私有组件不直接持有引擎句柄，也不执行 SendInput。
pub trait ButtonProfileHostContext: Send + Sync {
    /// 安装/清除当前方案覆盖层。
    fn set_overlay(&self, overlay: Option<ButtonMappings>);

    /// 当前基础映射（用于默认方案概览与生成覆盖层）。
    fn base_mappings(&self) -> ButtonMappings;

    /// 当前前台进程的可执行文件名（例如 `cloudmusic.exe`）；读不到为 `None`。
    fn foreground_executable(&self) -> Option<String> {
        None
    }
}

/// 私有组件给出的访问状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum ProfileAccess {
    /// 私有组件未注入或不可用。
    Unavailable,
    /// 已保存方案可见，但编辑/自动应用需要 Plus。
    RequiresPlus,
    /// 可使用；`valid_until` 只用于界面提示，不参与公开层判断。
    Allowed {
        #[serde(
            rename = "validUntil",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        valid_until: Option<String>,
    },
}

/// 键位方案宿主：私有组件实现，公开命令层只转发。
pub trait ButtonProfileHost: Send + Sync {
    /// 私有实现是否已安装（不读取方案文件）。
    fn is_available(&self) -> bool {
        false
    }

    fn access(&self) -> ProfileAccess {
        ProfileAccess::Unavailable
    }

    /// 会员组件推送最新访问状态；公开构建与未注入时 no-op。
    fn set_access(&self, _access: ProfileAccess) {}

    /// 页面快照（JSON）。`None` = 宿主不可用。
    fn snapshot_json(&self) -> Option<String> {
        None
    }

    /// 页面命令分发。公开层不理解命令内容。
    fn dispatch(&self, _command: &str, _payload: Option<&str>) -> Result<String, String> {
        Err("button_profile_host_unavailable".to_owned())
    }

    /// 前台应用变化（可执行文件名；`None` = 无前台应用）。
    fn foreground_changed(&self, _executable: Option<&str>) {}
}

/// 未注入私有组件时的宿主。
#[derive(Debug, Default)]
pub struct DisabledButtonProfileHost;

impl ButtonProfileHost for DisabledButtonProfileHost {}

/// 全局宿主槽：与常用语组件同形，启动时安装一次。
static BUTTON_PROFILE_HOST: OnceLock<RwLock<Arc<dyn ButtonProfileHost>>> = OnceLock::new();

fn button_profile_slot() -> &'static RwLock<Arc<dyn ButtonProfileHost>> {
    BUTTON_PROFILE_HOST.get_or_init(|| RwLock::new(Arc::new(DisabledButtonProfileHost)))
}

/// 安装/替换宿主（重复调用以后写入者为准）。
pub fn set_button_profile_host(host: Arc<dyn ButtonProfileHost>) {
    let slot = button_profile_slot();
    match slot.write() {
        Ok(mut guard) => *guard = host,
        Err(poisoned) => *poisoned.into_inner() = host,
    }
}

/// 当前宿主；公开构建恒为 [`DisabledButtonProfileHost`]。
pub fn current_button_profile_host() -> Arc<dyn ButtonProfileHost> {
    match button_profile_slot().read() {
        Ok(guard) => Arc::clone(&guard),
        Err(poisoned) => Arc::clone(&poisoned.into_inner()),
    }
}

/// 私有组件是否已注入。
pub fn is_button_profile_host_available() -> bool {
    current_button_profile_host().is_available()
}

/// 会员组件推送访问状态（未注入时 no-op）。
pub fn set_button_profile_access(access: ProfileAccess) {
    current_button_profile_host().set_access(access);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_host_is_unavailable_and_forwards_nothing() {
        let host = DisabledButtonProfileHost;
        assert_eq!(host.access(), ProfileAccess::Unavailable);
        assert!(host.snapshot_json().is_none());
        assert_eq!(
            host.dispatch("snapshot", None),
            Err("button_profile_host_unavailable".to_owned())
        );
        host.foreground_changed(Some("notepad.exe"));
    }

    #[test]
    fn default_slot_has_no_private_component() {
        assert!(!is_button_profile_host_available());
        assert_eq!(
            current_button_profile_host().access(),
            ProfileAccess::Unavailable
        );
    }

    #[test]
    fn access_serializes_with_the_frontend_contract() {
        assert_eq!(
            serde_json::to_value(ProfileAccess::RequiresPlus).unwrap()["status"],
            "requiresPlus"
        );
        assert_eq!(
            serde_json::to_value(ProfileAccess::Allowed {
                valid_until: Some("2026-11-01T00:00:00Z".to_owned())
            })
            .unwrap()["validUntil"],
            "2026-11-01T00:00:00Z"
        );
    }
}
