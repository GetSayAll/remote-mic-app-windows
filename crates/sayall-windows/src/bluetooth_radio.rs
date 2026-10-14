//! 蓝牙栈僵死态的错误码分类（原"无线电自动恢复"模块）。
//!
//! 历史：本模块曾实现"BLE 僵死链路的终极恢复手段"——自动重连连续失败达
//! 阈值时关开整个蓝牙无线电（Off/On），并在 WinRT 栈不可用时回退到提权
//! PnP 重启。依据是 2026-09-05 真机取证：应用被强杀后 Windows 侧可能残留
//! 僵死的 GATT/HID 链路或服务缓存，普通重试循环永远无法恢复。
//!
//! **2026-10-14 产品决策：移除自动关开蓝牙无线电与 PnP 重启。** 原因：
//! - 该动作会断开用户**全部**蓝牙设备（现场报障："开启软件时蓝牙设备
//!   周期性中断、整个蓝牙断（图标消失）"，约 1 小时/次；关闭软件后恢复）；
//! - 收益从未被证明：2026-09-16 A/B 对照显示僵死态下 Off/On 与不开关
//!   统计上不可区分（0.61% vs 0.62%），且"执行成功"多命中启动预热缓存、
//!   未触达真实蓝牙栈；非僵死态的收益也从未测出（本机 6 天内仅执行 2 次：
//!   1 次与恢复无关，1 次无法与"时间自然恢复"区分）。
//!
//! 失败路径现在与其它失败一致：只按既有退避持续重连；僵死态的用户文案在
//! `ble::user_facing_connection_error` 内分流，不在本模块发生任何系统级动作。
//!
//! 历史证据与复算入口（保留供追溯）：`ATTRIBUTION.md`「2026-09-16 A/B 对照」、
//! `Testing/WindowsBleResourceRecovery.md`、`scripts/analyze-radio-recovery-ab.py`。
//!
//! 本模块现在只回答一个问题：**该错误码是否属于"蓝牙栈僵死态"**——用于失败
//! 分类日志与用户文案分流。

/// **系统蓝牙栈僵死**的错误码（`ble::ble_error_code` 的输出）。
///
/// 这些码表示 OS 侧 BLE 栈已耗尽/链路僵死，而不是"遥控器暂时不可达"。
/// 2026-09-16 实测：此态下无线电 Off/On 与提权 PnP 重启均无效（Off/On 报
/// "执行成功"143 次后连接只恢复 3 次 / 2.10%，与同事件内不开关的对照组
/// 0.62% 统计上不可区分）；Off/On 与 PnP 机制已于 2026-10-14 整体移除，
/// 此分类只用于文案与日志分流。
///
/// 证据与复算：`ATTRIBUTION.md`「2026-09-16 A/B 对照」、
/// `Testing/WindowsBleResourceRecovery.md`、`scripts/analyze-radio-recovery-ab.py`。
pub const STACK_EXHAUSTED_ERROR_CODES: [&str; 2] =
    ["windows_resource_exhausted", "winrt_operation_aborted"];

/// 该错误码是否属于"蓝牙栈僵死态"（单元测试覆盖）。
///
/// 只认常量里的码。**不要**在这里另写一份 `matches!` 字面量——两处定义会各自
/// 漂移：往 `STACK_EXHAUSTED_ERROR_CODES` 加码却忘了同步函数，测试照样通过，
/// 而分流会静默漏掉新码（2026-09-16 复查发现过这个隐患）。
pub fn is_stack_exhausted(error_code: &str) -> bool {
    STACK_EXHAUSTED_ERROR_CODES.contains(&error_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 分流的唯一判据。这里**故意**只断言常量里的两个码 + 边界样本，不再抄
    /// 一份字面量：`is_stack_exhausted` 与 `STACK_EXHAUSTED_ERROR_CODES` 必须
    /// 同源，否则往常量加码会静默漏掉分流。
    #[test]
    fn exhausted_stack_is_recognised_and_everything_else_is_not() {
        for code in STACK_EXHAUSTED_ERROR_CODES {
            assert!(is_stack_exhausted(code), "{code} 应被判为僵死态");
        }

        // 近似但不相同的码不能误命中——它们是可恢复的正常失败。
        for code in [
            "windows_resource_exhausted_",
            "WINRT_OPERATION_ABORTED",
            "operation_aborted",
            "",
            "timeout",
        ] {
            assert!(!is_stack_exhausted(code), "{code} 不该被判为僵死态");
        }

        assert_eq!(STACK_EXHAUSTED_ERROR_CODES.len(), 2);
    }
}
