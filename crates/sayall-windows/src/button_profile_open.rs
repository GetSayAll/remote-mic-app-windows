//! `.sayall` 文件关联的进程间投递。
//!
//! 双击 `.sayall` 时 Windows 会把路径作为命令行参数启动应用。已运行实例
//! 由单实例互斥体拦下第二个进程，因此路径必须先落到会话内的临时文件，
//! 再由第一个实例的监听线程取走并转成前端事件。

use std::path::{Path, PathBuf};

const PENDING_FILE_NAME: &str = "pending-button-profile.sayall-path";

/// Returns the first `.sayall` file passed on the command line.
pub fn path_from_args() -> Option<PathBuf> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|path| is_sayall_file(path))
}

/// Stores a file path for the already-running instance.
pub fn stash(path: &Path) -> std::io::Result<()> {
    let target = pending_file_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "local app data unavailable")
    })?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target, path.to_string_lossy().as_bytes())
}

/// Takes the pending path once. Invalid or stale entries are discarded.
pub fn take() -> Option<PathBuf> {
    let target = pending_file_path()?;
    let raw = std::fs::read_to_string(&target).ok()?;
    let _ = std::fs::remove_file(&target);
    let path = PathBuf::from(raw.trim());
    is_sayall_file(&path).then_some(path)
}

fn pending_file_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|root| PathBuf::from(root).join("SayAll").join(PENDING_FILE_NAME))
}

fn is_sayall_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sayall"))
        && path.is_file()
}
