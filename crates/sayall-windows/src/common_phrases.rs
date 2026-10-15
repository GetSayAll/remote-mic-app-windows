//! 常用语宿主接缝：公开侧**只**保留稳定动作 ID 与可选适配接口。
//!
//! 产品规则（2026-10-12/2026-10-13 定稿）：常用语是会员能力，Plus 与 Pro 可用、Free 不可用；
//! 实现、内置资源与测试位于私有 `packages/windows-common-phrases/`（macOS 对应
//! `packages/macos-common-phrases/`）。公开构建缺少私有组件时：
//!
//! - 按键映射页不显示「打开常用语」入口；
//! - 已保存但无法用的绑定显示为不可用，**不静默替换**成含义不同的动作；
//! - 稳定按键、语音与音频功能不受影响。
//!
//! 本模块因此只定义：
//! - 门禁决策类型 [`CommonPhraseAccess`]（会员事实源在私有会员组件，公开侧不做判断）；
//! - 适配接口 [`CommonPhraseHost`]（私有组件实现；未接入时用 [`DisabledCommonPhraseHost`]）。
//!
//! 按键认领约束（与 macOS 侧一致）：面板打开期间对 OK/左/上/右/下/返回 的认领必须发生在
//! **手势识别之前**，命中即直接执行并结束，不得进入单击/双击/长按判定，也不得写成持久绑定。

use crate::raw_input::RemoteButton;

/// 面板按键认领的来源标识（Windows 只有一条遥控器来源；保留参数以便未来多来源）。
pub const PHRASE_SOURCE_DEFAULT: &str = "remote";

/// 会员门禁决策。公开侧不产生该决策，只透传私有组件的判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommonPhraseAccess {
    /// 私有组件未接入（公开构建的默认状态）：能力不可用，入口隐藏。
    Unavailable,
    /// 组件已接入但当前没有有效会员租约（Free 用户）：显示会员入口，不可使用。
    Denied,
    /// 有有效会员租约；`valid_until_unix` 为租约到期时间（未知时为 `None`）。
    Allowed { valid_until_unix: Option<i64> },
}

/// 面板浮层窗口（由宿主实现）：创建**不抢焦点**的浮层并加载可选组件的 UI 资源。
///
/// 约定：`show` 不得抢走前台应用的焦点；`hide` 必须幂等且不得改变用户按键映射。
pub trait PhrasePanelWindow: Send + Sync {
    fn show(&self);
    fn hide(&self);
}

/// 面板上某个位置的数据（只含短显示，不含完整句）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonPhrasePanelEntry {
    pub button: RemoteButton,
    /// 该位置的短显示；未分配时为空字符串。
    pub label: String,
}

/// 可选常用语组件适配接口。所有方法都有默认实现（不可用 + 不认领），
/// 因此未接入私有组件时引擎行为与改动前完全一致。
pub trait CommonPhraseHost: Send + Sync {
    /// 门禁决策（界面据此显示入口/会员提示）。
    fn access(&self) -> CommonPhraseAccess {
        CommonPhraseAccess::Unavailable
    }

    /// 会员是否可用（`Allowed` 的便捷判据）。
    fn can_use(&self) -> bool {
        matches!(self.access(), CommonPhraseAccess::Allowed { .. })
    }

    /// 打开面板（返回是否真的打开）。未接入或非会员时返回 `false`。
    fn open(&self, source: &str) -> bool {
        let _ = source;
        false
    }

    /// 该按键此刻是否归面板所有（只读预判；供门控遮蔽与界面状态使用）。
    fn claims(&self, button: RemoteButton, source: &str) -> bool {
        let _ = (button, source);
        false
    }

    /// 处理一次按键边沿；返回 `true` 表示已被面板消费（不得进入手势识别，也不执行原映射）。
    fn handle(&self, button: RemoteButton, is_pressed: bool, source: &str) -> bool {
        let _ = (button, is_pressed, source);
        false
    }

    /// 面板当前是否打开。
    fn is_panel_open(&self) -> bool {
        false
    }

    /// 来源失效（遥控器断连、输入重置）：关闭面板并清掉认领状态。
    fn reset(&self, reason: &str) {
        let _ = reason;
    }

    /// 关闭面板（返回键之外的路径：应用退出、前台变化等）。
    fn close(&self, reason: &str) {
        let _ = reason;
    }

    /// 面板五个位置的当前数据（供浮层渲染；只含短显示）。
    fn panel_entries(&self) -> Vec<CommonPhrasePanelEntry> {
        Vec::new()
    }

    /// 鼠标点选面板某个位置（与遥控器按下等价）；返回是否已受理。
    fn select(&self, button: RemoteButton) -> bool {
        let _ = button;
        false
    }
}

/// 未接入私有组件时的空实现：不可用、不认领、不影响任何按键行为。
#[derive(Debug, Default)]
pub struct DisabledCommonPhraseHost;

impl CommonPhraseHost for DisabledCommonPhraseHost {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_host_is_unavailable_and_never_claims_a_button() {
        let host = DisabledCommonPhraseHost;
        assert_eq!(host.access(), CommonPhraseAccess::Unavailable);
        assert!(!host.can_use());
        assert!(!host.open(PHRASE_SOURCE_DEFAULT));
        assert!(!host.is_panel_open());
        for button in [
            RemoteButton::Ok,
            RemoteButton::Left,
            RemoteButton::Up,
            RemoteButton::Right,
            RemoteButton::Down,
            RemoteButton::Back,
            RemoteButton::VolumeUp,
        ] {
            assert!(!host.claims(button, PHRASE_SOURCE_DEFAULT));
            assert!(!host.handle(button, true, PHRASE_SOURCE_DEFAULT));
            assert!(!host.handle(button, false, PHRASE_SOURCE_DEFAULT));
        }
        // 收尾路径必须可调用且无副作用。
        host.reset("test");
        host.close("test");
        assert!(host.panel_entries().is_empty());
        assert!(!host.select(RemoteButton::Ok));
    }

    #[test]
    fn panel_button_mask_covers_exactly_the_six_panel_buttons() {
        let mask = crate::key_gate::phrase_panel_mask();
        let expected = [
            RemoteButton::Ok,
            RemoteButton::Left,
            RemoteButton::Up,
            RemoteButton::Right,
            RemoteButton::Down,
            RemoteButton::Back,
        ];
        for button in expected {
            assert_eq!(
                (mask >> button.ordinal()) & 1,
                1,
                "{button:?} 必须在遮蔽掩码里"
            );
        }
        for button in [
            RemoteButton::Home,
            RemoteButton::Tv,
            RemoteButton::Menu,
            RemoteButton::VolumeUp,
        ] {
            assert_eq!((mask >> button.ordinal()) & 1, 0, "{button:?} 不得被遮蔽");
        }
    }
}
