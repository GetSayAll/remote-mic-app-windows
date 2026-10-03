//! 「第二个实例请求显示主窗口」的会话内命名事件（2026-10-03）。
//!
//! 背景：单实例守卫（`src-tauri/src/lib.rs`）在检测到已有实例时让第二个进程
//! 直接退出。当主窗口最小化或收进托盘时，双击快捷方式 / 再点启动图标毫无可见
//! 反应——用户 2026-10-03 报障的两个入口之一。本模块提供两端的对接信号：
//! 已运行实例创建事件并阻塞等待，第二个实例在退出前置位事件；等待端收到后
//! 把主窗口恢复并置前。
//!
//! 与 `graceful_exit` 的差异（同为会话内命名事件模式）：本事件用**自动重置**
//! （bManualReset=false）。一次置位对应恰好一次"显示"请求，等待端消费后事件
//! 回到未置位，陈旧信号不会反复唤醒；而置位时若暂无等待者，信号保持到第一次
//! 等待消费，请求不会丢失（等待线程晚于请求启动的场景）。
//!
//! 安全边界：同一登录会话内的其它进程也能置位该事件，最坏后果是主窗口被显示
//! 并尝试置前（用户可见、无数据风险），与 `graceful_exit` 属同一级可接受风险。

use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
};

/// 命名事件名（会话内约定）。`src-tauri/src/lib.rs` 的等待线程与二次启动分支
/// 都只经本模块引用，改名不会漏掉任一端。
pub const SHOW_MAIN_WINDOW_EVENT_NAME: &str = r"Local\SayAll-ShowMainWindow";

fn wide(name: &str) -> Vec<u16> {
    let mut buffer: Vec<u16> = name.encode_utf16().collect();
    buffer.push(0);
    buffer
}

/// 已运行实例持有的"显示主窗口"请求信号。
///
/// 创建后一直持有句柄（`Drop` 时关闭）；等待方按进程存活设计，不依赖析构
/// （Tauri 的退出路径是 `std::process::exit`，不执行析构）。
pub struct ShowMainWindowSignal {
    handle: HANDLE,
}

impl ShowMainWindowSignal {
    /// 用生产事件名创建（或打开已存在的）。
    pub fn create() -> windows::core::Result<Self> {
        Self::create_named(SHOW_MAIN_WINDOW_EVENT_NAME)
    }

    /// 用指定名字创建。生产代码只应使用 [`ShowMainWindowSignal::create`]；
    /// 参数化是为了让测试能用互不干扰的事件名（命名事件是全局对象，共用名字
    /// 会让测试互相置位，甚至误触本机正在运行的应用）。
    pub fn create_named(name: &str) -> windows::core::Result<Self> {
        let name = wide(name);
        // 自动重置：见模块头部的语义说明。
        let handle = unsafe { CreateEventW(None, false, false, PCWSTR::from_raw(name.as_ptr())) }?;
        Ok(Self { handle })
    }

    /// 无限期等待下一次显示请求。返回 `false` 表示等待本身失败。
    pub fn wait(&self) -> bool {
        unsafe { WaitForSingleObject(self.handle, INFINITE) == WAIT_OBJECT_0 }
    }

    /// 有界等待（供单元测试与需要兜底的调用方使用）。
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let millis = timeout.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        unsafe { WaitForSingleObject(self.handle, millis) == WAIT_OBJECT_0 }
    }
}

impl Drop for ShowMainWindowSignal {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

/// 请求已运行实例显示主窗口（第二个实例在退出前调用）。
///
/// 事件不存在时返回错误——调用方据此如实记录"请求未能送达"（例如对端是未带
/// 此功能的旧版本，或等待线程未能创建事件），而不是静默当作成功。
pub fn request_show_main_window() -> windows::core::Result<()> {
    request_named(SHOW_MAIN_WINDOW_EVENT_NAME)
}

fn request_named(name: &str) -> windows::core::Result<()> {
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

    /// 事件名是两端唯一契约；改名会让"二次启动显示主窗口"静默失效。
    #[test]
    fn event_name_is_stable() {
        assert_eq!(SHOW_MAIN_WINDOW_EVENT_NAME, r"Local\SayAll-ShowMainWindow");
    }

    /// 没有等待端时，请求必须返回错误，调用方才能如实记录请求未送达。
    /// 用带 `-absent` 后缀的名字：不得使用生产名，否则会置位本机正在运行的
    /// 应用（测试是把真实应用弹到前台的外部副作用）。
    #[test]
    fn requesting_without_a_listener_fails() {
        assert!(request_named(r"Local\SayAll-ShowMainWindow-absent").is_err());
    }

    #[test]
    fn request_is_observed_by_a_waiter_and_not_repeated() {
        const NAME: &str = r"Local\SayAll-ShowMainWindow-test-signal";
        let signal = ShowMainWindowSignal::create_named(NAME).expect("create named event");
        let setter = std::thread::spawn(move || request_named(NAME).expect("set named event"));
        assert!(signal.wait_timeout(Duration::from_secs(5)));
        setter.join().expect("setter thread");
        // 自动重置：消费一次后回到未置位，陈旧信号不会反复唤醒。
        assert!(!signal.wait_timeout(Duration::from_millis(50)));
    }
}
