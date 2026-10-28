//! 键位方案可选组件的**公开空壳**。
//!
//! 正式实现在私有仓 `sayall-private-platform/packages/windows-button-profiles/`，
//! 构建期由该包的注入脚本覆盖本目录（`scripts/install-into-host.ps1`）。
//!
//! 未注入时的行为（公开构建）：
//! - [`install`] 返回 `None`：能力不可用，侧栏不显示「键位方案」入口；
//! - 已存在的私有方案文件不读取、不参与按键执行；
//! - 稳定按键、语音与音频功能不受影响。
//!
//! 公开侧**不得**复制私有实现补齐空缺（产品规则见私有平台
//! `packages/windows-button-profiles/docs/private-boundary.md`）。

use std::path::PathBuf;
use std::sync::Arc;

use sayall_windows::button_profiles::{ButtonProfileHost, ButtonProfileHostContext};

/// 空壳标识：注入脚本与发布校验用它区分「公开空壳」与「私有实现」。
pub const COMPONENT_MARKER: &str = "sayall-windows-button-profiles/stub";

/// 安装入口（与私有实现的签名一致）。空壳恒返回 `None`。
///
/// `context` 是公开宿主能力；私有实现用它安装/清除方案。
pub fn install(
    _state_dir: PathBuf,
    _context: Arc<dyn ButtonProfileHostContext>,
) -> Option<Arc<dyn ButtonProfileHost>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopContext;

    impl ButtonProfileHostContext for NoopContext {
        fn set_overlay(&self, _overlay: Option<sayall_windows::send_input::ButtonMappings>) {}
        fn base_mappings(&self) -> sayall_windows::send_input::ButtonMappings {
            Default::default()
        }
    }

    #[test]
    fn stub_reports_the_component_absent() {
        let installed = install(PathBuf::from("."), Arc::new(NoopContext));
        assert!(installed.is_none(), "空壳不得提供键位方案实现");
        assert_eq!(COMPONENT_MARKER, "sayall-windows-button-profiles/stub");
    }
}
