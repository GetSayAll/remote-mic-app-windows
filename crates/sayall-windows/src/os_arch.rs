//! 本机的**原生**系统架构（不是本进程的仿真架构）。
//!
//! 为什么单独有这个模块：x64 程序在 ARM64 Windows 上以仿真方式运行，凡是"看自己"的办法
//! 都会给出误导性的答案——`std::env::consts::ARCH` 是**编译期**常量（x64 构建永远是
//! `x86_64`），`PROCESSOR_ARCHITECTURE` 环境变量同样报 `AMD64`。而增强捕获必须知道**系统**
//! 架构：承载遥控器的宿主 `WUDFHost.exe` 在 ARM64 系统上是原生 ARM64 进程，只有 arm64 助手
//! 与 arm64 Gadget 能注入它（Issue #206：x64 助手在 ARM64 上必然失败）。
//!
//! 取值口径：`IsWow64Process2` 的 `nativeMachine`（Windows 10 1709+；本产品最低要求
//! build 17763，故正常情况下一定可用）。调用失败时归 `Unknown`，调用方按"维持现状"处理
//! （即沿用 x64 路径），不因为一次探测失败就把功能判死。
//!
//! 判定逻辑是纯函数（不碰 API），因此即使手头没有 ARM64 机器，也能在 CI 里把每种取值钉住。

use std::fmt;

/// `IMAGE_FILE_MACHINE_AMD64`（PE 头里的 machine 值，x64）。
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
/// `IMAGE_FILE_MACHINE_ARM64`（PE 头里的 machine 值，ARM64）。
pub const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;

/// 本机原生架构。只区分本产品会分发载荷的两种，其余归 `Other`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeArch {
    X64,
    Arm64,
    /// 明确不是 x64 / ARM64 的原生架构（如 32 位 ARM、x86 系统）。
    Other,
    /// 探测失败：不知道，不据此否定任何能力。
    Unknown,
}

impl NativeArch {
    /// 对外（结构化日志与界面契约）的名字。`Other` 与 `Unknown` 都写 `unknown`：
    /// 对用户与前端而言两者是同一件事——"没识别出受支持的架构"。
    pub const fn as_str(self) -> &'static str {
        match self {
            NativeArch::X64 => "x64",
            NativeArch::Arm64 => "arm64",
            NativeArch::Other | NativeArch::Unknown => "unknown",
        }
    }

    /// 随安装包分发的助手文件名（两种架构各一份，见 ADR 0003）。
    pub const fn helper_file_name(self) -> &'static str {
        match self {
            NativeArch::Arm64 => "sayall-helper-arm64.exe",
            _ => "sayall-helper.exe",
        }
    }

    /// 随安装包分发的 Gadget 文件名（与助手的架构一一对应）。
    pub const fn gadget_file_name(self) -> &'static str {
        match self {
            NativeArch::Arm64 => "frida-gadget-arm64.dll",
            _ => "frida-gadget.dll",
        }
    }

    /// 是否按"架构专属载荷"路径选择助手与 Gadget。
    /// `Unknown` 也走 x64 那一份：这是探测失败时的"维持现状"口径（与升级前行为一致）。
    pub const fn uses_arch_specific_payload(self) -> bool {
        matches!(
            self,
            NativeArch::X64 | NativeArch::Arm64 | NativeArch::Unknown
        )
    }

    /// 明确不受支持的原生架构（例如 32 位 ARM 的 Windows）。`Unknown` 不算：
    /// 探测失败不等于不支持。
    pub const fn is_explicitly_unsupported(self) -> bool {
        matches!(self, NativeArch::Other)
    }
}

impl fmt::Display for NativeArch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// `IMAGE_FILE_MACHINE_*` → 原生架构（纯函数）。
pub const fn classify_native_machine(machine: u16) -> NativeArch {
    match machine {
        IMAGE_FILE_MACHINE_AMD64 => NativeArch::X64,
        IMAGE_FILE_MACHINE_ARM64 => NativeArch::Arm64,
        _ => NativeArch::Other,
    }
}

/// 本机原生架构。
#[cfg(windows)]
pub fn native_arch() -> NativeArch {
    use windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE;
    use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};

    let mut process_machine = IMAGE_FILE_MACHINE::default();
    let mut native_machine = IMAGE_FILE_MACHINE::default();
    // 只读这两个出参；它们无副作用，进程句柄是伪句柄（不需要关闭）。
    let probed = unsafe {
        IsWow64Process2(
            GetCurrentProcess(),
            &mut process_machine,
            Some(&mut native_machine),
        )
    };
    match probed {
        Ok(()) => classify_native_machine(native_machine.0),
        Err(_) => NativeArch::Unknown,
    }
}

#[cfg(not(windows))]
pub fn native_arch() -> NativeArch {
    NativeArch::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_the_two_payload_arches() {
        assert_eq!(classify_native_machine(0x8664), NativeArch::X64);
        assert_eq!(classify_native_machine(0xAA64), NativeArch::Arm64);
        assert_eq!(classify_native_machine(0x014C), NativeArch::Other);
        assert_eq!(classify_native_machine(0x0000), NativeArch::Other);
    }

    /// 名字是前端契约（`nativeArch: "x64" | "arm64" | "unknown"`），改动即破坏契约。
    #[test]
    fn names_match_the_frontend_contract() {
        assert_eq!(NativeArch::X64.as_str(), "x64");
        assert_eq!(NativeArch::Arm64.as_str(), "arm64");
        assert_eq!(NativeArch::Other.as_str(), "unknown");
        assert_eq!(NativeArch::Unknown.as_str(), "unknown");
    }

    /// 载荷文件名必须与打包脚本、锁文件、助手默认查找名逐字一致。
    #[test]
    fn payload_names_match_the_frozen_bundle_names() {
        assert_eq!(NativeArch::X64.helper_file_name(), "sayall-helper.exe");
        assert_eq!(
            NativeArch::Arm64.helper_file_name(),
            "sayall-helper-arm64.exe"
        );
        assert_eq!(NativeArch::X64.gadget_file_name(), "frida-gadget.dll");
        assert_eq!(
            NativeArch::Arm64.gadget_file_name(),
            "frida-gadget-arm64.dll"
        );
        // 探测失败时维持升级前的 x64 行为
        assert_eq!(NativeArch::Unknown.helper_file_name(), "sayall-helper.exe");
        assert_eq!(NativeArch::Unknown.gadget_file_name(), "frida-gadget.dll");
    }

    #[test]
    fn only_other_is_explicitly_unsupported() {
        assert!(!NativeArch::X64.is_explicitly_unsupported());
        assert!(!NativeArch::Arm64.is_explicitly_unsupported());
        // 探测失败不等于不支持
        assert!(!NativeArch::Unknown.is_explicitly_unsupported());
        assert!(NativeArch::Other.is_explicitly_unsupported());
    }

    /// 真实探测的不变量：能跑起本产品的 Windows 机器，原生架构必然是 x64 或 ARM64。
    ///
    /// 刻意**不**断言"等于编译架构"——x64 构建在 ARM64 机器上以仿真运行是本方案的常态，
    /// 那种断言会在 ARM64 机器上误报（正是 Issue #206 那类机器的形态）。
    #[test]
    fn real_probe_never_reports_an_unsupported_arch_on_windows() {
        if cfg!(windows) {
            let probed = native_arch();
            assert!(
                !probed.is_explicitly_unsupported() && probed != NativeArch::Unknown,
                "真实 Windows 机器应当探得到 x64 或 arm64，实际 {probed}"
            );
        }
    }
}
