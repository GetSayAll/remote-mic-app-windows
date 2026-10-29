//! 键位方案的公开宿主接缝。
//!
//! 公开构建只安装空壳：`install` 返回 `false`，页面与命令都不可用。
//! 私有组件在构建期注入 `sayall-windows-button-profiles` 与前端页面；
//! 本文件只负责把宿主能力（状态目录、覆盖层写入口）交给它，不理解方案内容。

use std::sync::Arc;

use tauri::Manager;

use sayall_windows::button_profiles::{
    current_button_profile_host, set_button_profile_access, set_button_profile_host,
    ButtonProfileHostContext, ProfileAccess,
};
use sayall_windows::send_input::ButtonMappings;

use crate::membership::{MembershipAvailability, MembershipRuntime, MembershipSnapshot};
use crate::platform::PlatformRuntime;

struct PlatformProfileContext {
    platform: Arc<dyn PlatformRuntime>,
}

impl ButtonProfileHostContext for PlatformProfileContext {
    fn set_overlay(&self, overlay: Option<ButtonMappings>) {
        self.platform.set_button_profile_overlay(overlay);
    }

    fn base_mappings(&self) -> ButtonMappings {
        self.platform.button_mappings()
    }

    fn foreground_executable(&self) -> Option<String> {
        foreground_executable()
    }
}

#[cfg(windows)]
fn foreground_executable() -> Option<String> {
    let path = sayall_windows::focus_windows::foreground_executable_path()?;
    std::path::Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_owned())
}

#[cfg(not(windows))]
fn foreground_executable() -> Option<String> {
    None
}

/// 安装私有组件（应用启动时调用一次）。
///
/// 未注入时返回 `false`：不注册任何入口，基础按键行为不变。
pub fn install(
    app: &tauri::AppHandle,
    platform: Arc<dyn PlatformRuntime>,
    membership: Arc<dyn MembershipRuntime>,
) -> bool {
    let state_dir = match app.path().app_config_dir() {
        Ok(dir) => dir,
        Err(_) => return false,
    };
    let context: Arc<dyn ButtonProfileHostContext> = Arc::new(PlatformProfileContext { platform });
    match sayall_windows_button_profiles::install(state_dir, context) {
        Some(host) => {
            set_button_profile_host(host);
            apply_membership(&membership.snapshot());
            true
        }
        None => false,
    }
}

/// 把会员摘要映射成键位方案访问状态（公开构建没有私有宿主，调用无效果）。
pub fn apply_membership(snapshot: &MembershipSnapshot) {
    if !current_button_profile_host().is_available() {
        return;
    }
    // 走查包（`runtime-simulation`）直接按 Plus 运行；正式 release 构建不会命中。
    let assume_member = cfg!(feature = "runtime-simulation");
    let valid_until = snapshot.valid_until.map(|value| value.to_string());
    let access = if assume_member {
        ProfileAccess::Allowed { valid_until }
    } else if snapshot.availability == MembershipAvailability::Unavailable
        || snapshot.sync_state == "unavailable"
    {
        // 会员组件没有安装、初始化失败，或没有可用于判断的租约：
        // 这不是“需要 Plus”，而是能力暂时不可用。
        ProfileAccess::Unavailable
    } else if snapshot.plus_active {
        if snapshot.sync_state == "offline" {
            // 上次验签的租约仍允许使用；界面应提示离线，而不是卖会员。
            ProfileAccess::TemporarilyOffline { valid_until }
        } else {
            ProfileAccess::Allowed { valid_until }
        }
    } else {
        ProfileAccess::RequiresPlus
    };
    set_button_profile_access(access);
}

/// 页面快照（私有 JSON；公开构建为 `null`）。
#[tauri::command]
pub fn button_profiles_snapshot() -> Option<String> {
    current_button_profile_host().snapshot_json()
}

/// 页面访问状态（公开构建 `unavailable`）。
#[tauri::command]
pub fn button_profiles_access() -> ProfileAccess {
    current_button_profile_host().access()
}

/// 选择 `.sayall` 导入文件（取消返回 `null`）。
#[tauri::command]
pub fn button_profiles_pick_import_file() -> Result<Option<String>, String> {
    sayall_windows::file_dialog::pick_button_profile_import_path()
        .map(|path| path.map(|path| path.to_string_lossy().into_owned()))
}

/// 选择键位方案备份的导出位置（取消返回 `null`）。
#[tauri::command]
pub fn button_profiles_pick_export_file() -> Result<Option<String>, String> {
    sayall_windows::file_dialog::pick_button_profile_export_path()
        .map(|path| path.map(|path| path.to_string_lossy().into_owned()))
}

/// 页面命令分发（命令名与 payload 均由私有组件定义）。
#[tauri::command]
pub fn button_profiles_dispatch(
    command: String,
    payload: Option<String>,
) -> Result<String, String> {
    current_button_profile_host().dispatch(&command, payload.as_deref())
}
