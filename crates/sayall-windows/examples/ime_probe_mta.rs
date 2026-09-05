//! MTA-thread validation probe for the WeType session-activation path.
//!
//! Reproduces the app's exact environment: RoInitialize(MULTITHREADED) (= COM
//! MTA) on the calling thread, then TSF ActivateProfile + immediate chord
//! injection, with the ConsentStore mic-open timestamp as the judgement.
//! The original experiments validated this path on an STA (PowerShell) thread;
//! deployed-app logs on 2026-09-05 showed sessions with clean chords but no
//! WeType reaction, suggesting the MTA call may not take effect.
//!
//! Usage: cargo run --release -p sayall-windows --example ime_probe_mta

use windows::core::GUID;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, TF_IPPMF_FORSESSION, TF_PROFILETYPE_INPUTPROCESSOR,
};

const CLSID_TF_INPUT_PROCESSOR_PROFILES: GUID =
    GUID::from_u128(0x33c53a50_f456_4884_b049_85fd643e_cfed);
const WETYPE_CLSID: GUID = GUID::from_u128(0x86598fb9_66a2_463e_b9c2_aeb906d477ad);
const WETYPE_PROFILE: GUID = GUID::from_u128(0x607fdf85_fcc8_4dbd_a365_41296f980c9c);
const MS_PINYIN_CLSID: GUID = GUID::from_u128(0x81d4e9c9_1d3b_41bc_9e6c_4b40bf79e35e);
const MS_PINYIN_PROFILE: GUID = GUID::from_u128(0xfa550b04_5ad7_411f_a5ac_ca038ec515d7);
const LANGID_ZH_CN: u16 = 0x0804;

fn activate(
    manager: &ITfInputProcessorProfileMgr,
    clsid: &GUID,
    profile: &GUID,
) -> windows::core::Result<()> {
    unsafe {
        manager.ActivateProfile(
            TF_PROFILETYPE_INPUTPROCESSOR,
            LANGID_ZH_CN,
            clsid,
            profile,
            HKL::default(),
            TF_IPPMF_FORSESSION,
        )
    }
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
        if SendInput(&[ctrl], size_of::<INPUT>() as i32) != 1 {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        let win = scan(0x5B, false, true);
        if SendInput(&[win], size_of::<INPUT>() as i32) != 1 {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let win_up = scan(0x5B, true, true);
        let _ = SendInput(&[win_up], size_of::<INPUT>() as i32);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let ctrl_up = scan(0x1D, true, false);
        let _ = SendInput(&[ctrl_up], size_of::<INPUT>() as i32);
        true
    }
}

fn main() {
    // Trial B (the fix path): both IME switches on short-lived STA threads.
    // MTA ActivateProfile returns S_OK but has no effect (proven by trial A
    // earlier on 2026-09-05); an STA thread is required.
    let switch = |clsid: GUID, profile: GUID, label: &str| -> Result<(), String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("ime-sta-probe".to_owned())
            .spawn(move || {
                let outcome = sta_activate(&clsid, &profile);
                let _ = tx.send(outcome);
            })
            .map_err(|error| format!("spawn failed: {error}"));
        match spawned {
            Ok(_) => match rx.recv_timeout(std::time::Duration::from_millis(2000)) {
                Ok(result) => result,
                Err(_) => Err(format!("{label} timed out")),
            },
            Err(error) => Err(error),
        }
    };

    // 1) Wrong IME first (STA thread).
    match switch(MS_PINYIN_CLSID, MS_PINYIN_PROFILE, "MsPinyin") {
        Ok(()) => println!("STA-thread activate(MsPinyin): OK"),
        Err(error) => println!("STA-thread activate(MsPinyin) FAILED: {error}"),
    }
    std::thread::sleep(std::time::Duration::from_millis(800));

    // 2) WeType activation on an STA thread (the fix), zero delay to chord.
    match switch(WETYPE_CLSID, WETYPE_PROFILE, "WeType") {
        Ok(()) => println!("STA-thread activate(WeType): OK"),
        Err(error) => println!("STA-thread activate(WeType) FAILED: {error}"),
    }

    // 3) Inject the chord immediately.
    let ok = inject_chord();
    println!("chord injected: {ok}");

    // 4) Judgement: check whether the mic opened (ConsentStore timestamp).
    println!("probe done - check ConsentStore for a fresh mic open");
}

/// 在临时 STA 线程上执行 TSF 会话级激活：CoInitializeEx(STA) →
/// CoCreateInstance → ActivateProfile → CoUninitialize。全部调用都在本线程
/// 内完成（无跨套间封送，无需消息泵）。
fn sta_activate(clsid: &GUID, profile: &GUID) -> Result<(), String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // S_OK(0) 或 S_FALSE(1)（已初始化）都算成功。
        if hr.is_err() && hr != windows::core::HRESULT(1) {
            return Err(format!("CoInitializeEx(STA) 失败：{hr:?}"));
        }
        let result = (|| {
            let manager: ITfInputProcessorProfileMgr = CoCreateInstance(
                &CLSID_TF_INPUT_PROCESSOR_PROFILES,
                None,
                CLSCTX_INPROC_SERVER,
            )
            .map_err(|error| format!("创建 TSF 配置管理器失败：{error}"))?;
            manager
                .ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    clsid,
                    profile,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                )
                .map_err(|error| format!("激活失败：{error}"))
        })();
        CoUninitialize();
        result
    }
}
