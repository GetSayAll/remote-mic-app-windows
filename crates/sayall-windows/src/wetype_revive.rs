//! ConsentStore 微信输入法（WeType）开麦判据读取。
//!
//! 用途（2026-09-05 WeType 热键休眠调查）：WeType 内部状态不可观测，
//! ConsentStore 的 LastUsedTimeStart 时间戳是"微信输入法真的响应并打开了
//! 麦克风"的唯一公开判据——与整晚持锁实验的 ground truth 相同。
//! 供 ble.rs 的 wetype_check（热键休眠检测与自动恢复）使用。
//!
//! 历史注记：曾实现"对 WeType 进程解除后台节流"作为唤醒手段，真机实证
//! 跨进程 SetProcessInformation(ProcessPowerThrottling) 返回 E_INVALIDARG
//! （不支持作用于其他进程），方案已由 ime.rs 的 TSF 配置切换唤醒取代。

use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    REG_VALUE_TYPE,
};

/// ConsentStore microphone\NonPackaged 根键。
const CONSENT_NONPACKAGED: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone\\NonPackaged";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// 读取 WeType 的最近开麦时间戳（原始 FILETIME，100ns 单位）。
/// 未找到 WeType 条目或读取失败返回 None（调用方按"无法判定"处理）。
pub fn wetype_mic_start() -> Option<u64> {
    let subkey = wide(CONSENT_NONPACKAGED);
    let mut root = HKEY::default();
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(subkey.as_ptr()),
            None,
            KEY_READ,
            &mut root,
        )
    }
    .0
        != 0
    {
        return None;
    }
    let mut index: u32 = 0;
    loop {
        let mut name = [0u16; 260];
        let mut len = name.len() as u32;
        if unsafe {
            RegEnumKeyExW(
                root,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        }
        .0
            != 0
        {
            break;
        }
        index += 1;
        let entry = String::from_utf16_lossy(&name[..len as usize]).to_lowercase();
        if !entry.contains("wetype") {
            continue;
        }
        let mut key = HKEY::default();
        let entry_wide = wide(&entry);
        if unsafe {
            RegOpenKeyExW(
                root,
                windows::core::PCWSTR(entry_wide.as_ptr()),
                None,
                KEY_READ,
                &mut key,
            )
        }
        .0
            != 0
        {
            continue;
        }
        let value = wide("LastUsedTimeStart");
        let mut data: u64 = 0;
        let mut size = std::mem::size_of::<u64>() as u32;
        let mut kind = REG_VALUE_TYPE::default();
        let queried = unsafe {
            RegQueryValueExW(
                key,
                windows::core::PCWSTR(value.as_ptr()),
                None,
                Some(&mut kind),
                Some((&mut data as *mut u64).cast()),
                Some(&mut size),
            )
        };
        unsafe {
            let _ = RegCloseKey(key);
        }
        if queried.0 == 0 {
            unsafe {
                let _ = RegCloseKey(root);
            }
            return Some(data);
        }
    }
    unsafe {
        let _ = RegCloseKey(root);
    }
    None
}