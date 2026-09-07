//! 预设快捷键注入链路真机验证探针（2026-09-06，用户要求逐项真机验证
//! 而非按理论屏蔽）。
//!
//! 验证目标：按键映射编辑器的每个预设和弦（基础按键/系统与媒体）以及
//! 自定义快捷键的代表和弦，经应用**实际注入管线**
//! （`SendInputRuntime::tap` → `plan_key_tap` → SendInput 批次）注入后，
//! OS 输入流（WH_KEYBOARD_LL 捕获，仅记录注入事件）收到与计划完全一致
//! 的事件序列：各键 DOWN 依序 + 逆序 UP、VK 正确。
//!
//! 焦点安全：注入目标统一为探针拉起的空白记事本（每组前经
//! `app_launcher::activate_or_launch("notepad")` 前台化），避免把
//! Ctrl+W/Ctrl+S/Ctrl+C 等发送进用户正在使用的窗口；有副作用的和弦
//! （保存→保存对话框、查找→查找条、截图→贴条、右键菜单→菜单、静音）
//! 验证后注入 Esc/还原组合清理。
//!
//! 跳过项：Win+L（锁定）不参与真机注入（会锁定会话且无法自动恢复）；
//! 其机制由同族 Win 和弦（显示桌面/搜索）覆盖，锁定效果留待用户单次
//! 人工确认。
//!
//! 运行：`cargo run -p sayall-windows --release --example preset_inject_probe`

#[cfg(windows)]
mod windows_impl {
    use sayall_windows::app_launcher;
    use sayall_windows::send_input::{plan_key_tap, KeyChord, KeyCode};
    use sayall_windows::send_input_windows::SendInputRuntime;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Mutex};
    use std::thread;
    use std::time::Duration;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
        UnhookWindowsHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, PM_NOREMOVE, WH_KEYBOARD_LL,
        WM_QUIT,
    };

    /// OS 捕获的注入事件（vkCode + DOWN/UP）。
    type Captured = Vec<(u32, bool)>;

    static CAPTURE: Mutex<Captured> = Mutex::new(Vec::new());
    static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);

    /// 一个待验证和弦：名称、键序、验证后的清理和弦（Esc 关对话框/贴条、
    /// 再按一次 Win+D 还原桌面、再静音一次恢复静音态）。
    struct ProbeCase {
        label: &'static str,
        keys: &'static [KeyCode],
        cleanup: Option<&'static [KeyCode]>,
        /// true = 只规划比对，不做真机注入（Win+L 锁定会话）。
        plan_only: bool,
    }

    use KeyCode as K;

    /// 用例顺序即安全顺序：Ctrl+字母族（对前台窗口有破坏性，必须落在
    /// 牺牲记事本上）全部排在焦点劫持者（Alt+Tab/Win+D/搜索/截图，均为
    /// 系统级效果、落点无关）之前；Ctrl+W 会关闭记事本，排在本组末尾，
    /// 之后由逐组激活逻辑重新拉起。
    const CASES: &[ProbeCase] = &[
        // —— Ctrl+字母族（焦点安全窗口期：记事本刚被前台化）——
        ProbeCase {
            label: "复制 Ctrl+C",
            keys: &[K::Control, K::C],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "粘贴 Ctrl+V",
            keys: &[K::Control, K::V],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "剪切 Ctrl+X",
            keys: &[K::Control, K::X],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "全选 Ctrl+A",
            keys: &[K::Control, K::A],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "撤销 Ctrl+Z",
            keys: &[K::Control, K::Z],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "重做 Ctrl+Y",
            keys: &[K::Control, K::Y],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "查找 Ctrl+F",
            keys: &[K::Control, K::F],
            // 记事本查找条：Esc 关闭。
            cleanup: Some(&[K::Escape]),
            plan_only: false,
        },
        ProbeCase {
            label: "保存 Ctrl+S",
            keys: &[K::Control, K::S],
            // 空白未命名文档弹出"另存为"对话框：Esc 取消。
            cleanup: Some(&[K::Escape]),
            plan_only: false,
        },
        ProbeCase {
            label: "发送 Ctrl+Enter",
            keys: &[K::Control, K::Enter],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "换行 Shift+Enter",
            keys: &[K::Shift, K::Enter],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "关闭窗口 Ctrl+W",
            keys: &[K::Control, K::W],
            // 记事本标签页关闭：下一组前会重新拉起记事本。
            cleanup: None,
            plan_only: false,
        },
        // —— 单键与导航（记事本重新拉起后执行；空文档内全部无害）——
        ProbeCase {
            label: "Enter",
            keys: &[K::Enter],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "Esc",
            keys: &[K::Escape],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "空格",
            keys: &[K::Space],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "Tab",
            keys: &[K::Tab],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "退格",
            keys: &[K::Backspace],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "删除",
            keys: &[K::Delete],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "↑",
            keys: &[K::Up],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "↓",
            keys: &[K::Down],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "←",
            keys: &[K::Left],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "→",
            keys: &[K::Right],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "Home",
            keys: &[K::Home],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "右键菜单 Apps",
            keys: &[K::Apps],
            cleanup: Some(&[K::Escape]),
            plan_only: false,
        },
        ProbeCase {
            label: "刷新 F5",
            keys: &[K::F5],
            cleanup: None,
            plan_only: false,
        },
        // —— 自定义快捷键代表（右 Alt 扫描码注入路径 + 三键和弦）——
        ProbeCase {
            label: "右Alt+空格（自定义代表）",
            keys: &[K::RightAlt, K::Space],
            // Alt+Space 打开记事本系统菜单：Esc 关闭。
            cleanup: Some(&[K::Escape]),
            plan_only: false,
        },
        ProbeCase {
            label: "Ctrl+Shift+O（自定义代表）",
            keys: &[K::Control, K::Shift, K::O],
            cleanup: None,
            plan_only: false,
        },
        // —— 媒体与音量（系统级效果，落点无关；音量+与−成对净零）——
        ProbeCase {
            label: "静音",
            keys: &[K::VolumeMute],
            // 再静音一次恢复原音量态。
            cleanup: Some(&[K::VolumeMute]),
            plan_only: false,
        },
        ProbeCase {
            label: "音量+",
            keys: &[K::VolumeUp],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "音量−",
            keys: &[K::VolumeDown],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "播放/暂停",
            keys: &[K::MediaPlayPause],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "上一首",
            keys: &[K::MediaPrev],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "下一首",
            keys: &[K::MediaNext],
            cleanup: None,
            plan_only: false,
        },
        // —— 焦点劫持者（系统级效果，落点无关；排在最后）——
        ProbeCase {
            label: "切换窗口 Alt+Tab",
            keys: &[K::Alt, K::Tab],
            cleanup: None,
            plan_only: false,
        },
        ProbeCase {
            label: "显示桌面 Win+D",
            keys: &[K::LeftWindows, K::D],
            // 再按一次 Win+D 还原桌面。
            cleanup: Some(&[K::LeftWindows, K::D]),
            plan_only: false,
        },
        ProbeCase {
            label: "搜索 Win+S",
            keys: &[K::LeftWindows, K::S],
            cleanup: Some(&[K::Escape]),
            plan_only: false,
        },
        ProbeCase {
            label: "截图 Win+Shift+S",
            keys: &[K::LeftWindows, K::Shift, K::S],
            // 截图贴条：Esc 关闭。
            cleanup: Some(&[K::Escape]),
            plan_only: false,
        },
        // —— 锁定：只规划比对，不做真机注入（会锁定会话）——
        ProbeCase {
            label: "锁定 Win+L",
            keys: &[K::LeftWindows, K::L],
            cleanup: None,
            plan_only: true,
        },
    ];

    fn clear_captured() {
        lock_capture().clear();
    }

    fn take_captured() -> Captured {
        std::mem::take(&mut *lock_capture())
    }

    fn lock_capture() -> std::sync::MutexGuard<'static, Captured> {
        CAPTURE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && HOOK_ACTIVE.load(Ordering::Relaxed) {
            let message = wparam.0 as u32;
            if matches!(message, 0x0100 | 0x0104 | 0x0101 | 0x0105) {
                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                if kb.flags.contains(LLKHF_INJECTED) {
                    let is_key_up = matches!(message, 0x0101 | 0x0105);
                    lock_capture().push((kb.vkCode as u32, is_key_up));
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// 钩子线程：先创建消息队列（WM_QUIT 必达，key_gate 同款修复），
    /// 挂 WH_KEYBOARD_LL 后进入消息泵。
    fn hook_thread(thread_id_tx: mpsc::Sender<u64>) {
        unsafe {
            let mut probe = MSG::default();
            let _ = PeekMessageW(&mut probe, None, 0, 0, PM_NOREMOVE);
            let instance: HINSTANCE = match GetModuleHandleW(None) {
                Ok(module) => module.into(),
                Err(_) => return,
            };
            let _ = thread_id_tx.send(GetCurrentThreadId() as u64);
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
                Ok(hook) => hook,
                Err(_) => return,
            };
            HOOK_ACTIVE.store(true, Ordering::Relaxed);
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                if message.message == WM_QUIT {
                    break;
                }
            }
            HOOK_ACTIVE.store(false, Ordering::Relaxed);
            let _ = UnhookWindowsHookEx(hook);
            let _ = instance;
        }
    }

    fn expected_sequence(keys: &[KeyCode]) -> Result<Captured, String> {
        let chord = KeyChord {
            keys: keys.to_vec(),
        };
        let plan = plan_key_tap(&chord).map_err(|error| error.to_string())?;
        Ok(plan
            .iter()
            .map(|event| (expected_vk(event.key), event.is_key_up))
            .collect())
    }

    /// 期望的 LL 层 VK：通用修饰键（Ctrl/Shift/Alt）以扫描码注入
    /// （物理身份=左手变体），Windows 在 LL 层合成为 VK_L*；其余键
    /// virtual_key() 直出（右 Alt/Win 等已实测一致）。
    fn expected_vk(key: KeyCode) -> u32 {
        match key {
            KeyCode::Control => 0xA2, // VK_LCONTROL
            KeyCode::Shift => 0xA0,   // VK_LSHIFT
            KeyCode::Alt => 0xA4,     // VK_LMENU
            other => other.virtual_key() as u32,
        }
    }

    pub fn main() {
        println!("=== 预设注入链路真机验证（焦点目标=空白记事本，请勿操作键盘鼠标）===");
        println!("=== 副作用预告：Alt+Tab 会切换前台窗口；媒体键作用于当前媒体会话；音量±成对执行净零 ===");
        let runtime = SendInputRuntime::new();
        let (thread_id_tx, thread_id_rx) = mpsc::channel();
        let hook_thread = thread::Builder::new()
            .name("probe-hook".to_owned())
            .spawn(move || hook_thread(thread_id_tx))
            .ok();
        let hook_thread_id = thread_id_rx.recv().unwrap_or(0);
        if hook_thread_id == 0 {
            eprintln!("钩子线程启动失败");
            std::process::exit(2);
        }
        // 等钩子真正生效。
        thread::sleep(Duration::from_millis(300));

        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;
        for case in CASES {
            let expected = match expected_sequence(case.keys) {
                Ok(expected) => expected,
                Err(error) => {
                    println!("FAIL  {:<28} 规划失败：{error}", case.label);
                    failed += 1;
                    continue;
                }
            };
            if case.plan_only {
                println!(
                    "SKIP  {:<28} 规划序列正确（{} 事件）；真机注入跳过：会锁定会话",
                    case.label,
                    expected.len()
                );
                skipped += 1;
                continue;
            }
            // 前台化空白记事本（激活或拉起），把注入吸收进牺牲目标。
            if let Err(error) = app_launcher::activate_or_launch("notepad") {
                println!("FAIL  {:<28} 记事本前台化失败：{error}", case.label);
                failed += 1;
                continue;
            }
            thread::sleep(Duration::from_millis(250));
            clear_captured();
            let chord = KeyChord {
                keys: case.keys.to_vec(),
            };
            if let Err(error) = runtime.tap(chord) {
                println!("FAIL  {:<28} 注入失败：{}", case.label, error.to_string());
                failed += 1;
                continue;
            }
            thread::sleep(Duration::from_millis(350));
            let captured = take_captured();
            if captured == expected {
                println!(
                    "PASS  {:<28} {} 事件序列与计划一致",
                    case.label,
                    captured.len()
                );
                passed += 1;
            } else {
                println!(
                    "FAIL  {:<28} 期望 {expected:?}，实际 {captured:?}",
                    case.label
                );
                failed += 1;
            }
            // 副作用清理（Esc 关对话框/贴条、还原桌面/静音态）。
            if let Some(cleanup) = case.cleanup {
                let _ = runtime.tap(KeyChord {
                    keys: cleanup.to_vec(),
                });
                thread::sleep(Duration::from_millis(200));
                clear_captured();
            }
        }

        // 收尾：关闭钩子线程。
        unsafe {
            let _ = PostThreadMessageW(hook_thread_id as u32, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(handle) = hook_thread {
            let _ = handle.join();
        }
        println!(
            "=== 结果：PASS {passed} / FAIL {failed} / SKIP {skipped}（共 {}）===",
            CASES.len()
        );
        std::process::exit(if failed == 0 { 0 } else { 1 });
    }
}

#[cfg(windows)]
fn main() {
    windows_impl::main();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("本探针仅支持 Windows。");
}
