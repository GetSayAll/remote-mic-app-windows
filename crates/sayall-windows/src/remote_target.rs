//! 「应用正在用的那支遥控器」的单一来源（2026-10-13 现场）。
//!
//! 现场：同一台电脑上同时有两支同型号遥控器在线时，两条链路**都**拒绝盲选——
//! Raw Input 枚举到多个候选直接判失败（`DevicePathError::Ambiguous`），增强捕获
//! 助手也因「定位到多个候选宿主」退出（助手自检 note 里点名要 `--target-pid`）。
//! 结果是普通按键与「全按键支持」全断、界面没有任何解释；用户以为是自己刚更新的
//! 版本坏了（2026-10-13 报障原话）。
//!
//! 但应用**自己明确知道**正在用哪一支（BLE 会话持有的 `BluetoothLEDevice`），
//! 所以歧义是可消解的：把它的蓝牙地址发布给两个消费者——
//! 1. 进程内：Raw Input 绑定与审计按地址过滤候选（[`matches_address`]）；
//! 2. 进程外：提权助手读 [`TARGET_FILE_NAME`] 指向的文件，在多个候选宿主之间
//!    选出属于这支遥控器的那一个（助手侧同源实现，格式见 [`file_body`]）。
//!
//! 隐私：地址**不进日志**（日志只记"有没有值"与结果），文件落在
//! `%LOCALAPPDATA%\SayAll\`——与桥接描述文件同目录、同可见范围，不进仓库、
//! 不进用户可见文案、不进诊断摘要。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 目标文件：与桥接描述文件同目录（`%LOCALAPPDATA%\SayAll`）。
pub const TARGET_FILE_NAME: &str = "rc003-target.ini";

/// 0 = 未知（没有过滤条件，保持旧行为：唯一候选才绑定、多候选仍拒绝）。
static TARGET_ADDRESS: AtomicU64 = AtomicU64::new(0);

/// 发布/清除「正在用的遥控器」。`None` 或 0 = 未知。
pub fn set(address: Option<u64>) {
    let next = address.filter(|value| *value != 0).unwrap_or(0);
    let previous = TARGET_ADDRESS.swap(next, Ordering::Relaxed);
    if previous == next {
        return;
    }
    // 只记"有没有值"——地址本身是设备身份，不进日志（LOGGING.md 隐私红线）。
    note(format!(
        "remote_target action={} source=ble_session",
        if next == 0 { "cleared" } else { "published" }
    ));
    match write_file(next) {
        Ok(()) => {}
        Err(error) => note(format!(
            "remote_target action=write phase=completed terminal_result=failed reason={error}"
        )),
    }
}

/// 诊断日志（与其它平台模块同一个载体；非 Windows 上没有该载体，直接丢弃）。
fn note(message: String) {
    #[cfg(windows)]
    crate::ble::gatt_note(message);
    #[cfg(not(windows))]
    let _ = message;
}

/// 当前目标（`None` = 未知）。
pub fn get() -> Option<u64> {
    match TARGET_ADDRESS.load(Ordering::Relaxed) {
        0 => None,
        value => Some(value),
    }
}

/// 地址的 12 位十六进制小写形态——与 Raw Input 设备路径里的写法一致
/// （`…_dev_vid&012717_pid&32b8_rev&00a4_c05d39c2d9db#…`）。
pub fn address_token(address: u64) -> String {
    format!("{address:012x}")
}

/// 文本（设备路径 / 设备实例 id）是否属于这支遥控器。不区分大小写：
/// Raw Input 路径是小写、注册表实例 id 是大写。
pub fn matches_address(text: &str, address: u64) -> bool {
    text.to_ascii_lowercase().contains(&address_token(address))
}

/// 目标文件正文（助手侧解析同源；`version` 变化时两边都要改）。
pub fn file_body(address: u64) -> String {
    format!("version=1\naddress={}\n", address_token(address))
}

/// 解析目标文件正文（只认 `version=1` + 12 位十六进制地址；其它一律 `None`，
/// 不猜——旧版本或半截文件都不该把过滤条件带歪）。
pub fn parse_file_body(text: &str) -> Option<u64> {
    let mut version_ok = false;
    let mut address: Option<u64> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("version=") {
            version_ok = value.trim() == "1";
        } else if let Some(value) = line.strip_prefix("address=") {
            let value = value.trim();
            if value.len() == 12 {
                address = u64::from_str_radix(&value.to_ascii_lowercase(), 16).ok();
            }
        }
    }
    match (version_ok, address) {
        (true, Some(value)) if value != 0 => Some(value),
        _ => None,
    }
}

/// 从磁盘读目标（测试与排查用；运行期进程内直接读 [`get`]）。
pub fn read_file(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| parse_file_body(&text))
}

pub fn target_file_path() -> PathBuf {
    crate::rc003_bridge::default_bridge_dir().join(TARGET_FILE_NAME)
}

/// 写目标文件；`address == 0` 时删除（未知 = 没有过滤条件）。
fn write_file(address: u64) -> Result<(), String> {
    let path = target_file_path();
    if address == 0 {
        match std::fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("remove_failed:{error}")),
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("mkdir_failed:{error}"))?;
    }
    std::fs::write(&path, file_body(address)).map_err(|error| format!("write_failed:{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 路径匹配：Raw Input 路径小写、注册表实例 id 大写，两种都要命中；别的地址不命中。
    #[test]
    fn address_matching_is_case_insensitive_and_exact() {
        let address = 0x00C0_5D39_C2D9_DB;
        assert_eq!(address_token(address), "c05d39c2d9db");
        let raw_input_path = r"\\?\hid#{00001812-0000-1000-8000-00805f9b34fb}_dev_vid&012717_pid&32b8_rev&00a4_c05d39c2d9db#a&29fe826e&1&0000#{884b96c3-56ef-11d1-bc8c-00a0c91405dd}";
        assert!(matches_address(raw_input_path, address));
        let registry_instance = r"BTHLEDevice\{00001812-0000-1000-8000-00805F9B34FB}_DEV_VID&012717_PID&32B8_REV&00A4_C05D39C2D9DB\9&220DF34&1&0055";
        assert!(matches_address(registry_instance, address));
        // 另一支同型号遥控器：不命中（这正是 2026-10-13 现场的两支）。
        let other = r"BTHLEDevice\{00001812-0000-1000-8000-00805F9B34FB}_DEV_VID&012717_PID&32B8_REV&00A4_E85540B1B76B\9&2E2B90C9&1&0055";
        assert!(!matches_address(other, address));
    }

    /// 文件正文往返 + 只认自己这一版；坏值（半截、0）一律 `None`，不带歪过滤条件。
    #[test]
    fn target_file_round_trip_and_rejects_bad_values() {
        let address = 0x00C0_5D39_C2D9_DB;
        assert_eq!(parse_file_body(&file_body(address)), Some(address));
        assert_eq!(parse_file_body(""), None);
        assert_eq!(parse_file_body("address=c05d39c2d9db\n"), None); // 缺 version
        assert_eq!(parse_file_body("version=2\naddress=c05d39c2d9db\n"), None);
        assert_eq!(parse_file_body("version=1\naddress=000000000000\n"), None);
        assert_eq!(parse_file_body("version=1\naddress=zz\n"), None);
        // 允许大小写混写（助手侧可能被手工改过）。
        assert_eq!(
            parse_file_body("version=1\naddress=C05D39C2D9DB\n"),
            Some(address)
        );
    }
}
