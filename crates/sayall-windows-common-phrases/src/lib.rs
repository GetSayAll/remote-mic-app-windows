//! 常用语可选组件的**公开空壳**。
//!
//! 正式实现在私有仓库 `sayall-private-platform/packages/windows-common-phrases/`，
//! 构建期由该包的注入脚本覆盖本目录（`scripts/install-into-host.ps1`）。
//!
//! 未注入时的行为（公开构建）：
//! - [`install`] 返回 `None`：能力不可用，按键映射页不显示「打开常用语」入口；
//! - 已保存但无法用的绑定显示为不可用，不静默替换成含义不同的动作；
//! - 稳定按键、语音与音频功能不受影响。
//!
//! 公开侧**不得**复制私有实现补齐空缺（产品规则见私有平台
//! `docs/product/payment.md` 的「常用语归入会员能力」一节）。

use std::path::PathBuf;
use std::sync::Arc;

use sayall_windows::common_phrases::{CommonPhraseHost, PhrasePanelWindow};

/// 空壳标识：注入脚本与发布校验用它区分「公开空壳」与「私有实现」。
pub const COMPONENT_MARKER: &str = "sayall-windows-common-phrases/stub";

/// 安装入口（与私有实现的签名一致）。空壳恒返回 `None`。
pub fn install(
    _state_dir: PathBuf,
    _window: Option<Arc<dyn PhrasePanelWindow>>,
) -> Option<Arc<dyn CommonPhraseHost>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_reports_the_component_absent() {
        let installed = install(PathBuf::from("."), None);
        assert!(installed.is_none(), "空壳不得提供常用语实现");
        assert_eq!(COMPONENT_MARKER, "sayall-windows-common-phrases/stub");
    }
}
