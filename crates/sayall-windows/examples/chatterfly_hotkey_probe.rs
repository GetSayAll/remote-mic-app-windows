//! Bounded public-API probe for Chatterfly's configured Left Ctrl + Left Win hotkey.
//! Focus a text field first, then run with `scan` or `vk`. Both keys are
//! released even if a DOWN submission fails. This does not access Chatterfly
//! internals or change persistent configuration.

use std::mem::size_of;
use std::time::Duration;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, VIRTUAL_KEY,
};

fn input(scan_mode: bool, win: bool, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS::default();
    if scan_mode {
        flags |= KEYEVENTF_SCANCODE;
    }
    if win {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: if scan_mode {
                    VIRTUAL_KEY(0)
                } else {
                    VIRTUAL_KEY(if win { 0x5B } else { 0xA2 })
                },
                wScan: if scan_mode {
                    if win {
                        0x5B
                    } else {
                        0x1D
                    }
                } else {
                    0
                },
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(event: INPUT) -> u32 {
    unsafe { SendInput(&[event], size_of::<INPUT>() as i32) }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or_default();
    let scan_mode = match mode {
        "scan" => true,
        "vk" => false,
        _ => {
            eprintln!("usage: chatterfly_hotkey_probe scan|vk");
            std::process::exit(2);
        }
    };
    let win_first = args.get(2).is_some_and(|value| value == "win-first");
    let gap_ms: u64 = args
        .get(3)
        .and_then(|value| value.parse().ok())
        .unwrap_or(80);
    if gap_ms > 500 {
        eprintln!("gap must be at most 500 ms");
        std::process::exit(2);
    }
    println!("mode={mode} win_first={win_first} gap_ms={gap_ms} starting in 2 seconds");
    std::thread::sleep(Duration::from_secs(2));
    let first_down = send(input(scan_mode, win_first, false));
    std::thread::sleep(Duration::from_millis(gap_ms));
    let second_down = if first_down == 1 {
        send(input(scan_mode, !win_first, false))
    } else {
        0
    };
    std::thread::sleep(Duration::from_millis(1200));
    let second_up = send(input(scan_mode, !win_first, true));
    std::thread::sleep(Duration::from_millis(gap_ms));
    let first_up = send(input(scan_mode, win_first, true));
    println!("mode={mode} first_down={first_down} second_down={second_down} second_up={second_up} first_up={first_up}");
}
