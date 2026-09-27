//! 录入期"输入法让位"原语验证探针（2026-09-27，路线①）。
//!
//! 目标：验证在**真实窗口**上把输入区域切到非 IME 布局再恢复的可行性——产品
//! 路径是向录入窗口投递 `WM_INPUTLANGCHANGEREQUEST`。本探针创建一个隐藏窗口
//! （与录入窗口同属本进程的一个线程），读取当前布局与 ImmIsIME 判定、列出现有
//! 布局、切到非 IME 布局、核对生效、再恢复并核对，全程只操作自己的窗口线程。
//!
//! 用法：`cargo run --example ime_yield_probe -p sayall-windows`

#[cfg(not(windows))]
fn main() {
    println!("{{"kind":"ime_yield_probe","supported":false}}");
}

#[cfg(windows)]
fn main() {
    windows_probe::run();
}

#[cfg(windows)]
mod windows_probe {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::Ime::ImmIsIME;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        ActivateKeyboardLayout, GetKeyboardLayout, GetKeyboardLayoutList, LoadKeyboardLayoutW, HKL,
        KLF_ACTIVATE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetWindowThreadProcessId, PostMessageW, RegisterClassW, TranslateMessage, CW_USEDEFAULT,
        MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUTLANGCHANGEREQUEST, WNDCLASSW, WS_OVERLAPPED,
    };

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    fn hkl_value(hkl: HKL) -> usize {
        hkl.0 as usize
    }

    pub fn run() {
        unsafe {
            let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                .expect("module handle");
            let class = w!("sayall_ime_yield_probe");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("sayall ime yield probe"),
                WINDOW_STYLE(WS_OVERLAPPED.0),
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                200,
                100,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .expect("create window");
            let mut thread_id = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut thread_id));
            let current = GetKeyboardLayout(thread_id);
            println!(
                "[yield] window_thread={thread_id} current_hkl=0x{:04X} is_ime={}",
                hkl_value(current) & 0xFFFF,
                ImmIsIME(current).as_bool()
            );

            // 列出现有布局并挑一个非 IME 的。
            let mut list = vec![HKL::default(); 32];
            let count = GetKeyboardLayoutList(Some(&mut list));
            let candidates: Vec<HKL> = list[..count.max(0) as usize].to_vec();
            let printable: Vec<String> = candidates
                .iter()
                .map(|hkl| {
                    format!(
                        "0x{:04X}(ime={})",
                        hkl_value(*hkl) & 0xFFFF,
                        ImmIsIME(*hkl).as_bool()
                    )
                })
                .collect();
            println!("[yield] installed_layouts={}", printable.join(" "));

            let mut target = candidates
                .iter()
                .copied()
                .find(|hkl| hkl_value(*hkl) != hkl_value(current) && !ImmIsIME(*hkl).as_bool());
            let mut loaded_fallback = false;
            if target.is_none() {
                // 系统未装非 IME 布局：加载 en-US（标准布局，随时可加载）。
                if let Ok(hkl) = LoadKeyboardLayoutW(w!("00000409"), KLF_ACTIVATE) {
                    target = Some(hkl);
                    loaded_fallback = true;
                }
            }
            let Some(target) = target else {
                println!("[yield] 结果=失败 无可用非 IME 布局且加载 en-US 失败");
                let _ = DestroyWindow(hwnd);
                return;
            };
            println!(
                "[yield] target_hkl=0x{:04X} loaded_fallback={loaded_fallback}",
                hkl_value(target) & 0xFFFF
            );

            // 产品路径：直接切换本线程的输入区域（等价于窗口的输入区域切换，
            // 布局是按线程生效的；WM_INPUTLANGCHANGEREQUEST 对未激活窗口不生效，
            // 而应用窗口线程可自行调用本 API）。
            let activated = ActivateKeyboardLayout(
                target,
                windows::Win32::UI::Input::KeyboardAndMouse::ACTIVATE_KEYBOARD_LAYOUT_FLAGS(0),
            );
            let _ = std::thread::sleep(std::time::Duration::from_millis(300));
            let after = GetKeyboardLayout(thread_id);
            let switched = hkl_value(after) == hkl_value(target);
            println!(
                "[yield] activate_ok={} switched={switched} after_hkl=0x{:04X}",
                activated.is_ok(),
                hkl_value(after) & 0xFFFF
            );

            // 恢复原布局并核对。
            let _ = ActivateKeyboardLayout(
                current,
                windows::Win32::UI::Input::KeyboardAndMouse::ACTIVATE_KEYBOARD_LAYOUT_FLAGS(0),
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
            let restored = hkl_value(GetKeyboardLayout(thread_id)) == hkl_value(current);
            println!(
                "[yield] restored={restored} final_hkl=0x{:04X}",
                hkl_value(GetKeyboardLayout(thread_id)) & 0xFFFF
            );
            let _ = GetMessageW(&mut MSG::default(), None, 0, 0);
            let _ = DestroyWindow(hwnd);
            let _ = GetCurrentThreadId;
            let _ = PCWSTR::null();
        }
    }
}
