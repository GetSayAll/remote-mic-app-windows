//! 与豆包/语音键抢右 Alt 的桌面程序：Chatterfly 检测（2026-10-01）。
//!
//! 背景（Andy 真机反馈）：在应用里选了豆包、`GetActiveProfile` 也确认豆包是当前
//! 输入法，按住遥控器语音键时出现的却是 **Chatterfly 的语音输入**——说明桌面上
//! 还有第二个消费右 Alt 的常驻程序（与 Vokie 同一类冲突：第三方全局快捷键无法
//! 由本应用阻止，只能检测 + 如实引导）。
//!
//! 只读进程名：不读它的配置、不记路径（隐私红线同 `vokie`）。判据用"在跑"——
//! 没运行就不会抢键。

/// Chatterfly 进程是否在运行（进程名匹配 `chatterfly`，大小写不敏感）。
#[cfg(windows)]
pub fn running() -> bool {
    crate::vokie::any_process_name_matches(matches_chatterfly)
}

#[cfg(not(windows))]
pub fn running() -> bool {
    false
}

/// 进程名判据（纯函数，便于单测做阳性/阴性对照）。
pub(crate) fn matches_chatterfly(process_name: &str) -> bool {
    process_name.to_ascii_lowercase().contains("chatterfly")
}

#[cfg(test)]
mod tests {
    use super::matches_chatterfly;

    /// 只认 Chatterfly 进程名：本机实测进程名为 `ChatterflyCloud`（3 个实例），
    /// 大小写不敏感；不能把别的进程名误判进来（否则会给用户错误的冲突提示）。
    #[test]
    fn matches_chatterfly_process_names_only() {
        assert!(matches_chatterfly("ChatterflyCloud"));
        assert!(matches_chatterfly("ChatterflyCloud.exe"));
        assert!(matches_chatterfly("chatterfly"));
        assert!(!matches_chatterfly("Vokie.exe"));
        assert!(!matches_chatterfly("DoubaoIME.exe"));
        assert!(!matches_chatterfly("explorer.exe"));
    }
}
