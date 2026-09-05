//! 微信输入法（WeType）热键休眠的检测与自动唤醒。
//!
//! 背景（2026-09-05 真机日志实锤，sayall-gatt-live11 + kb-live）：
//! - TSF 全程报告 WeType 为当前会话活动输入法（激活无误），和弦注入干净
//!   （20ms 间隔、成对释放），但 WeType 的语音热键**不反应**（LWin 穿透、
//!   无 0xFC break key、麦克风不开）——其全局键盘钩子处于失效状态。
//! - 打开 WeType 自己的窗口（设置页）立即恢复（同一窗口期内的下一会话
//!   秒级触发、开麦正常）——窗口前台化使其进程解除后台状态、钩子复活。
//! - 机理推定：WeType 进程后台驻留被 Windows 节流（EcoQoS/定时器粗化），
//!   钩子回调超时被系统静默摘除——与 2026-09-05 本应用自身的节流问题
//!   （power.rs，F5 泄漏 559 次）同类，只是发生在第三方进程身上。
//!
//! 方案（第三方边界内，只用公开 API）：
//! - 检测：和弦注入后 ~700ms 读 ConsentStore 的 wetype 开麦时间戳，
//!   判断 WeType 是否真的响应了本次语音（WeType 内部状态我们无从观测，
//!   开麦是唯一的公开可观测判据——与整晚实验的 ground truth 一致）。
//! - 唤醒：未响应时，对 WeType 进程执行 SetProcessInformation(
//!   ProcessPowerThrottling, 禁用) —— 与本应用自身 power.rs 相同的公开
//!   API，作用对象换成同用户的 WeType 进程（OS 调度属性，非其内部配置/
//!   内存，属公开 API 边界）；相当于系统层面把它"叫醒"，无需打开其 UI。
//! - 护栏：全部尽力而为，失败不影响语音会话本身；结果全部落 gatt_note
//!   日志；二次复查仍无响应时在 last_error 给出人工提示（打开一次微信
//!   输入法任意界面）。

use windows::Win32::Foundation::{CloseHandle, WIN32_ERROR};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
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
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(subkey.as_ptr()),
            None,
            KEY_READ,
            &mut root,
        )
    };
    if opened.0 != 0 {
        return None;
    }
    let mut index: u32 = 0;
    loop {
        let mut name = [0u16; 260];
        let mut len = name.len() as u32;
        let enumerated = unsafe {
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
        };
        if enumerated.0 != 0 {
            break;
        }
        index += 1;
        let entry = String::from_utf16_lossy(&name[..len as usize]).to_lowercase();
        if !entry.contains("wetype") {
            continue;
        }
        // 找到 WeType 条目：打开并读 LastUsedTimeStart（QWORD）。
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

/// 对所有 WeType 进程禁用后台节流（公开 API；等同把被节流的进程"叫醒"）。
/// 返回逐进程结果描述（用于日志），失败不抛错。
pub fn unthrottle_wetype_processes() -> Vec<String> {
    let mut results = Vec::new();
    for pid in wetype_process_ids() {
        results.push(match unthrottle_process(pid) {
            Ok(()) => format!("pid={pid} ok"),
            Err(error) => format!("pid={pid} err={error}"),
        });
    }
    if results.is_empty() {
        results.push("no wetype process found".to_owned());
    }
    results
}

fn wetype_process_ids() -> Vec<u32> {
    let mut ids = Vec::new();
    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(handle) => handle,
        Err(_) => return ids,
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while ok {
        let name_len = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]).to_lowercase();
        if name.starts_with("wetype") {
            ids.push(entry.th32ProcessID);
        }
        ok = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    ids
}

fn unthrottle_process(pid: u32) -> Result<(), String> {
    use windows::Win32::System::Threading::{
        OpenProcess, SetProcessInformation, ProcessPowerThrottling,
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    };
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            false,
            pid,
        )
    }
    .map_err(|error| format!("open: {error}"))?;
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: 0,
    };
    let result = unsafe {
        SetProcessInformation(
            handle,
            ProcessPowerThrottling,
            &state as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    result.map_err(|error| format!("set: {error}"))
}

/// 防未使用导入告警的占位引用（WIN32_ERROR 在签名中隐性使用）。
#[allow(dead_code)]
fn _win32_error_used(_: WIN32_ERROR) {}
