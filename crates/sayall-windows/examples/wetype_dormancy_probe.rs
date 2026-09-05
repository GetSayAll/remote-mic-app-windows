//! WeType 休眠唤醒机制阶梯实测探针。
//!
//! 前提：WeType 处于休眠（ConsentStore wetype 时间戳长时间未动、和弦无反应）。
//! 依次测试候选唤醒机制，每步注入和弦并用 ConsentStore 判定是否开麦：
//!   A. TSF 配置切换（cycle profile）+ 1s 稳定 → 和弦
//!   B. 向 wetype_update 的顶层窗口 PostMessage(WM_NULL)（唤醒消息泵）→ 和弦
//!   C. SendMessageTimeout(WM_NULL)（强同步唤起线程）→ 和弦
//!   D. SetForegroundWindow 到 wetype_update 窗口（模拟用户交互激活）→ 和弦
//! 全部落结构化输出，供归档与决策。

use windows::core::GUID;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, TF_INPUTPROCESSORPROFILE, TF_IPPMF_FORSESSION,
    TF_IPP_FLAG_ENABLED, TF_PROFILETYPE_INPUTPROCESSOR,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
    SendMessageTimeoutW, SetForegroundWindow, SMTO_ABORTIFHUNG, WM_NULL,
};

const CLSID_TF_INPUT_PROCESSOR_PROFILES: GUID =
    GUID::from_u128(0x33c53a50_f456_4884_b049_85fd643e_cfed);
const WETYPE_CLSID: GUID = GUID::from_u128(0x86598fb9_66a2_463e_b9c2_aeb906d477ad);
const WETYPE_PROFILE: GUID = GUID::from_u128(0x607fdf85_fcc8_4dbd_a365_41296f980c9c);
const LANGID_ZH_CN: u16 = 0x0804;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let phase = args.get(1).map(String::as_str).unwrap_or("ladder");
    match phase {
        "cycle-test" => {
            // 健全性实验（钩子活着时）：cycle profile 后立即注入和弦，
            // 验证切换不破坏触发（v2 复活路径的前提）。
            println!("=== cycle-test (hook expected alive) ===");
            let before = mic_start();
            let r = cycle_profile();
            println!("cycle_profile: {r:?}");
            std::thread::sleep(std::time::Duration::from_millis(1000));
            inject_chord();
            std::thread::sleep(std::time::Duration::from_millis(900));
            let after = mic_start();
            println!("chord after cycle: mic_opened={}", opened(before, after));
        }
        _ => ladder(),
    }
}

fn ladder() {
    println!("=== WeType revive ladder probe ===");
    let before = mic_start();
    println!("baseline={before:?}");
    inject_chord();
    std::thread::sleep(std::time::Duration::from_millis(900));
    let mid = mic_start();
    let dormant = !opened(before, mid);
    println!(
        "step0 chord: mic_opened={} (dormant={dormant})",
        opened(before, mid)
    );
    if !dormant {
        println!("WeType not dormant; nothing to test");
        return;
    }

    // A. TSF 配置切换 + 1000ms 稳定。
    let r = cycle_profile();
    println!("A cycle_profile: {r:?}");
    std::thread::sleep(std::time::Duration::from_millis(1000));
    inject_chord();
    std::thread::sleep(std::time::Duration::from_millis(900));
    let a = mic_start();
    println!("A chord: mic_opened={}", opened(mid, a));
    if opened(mid, a) {
        println!(">>> WINNER: cycle_profile");
        return;
    }

    // B. PostMessage(WM_NULL) 到 wetype_update 的全部顶层窗口。
    let targets = wetype_update_windows();
    println!("B wetype_update windows: {} top-level", targets.len());
    for hwnd in &targets {
        unsafe {
            let _ = PostMessageW(Some(*hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    inject_chord();
    std::thread::sleep(std::time::Duration::from_millis(900));
    let b = mic_start();
    println!("B chord: mic_opened={}", opened(a, b));
    if opened(a, b) {
        println!(">>> WINNER: post_message");
        return;
    }

    // C. SendMessageTimeout（强同步，直接唤起目标线程泵消息）。
    for hwnd in &targets {
        unsafe {
            let mut result: usize = 0;
            let _ = SendMessageTimeoutW(
                *hwnd,
                WM_NULL,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                800,
                Some((&mut result) as *mut usize),
            );
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    inject_chord();
    std::thread::sleep(std::time::Duration::from_millis(900));
    let c = mic_start();
    println!("C chord: mic_opened={}", opened(b, c));
    if opened(b, c) {
        println!(">>> WINNER: send_message_timeout");
        return;
    }

    // D. SetForegroundWindow 到 wetype_update 窗口（AttachThreadInput 兜底）。
    if let Some(hwnd) = targets
        .iter()
        .find(|h| unsafe { IsWindowVisible(**h) }.as_bool())
    {
        unsafe {
            let prev = GetForegroundWindow();
            let prev_tid = GetWindowThreadProcessId(prev, None);
            let target_tid = GetWindowThreadProcessId(*hwnd, None);
            let me = GetCurrentThreadId();
            let _ = AttachThreadInput(me, prev_tid, true);
            let _ = AttachThreadInput(me, target_tid, true);
            let ok = SetForegroundWindow(*hwnd).as_bool();
            let _ = AttachThreadInput(me, target_tid, false);
            let _ = AttachThreadInput(me, prev_tid, false);
            println!("D set_foreground: ok={ok}");
            if ok {
                std::thread::sleep(std::time::Duration::from_millis(600));
                inject_chord();
                std::thread::sleep(std::time::Duration::from_millis(900));
                let d = mic_start();
                println!("D chord: mic_opened={}", opened(c, d));
                if opened(c, d) {
                    println!(">>> WINNER: set_foreground_window");
                    restore_foreground(prev);
                    return;
                }
                restore_foreground(prev);
            }
        }
    } else {
        println!("D skipped: no visible wetype_update window");
    }

    println!(">>> NO WINNER: all mechanisms failed");
}

fn restore_foreground(prev: HWND) {
    unsafe {
        let prev_tid = GetWindowThreadProcessId(prev, None);
        let me = GetCurrentThreadId();
        let _ = AttachThreadInput(me, prev_tid, true);
        let _ = SetForegroundWindow(prev);
        let _ = AttachThreadInput(me, prev_tid, false);
    }
}

fn opened(before: Option<u64>, after: Option<u64>) -> bool {
    match (before, after) {
        (Some(b), Some(a)) => a > b,
        (None, Some(_)) => true,
        _ => false,
    }
}

fn mic_start() -> Option<u64> {
    let script = r#"
$root = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged'
$found = $null
Get-ChildItem $root -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -match 'wetype' } | ForEach-Object {
    $v = (Get-ItemProperty $_.PSPath).LastUsedTimeStart
    if ($v -and -not $found) { $script:found = $v }
}
if ($found) { Write-Output $found } else { Write-Output -1 }
"#;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim()
        .parse::<i64>()
        .ok()
        .and_then(|v| if v > 0 { Some(v as u64) } else { None })
}

fn cycle_profile() -> Result<String, String> {
    // 与应用 ime.rs 相同的 STA 纪律：ActivateProfile 必须在 CoInitializeEx(STA)
    // 线程上调用（MTA 返回 S_OK 但不生效的陷阱，2026-09-05 实锤）。
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("probe-sta-cycle".to_owned())
        .spawn(move || {
            let result = sta_cycle_profile();
            let _ = sender.send(result);
        })
        .map_err(|e| format!("spawn: {e}"))?;
    receiver
        .recv_timeout(std::time::Duration::from_secs(3))
        .map_err(|_| "sta cycle timeout".to_owned())?
}

fn sta_cycle_profile() -> Result<String, String> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != windows::core::HRESULT(1) {
            return Err(format!("CoInitializeEx(STA) failed: {hr:?}"));
        }
        let result = (|| {
            let manager: ITfInputProcessorProfileMgr = CoCreateInstance(
                &CLSID_TF_INPUT_PROCESSOR_PROFILES,
                None,
                CLSCTX_INPROC_SERVER,
            )
            .map_err(|e| format!("CoCreateInstance: {e}"))?;
            let enumerator = manager
                .EnumProfiles(LANGID_ZH_CN)
                .map_err(|e| format!("EnumProfiles: {e}"))?;
            let mut profiles = [TF_INPUTPROCESSORPROFILE::default(); 16];
            let mut fetched: u32 = 0;
            let mut alternative: Option<(GUID, GUID)> = None;
            loop {
                if enumerator.Next(&mut profiles, &mut fetched).is_err() || fetched == 0 {
                    break;
                }
                for p in &profiles[..fetched as usize] {
                    if p.dwProfileType == TF_PROFILETYPE_INPUTPROCESSOR
                        && p.clsid != WETYPE_CLSID
                        && p.dwFlags & TF_IPP_FLAG_ENABLED != 0
                    {
                        alternative = Some((p.clsid, p.guidProfile));
                        break;
                    }
                }
                if alternative.is_some() || (fetched as usize) < profiles.len() {
                    break;
                }
            }
            let Some((alt_clsid, alt_profile)) = alternative else {
                return Err("no alternative profile".to_owned());
            };
            manager
                .ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    &alt_clsid,
                    &alt_profile,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                )
                .map_err(|e| format!("activate alt: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(80));
            manager
                .ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    &WETYPE_CLSID,
                    &WETYPE_PROFILE,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                )
                .map_err(|e| format!("activate wetype: {e}"))?;
            Ok(format!("switched via {:08X}", alt_clsid.data1))
        })();
        CoUninitialize();
        result
    }
}

struct EnumCtx {
    out: Vec<HWND>,
    target_pid: u32,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == ctx.target_pid {
        ctx.out.push(hwnd);
    }
    true.into()
}

fn wetype_update_windows() -> Vec<HWND> {
    let mut ctx = EnumCtx {
        out: Vec::new(),
        target_pid: wetype_update_pid().unwrap_or(0),
    };
    if ctx.target_pid == 0 {
        return ctx.out;
    }
    unsafe {
        let lparam = LPARAM(&mut ctx as *mut EnumCtx as isize);
        let _ = EnumWindows(Some(enum_proc), lparam);
    }
    ctx.out
}

fn wetype_update_pid() -> Option<u32> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-Process wetype_update -ErrorAction SilentlyContinue).Id",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim().parse::<u32>().ok()
}

fn inject_chord() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_TYPE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        KEYEVENTF_SCANCODE, VIRTUAL_KEY,
    };
    unsafe {
        let scan = |code: u16, up: bool, ext: bool| -> INPUT {
            let mut flags = KEYEVENTF_SCANCODE.0;
            if ext {
                flags |= 0x0001;
            }
            if up {
                flags |= KEYEVENTF_KEYUP.0;
            }
            INPUT {
                r#type: INPUT_TYPE(1),
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: code,
                        dwFlags: KEYBD_EVENT_FLAGS(flags),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }
        };
        let ctrl = scan(0x1D, false, false);
        if SendInput(&[ctrl], std::mem::size_of::<INPUT>() as i32) != 1 {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        let win = scan(0x5B, false, true);
        if SendInput(&[win], std::mem::size_of::<INPUT>() as i32) != 1 {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let win_up = scan(0x5B, true, true);
        let _ = SendInput(&[win_up], std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let ctrl_up = scan(0x1D, true, false);
        let _ = SendInput(&[ctrl_up], std::mem::size_of::<INPUT>() as i32);
        true
    }
}
