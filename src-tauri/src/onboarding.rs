//! Onboarding（首次使用设置向导）状态与迁移判定。
//!
//! 规范来源：Mac 仓 `origin/main` 的 `feature/first-run-onboarding/PRODUCT_SPEC.md`
//! 与 `platform-windows.md`；Windows 设计稿
//! `artifacts/design/2026-10-04-onboarding-design.html`（§6 持久化与生命周期）。
//!
//! 落点刻意独立于 `settings.json`：向导事务里要保存 `KeyChord`（sayall-windows
//! 类型，core 不引入按键类型），与 `voice-hold-hotkey.json` / `other-voice-hotkey.json`
//! 同款独立 JSON 文件、同一配置目录。
//!
//! 迁移规则（设计稿 §6.2）：
//! - 无向导状态文件、也没有任何旧安装证据 → 全新安装，从「欢迎」开始；
//! - 无向导状态文件、但存在旧安装证据（settings / 按键映射 / 快捷键文件任一）
//!   → 老用户，直接标记为当前流程版本已完成，不强制重走向导；
//! - 状态文件损坏 → 保守修复：有旧安装证据按老用户、否则按全新，绝不静默丢弃。

use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

/// 当前向导流程版本。新增真正必要的能力门禁时递增，并按 `completed_version`
/// 决定补跑范围（本版只预留字段）。
pub const CURRENT_FLOW_VERSION: u32 = 1;
/// 迁移与状态文件的一次性版本位：防止「第二次启动被误判为老用户」
/// （Mac 侧对应 Bug：2026-08-11-existing-users-forced-through-onboarding）。
pub const CURRENT_MIGRATION_VERSION: u32 = 1;
/// 向导状态文件名（与 settings.json 同目录）。
pub const STATE_FILE_NAME: &str = "onboarding.json";
/// 升级判定使用的「旧安装证据」文件；只检查存在性，不解析内容。
pub const INSTALL_EVIDENCE_FILES: [&str; 4] = [
    "settings.json",
    "button-mappings.json",
    "voice-hold-hotkey.json",
    "other-voice-hotkey.json",
];

/// 向导步骤（顺序即导航顺序，设计稿 §3 的 7 步）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingStep {
    Welcome,
    Remote,
    Audio,
    VoiceTool,
    VoiceTest,
    Controls,
    Complete,
}

impl OnboardingStep {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Remote => "remote",
            Self::Audio => "audio",
            Self::VoiceTool => "voice_tool",
            Self::VoiceTest => "voice_test",
            Self::Controls => "controls",
            Self::Complete => "complete",
        }
    }
}

impl<'de> Deserialize<'de> for OnboardingStep {
    /// 未知值（更旧/更新版本写入）一律归一化为「欢迎」，不允许一个陌生步骤名
    /// 把整份状态打成解析失败（设计稿 §6：旧 step 可解码、中断可续接）。
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "welcome" => Self::Welcome,
            "remote" => Self::Remote,
            "audio" => Self::Audio,
            "voice_tool" => Self::VoiceTool,
            "voice_test" => Self::VoiceTest,
            "controls" => Self::Controls,
            "complete" => Self::Complete,
            _ => Self::Welcome,
        })
    }
}

/// IPC 入参用：只接受已知步骤名；未知值由命令层拒绝并留日志。
pub fn parse_step(value: &str) -> Option<OnboardingStep> {
    Some(match value {
        "welcome" => OnboardingStep::Welcome,
        "remote" => OnboardingStep::Remote,
        "audio" => OnboardingStep::Audio,
        "voice_tool" => OnboardingStep::VoiceTool,
        "voice_test" => OnboardingStep::VoiceTest,
        "controls" => OnboardingStep::Controls,
        "complete" => OnboardingStep::Complete,
        _ => return None,
    })
}

/// 持久化的向导状态（`onboarding.json`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnboardingState {
    pub flow_version: u32,
    pub completed_version: u32,
    pub step: OnboardingStep,
    pub migration_version: u32,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            flow_version: CURRENT_FLOW_VERSION,
            completed_version: 0,
            step: OnboardingStep::Welcome,
            migration_version: CURRENT_MIGRATION_VERSION,
        }
    }
}

impl OnboardingState {
    /// 读入归一化：0 版本号补齐为当前版本；其余字段保持原意。
    pub fn normalized(mut self) -> Self {
        if self.flow_version == 0 {
            self.flow_version = CURRENT_FLOW_VERSION;
        }
        if self.migration_version == 0 {
            self.migration_version = CURRENT_MIGRATION_VERSION;
        }
        self
    }

    /// 未完成当前流程版本 = 向导进行中（完成前不允许进入主界面）。
    pub fn is_active(&self) -> bool {
        self.completed_version < self.flow_version
    }

    pub fn view(&self) -> OnboardingStateView {
        OnboardingStateView {
            flow_version: self.flow_version,
            completed_version: self.completed_version,
            step: self.step.as_str(),
            is_active: self.is_active(),
        }
    }
}

/// 返回给前端的只读视图（camelCase，与既有 IPC 约定一致）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStateView {
    pub flow_version: u32,
    pub completed_version: u32,
    pub step: &'static str,
    pub is_active: bool,
}

/// 无状态文件时的初次判定：有任何旧安装证据即为老用户，直接标记完成。
pub fn resolve_initial_state(install_evidence: bool) -> OnboardingState {
    OnboardingState {
        completed_version: if install_evidence {
            CURRENT_FLOW_VERSION
        } else {
            0
        },
        ..OnboardingState::default()
    }
}

pub fn state_path(settings_path: &Path) -> PathBuf {
    settings_path.with_file_name(STATE_FILE_NAME)
}

/// 旧安装证据：配置目录里任一已知配置文件存在（不解析内容）。
pub fn install_evidence_exists(settings_path: &Path) -> bool {
    let Some(directory) = settings_path.parent() else {
        return false;
    };
    INSTALL_EVIDENCE_FILES
        .iter()
        .any(|name| directory.join(name).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sayall-onboarding-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fresh_install_starts_active_from_welcome() {
        let state = resolve_initial_state(false);
        assert!(state.is_active());
        assert_eq!(state.completed_version, 0);
        assert_eq!(state.step, OnboardingStep::Welcome);
        assert_eq!(state.flow_version, CURRENT_FLOW_VERSION);
        assert_eq!(state.migration_version, CURRENT_MIGRATION_VERSION);
    }

    #[test]
    fn existing_install_is_migrated_to_completed_without_touching_other_fields() {
        let state = resolve_initial_state(true);
        assert!(!state.is_active());
        assert_eq!(state.completed_version, CURRENT_FLOW_VERSION);
        assert_eq!(state.step, OnboardingStep::Welcome);
    }

    #[test]
    fn state_json_round_trips_and_unknown_step_falls_back_to_welcome() {
        let mut state = OnboardingState::default();
        state.step = OnboardingStep::VoiceTest;
        state.completed_version = 0;
        let encoded = serde_json::to_string(&state).unwrap();
        assert!(encoded.contains("\"step\":\"voice_test\""));
        let decoded: OnboardingState = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, state);

        // 陌生步骤名不允许把整份状态打成解析失败：归一化为 Welcome，其余字段保留。
        let decoded: OnboardingState = serde_json::from_str(
            r#"{"flow_version":1,"completed_version":0,"step":"legacy_step","migration_version":1}"#,
        )
        .unwrap();
        assert_eq!(decoded.step, OnboardingStep::Welcome);
        assert!(decoded.is_active());
    }

    #[test]
    fn parse_step_rejects_unknown_values_for_ipc() {
        assert_eq!(parse_step("voice_tool"), Some(OnboardingStep::VoiceTool));
        assert_eq!(parse_step("complete"), Some(OnboardingStep::Complete));
        assert_eq!(parse_step("legacy_step"), None);
        assert_eq!(parse_step(""), None);
    }

    #[test]
    fn install_evidence_is_any_known_config_file_and_excludes_state_file() {
        let dir = unique_dir("evidence");
        let settings_path = dir.join("settings.json");
        assert!(!install_evidence_exists(&settings_path));

        std::fs::write(dir.join(STATE_FILE_NAME), "{}").unwrap();
        assert!(!install_evidence_exists(&settings_path));

        std::fs::write(&settings_path, "{}").unwrap();
        assert!(install_evidence_exists(&settings_path));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn install_evidence_covers_button_mapping_and_hotkey_files() {
        let dir = unique_dir("evidence-files");
        let settings_path = dir.join("settings.json");
        std::fs::write(dir.join("voice-hold-hotkey.json"), "null").unwrap();
        assert!(install_evidence_exists(&settings_path));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn state_path_is_sibling_of_settings_file() {
        let path = state_path(Path::new("/tmp/sayall/settings.json"));
        assert_eq!(path, PathBuf::from("/tmp/sayall/onboarding.json"));
    }
}
