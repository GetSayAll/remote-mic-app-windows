//! Bounded public-API probe for Chatterfly's configured modifier-only hotkey.
//! Focus a text field first, then run with `scan|vk ctrl-win|ctrl-alt`. Both keys are
//! released even if a DOWN submission fails. This does not access Chatterfly
//! internals or change persistent configuration.

use std::mem::size_of;
use std::time::Duration;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, VIRTUAL_KEY,
};

#[derive(Clone, Copy)]
enum Key {
    Control,
    Alt,
    Win,
}

impl Key {
    fn virtual_key(self) -> u16 {
        match self {
            Self::Control => 0xA2,
            Self::Alt => 0xA4,
            Self::Win => 0x5B,
        }
    }

    fn scan_code(self) -> u16 {
        match self {
            Self::Control => 0x1D,
            Self::Alt => 0x38,
            Self::Win => 0x5B,
        }
    }
}

fn input(scan_mode: bool, key: Key, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS::default();
    if scan_mode {
        flags |= KEYEVENTF_SCANCODE;
    }
    if matches!(key, Key::Win) {
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
                    VIRTUAL_KEY(key.virtual_key())
                },
                wScan: if scan_mode { key.scan_code() } else { 0 },
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
            eprintln!(
                "usage: chatterfly_hotkey_probe scan|vk ctrl-win|ctrl-alt [second-first] [gap-ms]"
            );
            std::process::exit(2);
        }
    };
    let second_key = match args.get(2).map(String::as_str) {
        Some("ctrl-win") => Key::Win,
        Some("ctrl-alt") => Key::Alt,
        _ => {
            eprintln!("second argument must be ctrl-win or ctrl-alt");
            std::process::exit(2);
        }
    };
    let second_first = args.get(3).is_some_and(|value| value == "second-first");
    let gap_ms: u64 = args
        .get(4)
        .and_then(|value| value.parse().ok())
        .unwrap_or(80);
    if gap_ms > 500 {
        eprintln!("gap must be at most 500 ms");
        std::process::exit(2);
    }
    println!("mode={mode} second_first={second_first} gap_ms={gap_ms} starting in 2 seconds");
    std::thread::sleep(Duration::from_secs(2));
    let first_key = if second_first {
        second_key
    } else {
        Key::Control
    };
    let final_key = if second_first {
        Key::Control
    } else {
        second_key
    };
    let first_down = send(input(scan_mode, first_key, false));
    std::thread::sleep(Duration::from_millis(gap_ms));
    let second_down = if first_down == 1 {
        send(input(scan_mode, final_key, false))
    } else {
        0
    };
    std::thread::sleep(Duration::from_millis(1200));
    let second_up = send(input(scan_mode, final_key, true));
    std::thread::sleep(Duration::from_millis(gap_ms));
    let first_up = send(input(scan_mode, first_key, true));
    println!("mode={mode} first_down={first_down} second_down={second_down} second_up={second_up} first_up={first_up}");
}
