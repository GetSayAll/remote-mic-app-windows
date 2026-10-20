//! 启动状态与「本次启动前刚完成更新」判定（对齐 Mac）。
//!
//! Mac 侧在 `AppSettings.recordLaunchAndDetectCompletedUpdate` 里记录
//! `lastLaunchedBuild`，并据此得到 `completedUpdate`：
//! - 上次记录的 build 小于当前 build → 本次启动前完成了一次更新；
//! - 没有记录、但本机确实启动过旧版本（Sparkle 的 `SUHasLaunchedBefore`）→
//!   首次引入该机制时的老用户也算一次更新；
//! - 其它情况（首次全新启动、同版本、降级、记录不可解析）→ 不算更新。
//!
//! Windows 侧有意**不写进 `settings.json`**：安装生命周期测试会对该文件做逐字节
//! 哈希断言（升级不得改动用户设置），而启动记录是应用自己的运行状态。因此单独落
//! `launch-state.json`，与 `settings.json` 同目录。
//!
//! 版本口径：Windows 只有 `tauri.conf.json` 的 SemVer（安装包、exe 版本资源、
//! 更新器比较同源），没有 Mac 的独立 build 号，因此按 SemVer 比较「更高」才算更新。

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// 启动记录文件名（与 `settings.json` 同目录，不是用户设置）。
pub const STATE_FILE_NAME: &str = "launch-state.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct LaunchState {
    last_launched_version: Option<String>,
}

/// 一次启动的判定结果（`completed_update` 供启动可见性与更新完成提示使用）。
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchOutcome {
    pub completed_update: bool,
    pub previous_version: Option<String>,
    pub current_version: String,
}

/// 纯判定：本次启动是否发生在一次更新之后（单测覆盖）。
///
/// `has_launched_before` 只在**没有上次版本记录**时使用，对应 Mac 的
/// `sparkleHadLaunchedBefore`：老用户升级到首个带本机制的版本时也算完成更新。
pub fn completed_update(
    previous_version: Option<&str>,
    current_version: &str,
    has_launched_before: bool,
) -> bool {
    match previous_version {
        Some(previous) => match (
            semver::Version::parse(previous),
            semver::Version::parse(current_version),
        ) {
            (Ok(previous), Ok(current)) => current > previous,
            // 记录不可解析：不猜、不提示（宁可少提示一次，也不误报「已更新」）。
            _ => false,
        },
        None => has_launched_before,
    }
}

pub fn state_path(settings_path: &Path) -> PathBuf {
    settings_path.with_file_name(STATE_FILE_NAME)
}

/// 读取上次启动版本 → 判定 → 写入本次版本。读取失败如实返回错误；
/// 调用方记录日志并按「不算更新」继续启动（不阻断）。
pub fn record_launch(
    settings_path: &Path,
    current_version: &str,
    has_launched_before: bool,
) -> Result<LaunchOutcome, String> {
    let path = state_path(settings_path);
    let previous_version = match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str::<LaunchState>(&contents)
            .ok()
            .and_then(|state| state.last_launched_version),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(format!("读取启动状态失败：{error}")),
    };
    let completed = completed_update(
        previous_version.as_deref(),
        current_version,
        has_launched_before,
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败：{error}"))?;
    }
    let contents = serde_json::to_vec_pretty(&LaunchState {
        last_launched_version: Some(current_version.to_owned()),
    })
    .map_err(|error| format!("序列化启动状态失败：{error}"))?;
    fs::write(&path, contents).map_err(|error| format!("保存启动状态失败：{error}"))?;
    Ok(LaunchOutcome {
        completed_update: completed,
        previous_version,
        current_version: current_version.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sayall-launch-state-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn completed_update_compares_semver_and_falls_back_to_prior_launch() {
        // 版本变高 = 完成更新（Mac `currentBuildNumber > previousBuildNumber` 同义）。
        assert!(completed_update(Some("0.8.2"), "1.0.0", false));
        assert!(completed_update(Some("1.0.0"), "1.0.1", false));
        // 同版本、降级、记录不可解析：都不算更新。
        assert!(!completed_update(Some("1.0.0"), "1.0.0", true));
        assert!(!completed_update(Some("1.0.1"), "1.0.0", true));
        assert!(!completed_update(Some("not-a-version"), "1.0.0", true));
        // 没有记录：只有「本机启动过旧版本」才算一次更新（Mac sparkle 回退）。
        assert!(completed_update(None, "1.0.0", true));
        assert!(!completed_update(None, "1.0.0", false));
    }

    #[test]
    fn record_launch_is_one_shot_per_version_and_keeps_history() {
        let dir = unique_dir("record");
        let settings = dir.join("settings.json");

        // 全新安装第一次启动：不算更新，但记录当前版本。
        let first = record_launch(&settings, "1.0.0", false).unwrap();
        assert!(!first.completed_update);
        assert_eq!(first.previous_version, None);

        // 同版本重启：不再提示。
        let second = record_launch(&settings, "1.0.0", true).unwrap();
        assert!(!second.completed_update);
        assert_eq!(second.previous_version.as_deref(), Some("1.0.0"));

        // 升级：只提示一次，下一次启动回到普通路径。
        let upgraded = record_launch(&settings, "1.1.0", true).unwrap();
        assert!(upgraded.completed_update);
        assert_eq!(upgraded.previous_version.as_deref(), Some("1.0.0"));
        let after = record_launch(&settings, "1.1.0", true).unwrap();
        assert!(!after.completed_update);

        // 降级：不提示。
        let downgraded = record_launch(&settings, "1.0.9", true).unwrap();
        assert!(!downgraded.completed_update);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_install_without_state_file_counts_as_completed_update_once() {
        let dir = unique_dir("legacy");
        let settings = dir.join("settings.json");
        fs::write(&settings, b"{}").unwrap();

        // 老配置里没有 launch-state.json，但 settings.json 证明启动过旧版本。
        let first = record_launch(&settings, "1.0.0", true).unwrap();
        assert!(first.completed_update);
        let second = record_launch(&settings, "1.0.0", true).unwrap();
        assert!(!second.completed_update);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_launch_never_touches_user_settings_file() {
        let dir = unique_dir("settings-untouched");
        let settings = dir.join("settings.json");
        let original = br#"{"schema_version":4,"open_window_at_launch":false}"#;
        fs::write(&settings, original).unwrap();

        // 启动记录必须落在独立文件里：安装生命周期测试对 settings.json 做逐字节
        // 哈希断言（升级不得改动用户设置），这里把约束钉在单元测试里。
        let _ = record_launch(&settings, "1.0.0", true).unwrap();
        assert_eq!(fs::read(&settings).unwrap(), original);
        assert!(state_path(&settings).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_state_file_falls_back_to_prior_launch_evidence() {
        let dir = unique_dir("corrupt");
        let settings = dir.join("settings.json");
        fs::write(state_path(&settings), b"{ not json").unwrap();

        let outcome = record_launch(&settings, "1.0.0", true).unwrap();
        // 解析失败按「没有记录」处理，但这里 has_launched_before=true 会算更新；
        // 真正要守住的是：不 panic、不改用户设置，并把状态重写为有效文件。
        assert!(outcome.completed_update);
        let rewritten = fs::read_to_string(state_path(&settings)).unwrap();
        assert!(rewritten.contains("\"last_launched_version\": \"1.0.0\""));

        let _ = fs::remove_dir_all(&dir);
    }
}
