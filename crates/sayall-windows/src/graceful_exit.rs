//! 安装/升级前的"优雅退出"信号（2026-09-16）。
//!
//! 背景（两条都有源码或现场证据）：
//!
//! 1. Tauri v2 的 `App::run()` 在事件循环结束时**直接调用 `std::process::exit`**
//!    （`tauri/src/app.rs` 文档原文："This function never returns. When the
//!    application finishes, the process is exited directly using
//!    `std::process::exit`"）。`std::process::exit` **不执行 Rust 析构**，因此
//!    `BleRuntime::drop` 里的会话清理永远不会在进程退出时运行——现场证据：
//!    全日志 21 条 `ble_session_cleanup` 无一条出现在进程结束处。
//! 2. Tauri 的 NSIS 安装器在检测到应用在运行时**直接 `TerminateProcess`**
//!    （`tauri-bundler/.../nsis/utils.nsh` 的 `CheckIfAppIsRunning`：无优雅退出
//!    请求、`Sleep 500` 后即继续；静默安装连提示都没有）。于是"升级"这个动作
//!    会留下未正常关闭的 GATT 会话——正是 AGENTS.md 记录的链路僵死诱因
//!    （「部署不得强杀正在连接的应用」）。
//!
//! 本模块提供两侧对接的那个信号：应用启动时创建一个**会话内**命名事件并阻塞
//! 等待；安装器在强杀之前先置位它，并给应用一段时间自行退出。
//!
//! 为什么用 `Local\` 而不是 `Global\`：非提权进程没有 `SeCreateGlobalPrivilege`，
//! 无法创建全局命名对象；而安装器与应用处于同一登录会话，`Local\` 命名空间对
//! 两者都可见。
//!
//! 安全边界：同一会话内的其它进程也能置位该事件。最坏后果是应用**优雅退出**
//! （不是崩溃、不丢数据），风险可接受。事件名改动即破坏与安装器的兼容，
//! 必须与 `src-tauri/windows/installer-hooks.nsh` 的 `SAYALL_GRACEFUL_EXIT_EVENT`
//! 同步。安装器侧的等待是"先静默 `SAYALL_GRACEFUL_EXIT_SETTLE_MS`（1500ms），
//! 若进程仍在再补 `SAYALL_GRACEFUL_EXIT_TAIL_MS`（6500ms）"，合计 8 秒；
//! 应用侧的收尾超时必须明显小于它。

use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, OpenEventW, ResetEvent, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    INFINITE,
};

/// 命名事件名（与会话内安装器约定）。见模块头部的同步要求。
pub const GRACEFUL_EXIT_EVENT_NAME: &str = r"Local\SayAll-GracefulExit";

fn wide(name: &str) -> Vec<u16> {
    let mut buffer: Vec<u16> = name.encode_utf16().collect();
    buffer.push(0);
    buffer
}

/// 应用侧持有的"请求退出"信号。
///
/// 创建后一直持有句柄（`Drop` 时关闭）：安装器通过 `OpenEventW` + `SetEvent`
/// 置位。用**手动重置**（manual reset）而非自动重置，是为了避免"等待线程晚于
/// 信号启动"时漏掉请求。
pub struct GracefulExitSignal {
    handle: windows::Win32::Foundation::HANDLE,
}

impl GracefulExitSignal {
    /// 用生产事件名创建（或打开已存在的）。
    pub fn create() -> windows::core::Result<Self> {
        Self::create_named(GRACEFUL_EXIT_EVENT_NAME)
    }

    /// 用指定名字创建。生产代码只应使用 [`GracefulExitSignal::create`]；
    /// 参数化是为了让测试能用互不干扰的事件名（命名事件是全局对象，共用名字
    /// 会让并行测试互相置位）。
    pub fn create_named(name: &str) -> windows::core::Result<Self> {
        let name = wide(name);
        let handle = unsafe { CreateEventW(None, true, false, PCWSTR::from_raw(name.as_ptr())) }?;
        // 防御陈旧置位（2026-10-15 实测"应用启动即退出"）：手动重置事件一经置位
        // 就保持，且只要还有任一持有者（例如诊断/测试脚本 OpenEventW + SetEvent 后
        // 不关句柄）对象就不销毁——新实例照常创建会立刻等到这个陈旧信号、启动即
        // 退出。启动时清除一次；安装器若在此之前置位，说明它针对的是"上一个实例"
        // （本实例尚未运行，没有需要退出的理由），清除无碍；在本行之后的信号正常
        // 唤醒等待（竞态窗口为毫秒级，极端情况下安装器退回其有界等待路径）。
        let _ = unsafe { ResetEvent(handle) };
        Ok(Self { handle })
    }

    /// 无限期等待安装器请求退出。返回 `true` 表示收到请求。
    pub fn wait(&self) -> bool {
        unsafe { WaitForSingleObject(self.handle, INFINITE) == WAIT_OBJECT_0 }
    }

    /// 有界等待（供单元测试与需要兜底的调用方使用）。
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        unsafe { WaitForSingleObject(self.handle, millis) == WAIT_OBJECT_0 }
    }
}

impl Drop for GracefulExitSignal {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

/// 置位指定名字的事件（安装器侧同样做的事，此处供测试与诊断复用）。
///
/// 生产路径上置位的是安装器（NSIS `System::Call` → `SetEvent`），不是本函数。
pub fn signal_named(name: &str) -> windows::core::Result<()> {
    let name = wide(name);
    unsafe {
        let handle = OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR::from_raw(name.as_ptr()))?;
        let result = SetEvent(handle);
        let _ = CloseHandle(handle);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 安装器钩子里硬编码了同一个名字；改名会让"部署前请求退出"静默失效。
    #[test]
    fn event_name_matches_the_installer_contract() {
        assert_eq!(GRACEFUL_EXIT_EVENT_NAME, r"Local\SayAll-GracefulExit");
    }

    #[test]
    fn bounded_wait_times_out_without_a_signal() {
        let signal = GracefulExitSignal::create_named(r"Local\SayAll-GracefulExit-test-timeout")
            .expect("create named event");
        assert!(!signal.wait_timeout(Duration::from_millis(50)));
    }

    /// 安装器靠"打不开事件"判断没有实例在运行（`OpenEventW` 失败即跳过，零开销）。
    /// 这条测试把这个前提钉住：事件不存在时置位必须返回错误，而不是静默成功。
    #[test]
    fn signalling_an_absent_event_fails_so_the_installer_can_skip() {
        assert!(signal_named(r"Local\SayAll-GracefulExit-absent").is_err());
    }

    #[test]
    fn signal_is_observed_by_a_waiter() {
        const NAME: &str = r"Local\SayAll-GracefulExit-test-signal";
        let signal = GracefulExitSignal::create_named(NAME).expect("create named event");
        let setter = std::thread::spawn(move || signal_named(NAME).expect("set named event"));
        assert!(signal.wait_timeout(Duration::from_secs(5)));
        setter.join().expect("setter thread");
        // 手动重置：置位后保持，后续等待立即返回（防"等待线程晚于信号启动"）。
        assert!(signal.wait_timeout(Duration::from_millis(10)));
    }

    /// 防御回归（2026-10-15 实测"应用启动即退出"）：命名事件可能处于**陈旧的
    /// 置位状态**——第三方脚本 `OpenEventW + SetEvent` 后不关句柄，事件对象既
    /// 不销毁又保持置位；此时新实例照常创建会立刻等到这个陈旧信号、启动即退出。
    /// 启动时必须清除陈旧置位。
    #[test]
    fn a_stale_signalled_event_is_cleared_on_create() {
        const NAME: &str = r"Local\SayAll-GracefulExit-test-stale";
        // 模拟旧持有者 + 一次"空发"置位。
        let holder = GracefulExitSignal::create_named(NAME).expect("create named event");
        signal_named(NAME).expect("set named event");
        assert!(
            holder.wait_timeout(Duration::from_millis(10)),
            "置位必须先生效"
        );
        // 新实例创建：陈旧置位必须被清除，watcher 不得立即返回。
        let newcomer = GracefulExitSignal::create_named(NAME).expect("create named event");
        assert!(
            !newcomer.wait_timeout(Duration::from_millis(100)),
            "陈旧信号必须在新实例启动时被清除（否则应用启动即退出）"
        );
    }
}
