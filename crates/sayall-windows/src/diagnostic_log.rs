//! 诊断日志的追加与轮转：**单个文件不超过 10 MB，超过即生成新文件**（保留最近几份）。
//!
//! 背景（2026-10-08 现场实测）：单机累计的 `sayall-diagnostic.log` 长到 768 MB、
//! 写入速率约 107 MB/天，既打不开也随不了包。要求：任何单个日志文件不得超过
//! 10 MB，超过就换新文件。
//!
//! 设计约束：主程序与提权助手**写同一个文件**（见 LOGGING.md「单一日志文件：
//! 主程序 + 提权助手」），因此这里刻意做到"无长句柄"：
//!
//! - 每次追加都重新 `open(create|append)`。任一进程轮转（重命名）之后，另一方
//!   下一次写入自然落到新文件——不需要跨进程通知，也不会继续写进已被改名的旧文件；
//! - 轮转判定用「当前大小 + 本次长度 > 上限」，因此单个文件不会超过上限一个记录以上；
//! - 轮转文件名带 UTC 时间戳与进程号，两个进程同时轮转也不会互相覆盖。

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// 单个日志文件的体积上限（10 MB）。
pub const ROTATE_BYTES: u64 = 10 * 1024 * 1024;
/// 轮转文件保留份数（不含当前文件）：超出时删除最旧的。
pub const KEEP_ROTATED: usize = 5;

/// 轮转判定（纯函数，单测覆盖）：当前大小 + 本次长度已经越过上限。
pub fn rotation_due(current_bytes: u64, incoming_bytes: u64, cap: u64) -> bool {
    current_bytes + incoming_bytes > cap
}

/// 追加一行；必要时先轮转。返回是否写入成功。
pub fn append_line(path: &Path, line: &str) -> bool {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let incoming = line.len() as u64 + 1;
    let current = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if rotation_due(current, incoming, ROTATE_BYTES) {
        rotate(path);
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return false;
    };
    let mut buf = String::with_capacity(line.len() + 1);
    buf.push_str(line);
    buf.push('\n');
    let ok = file.write_all(buf.as_bytes()).is_ok();
    let _ = file.flush();
    ok
}

/// 探测"这个路径能不能写"（只创建空文件，不写内容）：`initialize_diagnostic_log`
/// 的返回判据用它，避免为了探测往日志里塞一行无意义内容。
pub fn probe(path: &Path) -> bool {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .is_ok()
}

/// 轮转：把当前文件改名成带时间戳与进程号的旧文件，并按份数修剪。
fn rotate(path: &Path) -> Option<PathBuf> {
    let rotated = rotated_path(path, &crate::ble::utc_stamp_compact(), std::process::id());
    if std::fs::rename(path, &rotated).is_err() {
        // 另一进程已经轮转过（或文件根本不存在）：不是错误，继续写新文件即可。
        return None;
    }
    prune(path, KEEP_ROTATED);
    Some(rotated)
}

/// 轮转文件名（纯函数，单测覆盖）：`<主名>-<UTC 时间戳>-p<进程号>.log`。
///
/// 时间戳形如 `20261008T063158984Z`：只含 ASCII 字母与数字，可安全做文件名，
/// 也可以按字典序当时间序排序（修剪时依赖这一点）。
fn rotated_path(path: &Path, stamp: &str, pid: u32) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "sayall-diagnostic".to_string());
    path.with_file_name(format!("{stem}-{stamp}-p{pid}.log"))
}

/// 按份数修剪轮转文件：名字里的 UTC 时间戳字典序 == 时间序，最旧的先删。
fn prune(path: &Path, keep: usize) {
    let Some(dir) = path.parent() else { return };
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let prefix = format!("{stem}-");
    let mut rotated: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|candidate| {
            candidate
                .file_name()
                .map(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with(&prefix) && name.ends_with(".log")
                })
                .unwrap_or(false)
        })
        .collect();
    if rotated.len() <= keep {
        return;
    }
    rotated.sort();
    for old in rotated.iter().take(rotated.len() - keep) {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "sayall-diagnostic-log-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    #[test]
    fn rotation_due_only_past_the_cap() {
        assert!(rotation_due(10 * 1024 * 1024, 1, 10 * 1024 * 1024));
        assert!(rotation_due(10 * 1024 * 1024 - 10, 11, 10 * 1024 * 1024));
        assert!(!rotation_due(10 * 1024 * 1024 - 10, 10, 10 * 1024 * 1024));
        assert!(!rotation_due(0, 1, 10 * 1024 * 1024));
    }

    #[test]
    fn rotated_name_carries_timestamp_and_pid() {
        let path = PathBuf::from(r"C:\Logs\sayall-diagnostic.log");
        let rotated = rotated_path(&path, "20261008T063158984Z", 7920);
        assert_eq!(
            rotated.file_name().unwrap().to_string_lossy(),
            "sayall-diagnostic-20261008T063158984Z-p7920.log"
        );
        assert_eq!(rotated.parent().unwrap(), path.parent().unwrap());
    }

    #[test]
    fn append_line_rotates_when_the_file_would_pass_the_cap() {
        let dir = scratch_dir("rotate");
        let path = dir.join("sayall-diagnostic.log");
        // 种一个"差 1 字节到上限"的文件，下一次写入必须触发轮转。
        let seed = vec![b'x'; (ROTATE_BYTES - 1) as usize];
        std::fs::write(&path, &seed).expect("seed log");
        assert!(append_line(&path, "hello"));
        let rotated: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with("sayall-diagnostic-"))
            .collect();
        assert_eq!(rotated.len(), 1, "应生成一个新的旧文件：{rotated:?}");
        assert!(std::fs::metadata(&path).unwrap().len() < ROTATE_BYTES);
        let fresh = std::fs::read_to_string(&path).unwrap();
        assert!(fresh.contains("hello"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_only_the_newest_rotated_files() {
        let dir = scratch_dir("prune");
        let path = dir.join("sayall-diagnostic.log");
        std::fs::write(&path, b"current").unwrap();
        for index in 0..8 {
            let name = format!("sayall-diagnostic-2026100{index}T000000000Z-p1.log");
            std::fs::write(dir.join(name), b"old").unwrap();
        }
        prune(&path, 5);
        let mut left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with("sayall-diagnostic-"))
            .collect();
        left.sort();
        assert_eq!(left.len(), 5, "只留最近 5 份：{left:?}");
        assert!(left[0].contains("20261003"), "最旧的应被删掉：{left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn probe_creates_but_does_not_write() {
        let dir = scratch_dir("probe");
        let path = dir.join("sayall-diagnostic.log");
        assert!(probe(&path));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
