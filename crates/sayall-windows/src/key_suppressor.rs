//! 按住说话语音键抑制器。
//!
//! 背景（2026-09-04 调查实锤，Testing\investigation\remote-capture.log 取证）：
//! 遥控器语音键在 HID 键盘层是 F5（VK 0x74），按住期间 F5 处于按下状态；此时
//! 注入 左Ctrl+左Win 会被微信输入法判定为三键同按（无效和弦）而不触发语音。
//! 参考实现（ZSTDJan / Voice_VibeCoding）均以低级键盘钩子吞掉遥控器原始 F5
//! 解决此问题（VVC 的"F5 状态机"）。
//!
//! 结构（2026-09-04 加固后，双线程 + 钩子链头 bump）：
//! - **钩子线程**：常驻 WH_KEYBOARD_LL 钩子（专职消息泵）。吞键判定只针对
//!   F5，其余按键一律透传；武装条件（其一）：ATVV 语音会话进行中
//!   （`set_session_active`，BLE 工作线程调用），或 Raw Input 在武装宽限
//!   （250ms）内观察到来自遥控器（vid_2717/pid_32b8 设备族）的 F5；首个
//!   F5 在回调内有界等待 60ms 等任一武装信号（物理 F5 最坏 +60ms 延迟，
//!   ZSTDJan 同款取舍）。
//! - **Raw Input 线程**：独立消息窗口 + RIDEV_INPUTSINK，只做设备归因并刷
//!   新武装时戳。独立线程是必须的：LL 钩子回调在钩子线程内执行，回调内的
//!   有界等待期间同线程消息泵无法分发 WM_INPUT（单线程设计会自阻塞，2026-09-04
//!   实测归因永不生效——见 Bugs\2026-09-04-wetype-zero-gap-injection.md）。
//! - **钩子链头 bump**（VVC 技巧：先挂新钩再卸旧钩，无吞键空窗）：LL 钩子
//!   按"最新安装在最前"的顺序调用；若微信输入法等目标在本应用之后（重）
//!   安装了自己的 LL 钩子，其和弦判定会先于本抑制器看到遥控器 F5，导致
//!   和弦被"额外按键"拒绝。每次语音会话开始（`set_session_active(true)`）
//!   与每 10 秒定时器都把本钩子重新安装到链头，保证 F5 在到达任何目标钩子
//!   之前先被吞掉。
//! - 会话结束保留 250ms 宽限，覆盖 BLE 通知晚到的物理 F5 释放沿。
//!
//! 护栏：回调内只读原子状态 + 短睡眠轮询，无 IO/锁；线程退出时卸钩销窗。

#[cfg(windows)]
mod windows_impl {
    use crate::raw_input::device_path_matches_xiaomi_remote;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::sync::OnceLock;
    use std::thread::JoinHandle;
    use std::time::Instant;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::{
        GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, RAWINPUTDEVICE,
        RIDEV_INPUTSINK, RIDEV_REMOVE, RIDI_DEVICENAME, RID_INPUT, RIM_TYPEKEYBOARD,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        GetMessageW, PostThreadMessageW, RegisterClassW, SetTimer, SetWindowsHookExW,
        TranslateMessage, UnhookWindowsHookEx, UnregisterClassW, HHOOK, HWND_MESSAGE,
        KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_INPUT,
        WM_QUIT, WM_TIMER, WNDCLASSW,
    };

    const ARM_GRACE_MS: u64 = 250;
    const BOUNDED_WAIT_MS: u64 = 60;
    const VK_F5: u32 = 0x74;
    /// 链头 bump 的线程消息（WM_APP 私有区）。
    const WM_HOOK_BUMP: u32 = WM_APP + 0x50;
    const BUMP_TIMER_ID: usize = 0x5A11;
    const BUMP_TIMER_MS: u32 = 10_000;

    static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
    static ARMED_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
    static SWALLOW_MASTER: AtomicBool = AtomicBool::new(false);
    static CLOCK_BASE: OnceLock<Instant> = OnceLock::new();
    static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

    fn now_ms() -> u64 {
        CLOCK_BASE.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    fn armed() -> bool {
        let until = ARMED_UNTIL_MS.load(Ordering::Relaxed);
        until != 0 && now_ms() < until
    }

    fn session_active() -> bool {
        SESSION_ACTIVE.load(Ordering::Relaxed)
    }

    fn swallow_ready() -> bool {
        session_active() || armed()
    }

    /// 纯决策函数：给定状态与按键，是否吞键（单元测试覆盖）。
    pub fn decide(vk_code: u32, is_key_up: bool, session: bool, armed_now: bool) -> bool {
        if vk_code != VK_F5 {
            return false;
        }
        let _ = is_key_up;
        session || armed_now
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && SWALLOW_MASTER.load(Ordering::Relaxed) {
            // WM_KEYDOWN=0x0100 / WM_SYSKEYDOWN=0x0104 / WM_KEYUP=0x0101 / WM_SYSKEYUP=0x0105
            let message = wparam.0 as u32;
            if matches!(message, 0x0100 | 0x0104 | 0x0101 | 0x0105) {
                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                if kb.vkCode == VK_F5 {
                    if swallow_ready() {
                        return LRESULT(1);
                    }
                    // 首个沿：有界等待武装信号（BLE 会话标志或 Raw Input 设备归因）。
                    let deadline = now_ms() + BOUNDED_WAIT_MS;
                    while now_ms() < deadline {
                        if swallow_ready() {
                            return LRESULT(1);
                        }
                        std::thread::sleep(std::time::Duration::from_millis(2));
                    }
                    return CallNextHookEx(None, code, wparam, lparam);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// 钩子链头 bump：先挂新钩（立即成为链头），再卸旧钩——重叠安装无吞键空窗
    /// （Voice_VibeCoding 同款技巧）。新钩安装失败时保留旧钩。
    fn bump_to_chain_head(current: &mut Option<HHOOK>) {
        if let Ok(new_hook) =
            (unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) })
        {
            let old = current.replace(new_hook);
            if let Some(old) = old {
                unsafe {
                    let _ = UnhookWindowsHookEx(old);
                }
            }
        }
    }

    unsafe fn device_name_of(device: HANDLE) -> Option<String> {
        let mut size = 0u32;
        if GetRawInputDeviceInfoW(Some(device), RIDI_DEVICENAME, None, &mut size) == 0 || size == 0
        {
            return None;
        }
        let mut buffer = vec![0u16; size as usize];
        let written = GetRawInputDeviceInfoW(
            Some(device),
            RIDI_DEVICENAME,
            Some(buffer.as_mut_ptr().cast()),
            &mut size,
        );
        if written == 0 {
            return None;
        }
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..end]))
    }

    /// x64 下 RAWINPUTHEADER 为 24 字节：dwType@0、dwSize@4、hDevice@8、wParam@16；
    /// RAWKEYBOARD 紧随其后：MakeCode@24、Flags@26、(Reserved@28)、VKey@30。
    fn arm_from_raw_input(bytes: &[u8]) {
        if bytes.len() < 32 {
            return;
        }
        let dw_type = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if dw_type != RIM_TYPEKEYBOARD.0 as u32 {
            return;
        }
        let vkey = u16::from_le_bytes([bytes[30], bytes[31]]);
        if vkey as u32 != VK_F5 {
            return;
        }
        let device = HANDLE(u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]) as *mut core::ffi::c_void);
        let Some(path) = (unsafe { device_name_of(device) }) else {
            return;
        };
        if device_path_matches_xiaomi_remote(&path) {
            ARMED_UNTIL_MS.store(now_ms() + ARM_GRACE_MS, Ordering::Relaxed);
        }
    }

    // ---- Raw Input 归因线程（独立消息窗口 + 消息泵） ----

    unsafe extern "system" fn raw_wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_INPUT {
            let handle = windows::Win32::UI::Input::HRAWINPUT(lparam.0 as *mut _);
            let mut size = 0u32;
            if GetRawInputData(handle, RID_INPUT, None, &mut size, 24) != u32::MAX
                && size > 0
                && size <= 4096
            {
                let mut bytes = vec![0u8; size as usize];
                if GetRawInputData(
                    handle,
                    RID_INPUT,
                    Some(bytes.as_mut_ptr().cast()),
                    &mut size,
                    24,
                ) != u32::MAX
                {
                    arm_from_raw_input(&bytes);
                }
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }

    fn raw_input_thread(thread_id_tx: mpsc::Sender<u32>) {
        unsafe {
            let instance: HINSTANCE = match GetModuleHandleW(None) {
                Ok(module) => module.into(),
                Err(_) => return,
            };
            let _ = thread_id_tx.send(GetCurrentThreadId());

            let class_name: Vec<u16> = "SayAllVoiceKeySuppressorRaw\0"
                .encode_utf16()
                .collect();
            let class_name_ptr = PCWSTR(class_name.as_ptr());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(raw_wnd_proc),
                lpszClassName: class_name_ptr,
                hInstance: instance,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return;
            }
            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name_ptr,
                class_name_ptr,
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance),
                None,
            ) {
                Ok(hwnd) => hwnd,
                Err(_) => {
                    let _ = UnregisterClassW(class_name_ptr, Some(instance));
                    return;
                }
            };
            let keyboard_usage = RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: 6,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            };
            if RegisterRawInputDevices(
                &[keyboard_usage],
                std::mem::size_of::<RAWINPUTDEVICE>() as u32,
            )
            .is_err()
            {
                let _ = DestroyWindow(hwnd);
                let _ = UnregisterClassW(class_name_ptr, Some(instance));
                return;
            }

            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            let removals = [RAWINPUTDEVICE {
                usUsagePage: 1,
                usUsage: 6,
                dwFlags: RIDEV_REMOVE,
                hwndTarget: HWND::default(),
            }];
            let _ = RegisterRawInputDevices(
                &removals,
                std::mem::size_of::<RAWINPUTDEVICE>() as u32,
            );
            let _ = DestroyWindow(hwnd);
            let _ = UnregisterClassW(class_name_ptr, Some(instance));
        }
    }

    // ---- 钩子线程（LL 钩子 + 消息泵 + bump 消息/定时器） ----

    fn hook_thread(thread_id_tx: mpsc::Sender<u32>) {
        unsafe {
            let instance: HINSTANCE = match GetModuleHandleW(None) {
                Ok(module) => module.into(),
                Err(_) => return,
            };
            let _ = thread_id_tx.send(GetCurrentThreadId());
            let _ = CLOCK_BASE.get_or_init(Instant::now);

            let mut current: Option<HHOOK> = None;
            bump_to_chain_head(&mut current);
            if current.is_none() {
                return;
            }
            HOOK_THREAD_ID.store(GetCurrentThreadId(), Ordering::Relaxed);
            SetTimer(None, BUMP_TIMER_ID, BUMP_TIMER_MS, None);
            SWALLOW_MASTER.store(true, Ordering::Relaxed);

            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                match message.message {
                    WM_QUIT => break,
                    WM_HOOK_BUMP => bump_to_chain_head(&mut current),
                    WM_TIMER if message.wParam.0 as usize == BUMP_TIMER_ID => {
                        bump_to_chain_head(&mut current)
                    }
                    _ => {}
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }

            SWALLOW_MASTER.store(false, Ordering::Relaxed);
            HOOK_THREAD_ID.store(0, Ordering::Relaxed);
            if let Some(hook) = current.take() {
                let _ = UnhookWindowsHookEx(hook);
            }
            let _ = instance;
        }
    }

    /// 语音键抑制器句柄：持有即运行，丢弃即停止。会话武装走模块级
    /// [`set_session_active`]（BLE 工作线程直接调用，无需传递句柄）。
    #[derive(Debug)]
    pub struct VoiceKeySuppressor {
        worker: Option<JoinHandle<()>>,
        raw_worker: Option<JoinHandle<()>>,
        thread_id: u32,
        raw_thread_id: u32,
    }

    impl VoiceKeySuppressor {
        /// 启动抑制线程（钩子 + Raw Input 归因 + 消息泵）。
        pub fn start() -> VoiceKeySuppressor {
            SESSION_ACTIVE.store(false, Ordering::Relaxed);
            ARMED_UNTIL_MS.store(0, Ordering::Relaxed);
            let (thread_id_tx, thread_id_rx) = mpsc::channel();
            let worker = std::thread::Builder::new()
                .name("sayall-voice-key-suppressor".to_owned())
                .spawn(move || hook_thread(thread_id_tx))
                .ok();
            let thread_id = thread_id_rx.recv().unwrap_or(0);
            // Raw Input 归因线程为尽力而为：失败仅失去"首个 F5 早于会话激活"的
            // 提前吞键窗口，会话信号路径不受影响。
            let (raw_tx, raw_rx) = mpsc::channel();
            let raw_worker = std::thread::Builder::new()
                .name("sayall-voice-key-raw".to_owned())
                .spawn(move || raw_input_thread(raw_tx))
                .ok();
            let raw_thread_id = raw_rx.recv().unwrap_or(0);
            VoiceKeySuppressor {
                worker,
                raw_worker,
                thread_id,
                raw_thread_id,
            }
        }

        /// ATVV 语音会话起止（等价模块级 [`set_session_active`]）。
        pub fn set_session_active(&self, active: bool) {
            set_session_active(active);
        }
    }

    /// ATVV 语音会话起止（模块级，供 BleRuntime 工作线程调用）：
    /// 会话期间吞 F5；结束时保留 250ms 宽限覆盖晚到的释放沿。
    /// 会话开始同时请求钩子链头 bump——微信输入法等目标若在本应用之后
    /// 安装了自己的 LL 钩子，bump 保证本抑制器先于目标看到遥控器 F5。
    pub fn set_session_active(active: bool) {
        if active {
            SESSION_ACTIVE.store(true, Ordering::Relaxed);
            let thread_id = HOOK_THREAD_ID.load(Ordering::Relaxed);
            if thread_id != 0 {
                unsafe {
                    let _ =
                        PostThreadMessageW(thread_id, WM_HOOK_BUMP, WPARAM(0), LPARAM(0));
                }
            }
        } else {
            SESSION_ACTIVE.store(false, Ordering::Relaxed);
            ARMED_UNTIL_MS.store(now_ms() + ARM_GRACE_MS, Ordering::Relaxed);
        }
    }

    impl Drop for VoiceKeySuppressor {
        fn drop(&mut self) {
            SWALLOW_MASTER.store(false, Ordering::Relaxed);
            SESSION_ACTIVE.store(false, Ordering::Relaxed);
            HOOK_THREAD_ID.store(0, Ordering::Relaxed);
            if self.thread_id != 0 {
                unsafe {
                    let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
                }
            }
            if self.raw_thread_id != 0 {
                unsafe {
                    let _ =
                        PostThreadMessageW(self.raw_thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
                }
            }
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
            if let Some(worker) = self.raw_worker.take() {
                let _ = worker.join();
            }
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{decide, set_session_active, VoiceKeySuppressor};

#[cfg(test)]
mod tests {
    #[test]
    fn only_remote_or_session_f5_is_swallowed() {
        // 决策函数：非 F5 一律透传；F5 仅在会话或武装时吞（按下/释放同规则，
        // 保证被吞下的按沿对应的释放沿同样被吞，不留粘键）。
        assert!(!super::decide(0x41, false, false, false));
        assert!(!super::decide(0x41, true, true, true));
        assert!(!super::decide(0x74, false, false, false));
        assert!(super::decide(0x74, false, true, false));
        assert!(super::decide(0x74, false, false, true));
        assert!(super::decide(0x74, true, true, false));
        assert!(super::decide(0x74, true, false, true));
    }
}
