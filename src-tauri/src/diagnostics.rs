use sayall_core::VoiceInputTool;
use sayall_windows::raw_input::{RawInputPhase, RemoteButton};
use sayall_windows::send_input::SendInputSnapshot;
use sayall_windows::{AudioPhase, ConnectionPhase, ImeStatus, PlatformSnapshot};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub schema_version: u32,
    pub app_version: String,
    /// CI / 本地构建产物的构建号（`SAYALL_APP_BUILD`，无则 `unknown`）。
    pub app_build: String,
    /// 源码修订（40 位 git SHA，取不到时为 `unknown`）。
    pub source_revision: String,
    /// 发布通道（`SAYALL_BUILD_CHANNEL`，无则 `unknown`）。
    pub build_channel: String,
    /// 运行机器的 Windows 版本（major.minor.build，如 `10.0.26100`）。
    pub windows_version: String,
    /// 进程架构（`x86_64` 等）。
    pub process_architecture: String,
    pub platform: String,
    pub verification_status: String,
    pub capabilities: DiagnosticCapabilities,
    pub connection: ConnectionDiagnostic,
    pub audio: AudioDiagnostic,
    pub raw_input: RawInputDiagnostic,
    pub send_input: SendInputDiagnostic,
    pub button_mapping: ButtonMappingDiagnostic,
    /// 输入法状态（2026-10-13）：诊断"语音没出字"时判断是不是没切到用户选的
    /// 输入法。字段是**输入法显示名**（不是路径/设备身份，见 `LOGGING.md` 口径）。
    pub ime: ImeDiagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCapabilities {
    pub windows_api_available: bool,
    pub ble_scan_available: bool,
    pub ble_voice_ready: bool,
    pub wasapi_ready: bool,
    pub raw_input_ready: bool,
    pub send_input_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDiagnostic {
    pub phase: ConnectionPhase,
    pub capabilities_confirmed: bool,
    pub sample_rate: Option<u32>,
    pub frame_size: Option<usize>,
    pub decoded_samples: u64,
    pub generation: u64,
    pub reconnect_attempt: u32,
    pub power_notifications_available: bool,
    pub error_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnostic {
    pub phase: AudioPhase,
    pub endpoint_configured: bool,
    pub queued_samples: u64,
    pub submitted_samples: u64,
    pub generation: u64,
    pub error_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawInputDiagnostic {
    pub phase: RawInputPhase,
    pub matched_device_count: u32,
    pub raw_event_count: u64,
    pub semantic_edge_count: u64,
    pub last_button: Option<RemoteButton>,
    pub last_is_pressed: Option<bool>,
    pub error_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendInputDiagnostic {
    pub available: bool,
    pub submitted_batches: u64,
    pub submitted_events: u64,
    pub error_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonMappingDiagnostic {
    pub enabled: bool,
    pub gate_active: bool,
    pub listener_active: bool,
    pub swallowed_edges: u64,
    pub leaked_downs: u64,
    pub fired_gestures: u64,
    pub error_present: bool,
}

/// 输入法诊断段（schema v3，2026-10-13）。
///
/// `detected=false` = 本次查询不可用（TSF 读不到）；`matches_expected=None` =
/// 判定不出来（未选工具 / 工具不需要切输入法）——两者都表示"不能据此下结论"，
/// 与 `sayall_windows::ImeStatus` 同一语义，只是字段名按诊断段落命名。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImeDiagnostic {
    pub detected: bool,
    /// 当前会话活动输入法的显示名。
    pub active_name: Option<String>,
    /// 系统里可用的输入法显示名（去重、排序、最多 16 个）。
    pub installed_names: Vec<String>,
    /// 用户所选输入工具对应的目标输入法显示名。
    pub expected_name: Option<String>,
    /// 活动输入法是否就是目标。
    pub matches_expected: Option<bool>,
}

impl ImeDiagnostic {
    /// 由平台侧实时读回的状态构造（查询失败在平台侧已降级为 `unavailable()`）。
    fn from_status(status: &ImeStatus) -> Self {
        Self {
            detected: status.detected,
            active_name: status.active.clone(),
            installed_names: status.installed.clone(),
            expected_name: status.expected.clone(),
            matches_expected: status.matches_expected,
        }
    }
}

impl DiagnosticReport {
    pub fn capture(
        app_version: &str,
        platform: &PlatformSnapshot,
        send_input: &SendInputSnapshot,
        voice_input_tool: Option<VoiceInputTool>,
    ) -> Self {
        let atvv = platform.connection.capabilities.as_ref();
        #[cfg(windows)]
        let windows_version = sayall_windows::compatibility::current_windows_version().to_string();
        #[cfg(not(windows))]
        let windows_version = "unknown".to_owned();
        // 输入法状态**实时读回**（不缓存）：诊断的价值就在"此刻到底切没切过去"。
        // 平台侧查询有界（≤500ms）且失败降级为 `detected=false`，绝不 panic；
        // 上层把它放在阻塞线程池里执行，不占主线程。
        let ime = sayall_windows::ime_status_for(voice_input_tool);
        Self {
            schema_version: 3,
            app_version: app_version.to_owned(),
            app_build: option_env!("SAYALL_APP_BUILD")
                .unwrap_or("unknown")
                .to_owned(),
            source_revision: env!("SAYALL_SOURCE_REVISION").to_owned(),
            build_channel: option_env!("SAYALL_BUILD_CHANNEL")
                .unwrap_or("unknown")
                .to_owned(),
            windows_version,
            process_architecture: std::env::consts::ARCH.to_owned(),
            platform: platform.platform.clone(),
            verification_status: platform.verification_status.clone(),
            capabilities: DiagnosticCapabilities {
                windows_api_available: platform.windows_api_available,
                ble_scan_available: platform.ble_scan_available,
                ble_voice_ready: platform.ble_voice_ready,
                wasapi_ready: platform.wasapi_ready,
                raw_input_ready: platform.raw_input_ready,
                send_input_ready: platform.send_input_ready,
            },
            connection: ConnectionDiagnostic {
                phase: platform.connection.phase,
                capabilities_confirmed: atvv.is_some(),
                sample_rate: atvv.map(|capabilities| capabilities.sample_rate),
                frame_size: atvv.map(|capabilities| capabilities.frame_size),
                decoded_samples: platform.connection.decoded_samples,
                generation: platform.connection.generation,
                reconnect_attempt: platform.connection.reconnect_attempt,
                power_notifications_available: platform.connection.power_notifications_available,
                error_present: platform.connection.last_error.is_some(),
            },
            audio: AudioDiagnostic {
                phase: platform.audio.phase,
                endpoint_configured: platform.audio.selected_endpoint_id.is_some()
                    && platform.audio.selected_endpoint_name.is_some(),
                queued_samples: platform.audio.queued_samples,
                submitted_samples: platform.audio.submitted_samples,
                generation: platform.audio.generation,
                error_present: platform.audio.last_error.is_some(),
            },
            raw_input: RawInputDiagnostic {
                phase: platform.raw_input.phase,
                matched_device_count: platform.raw_input.matched_device_count,
                raw_event_count: platform.raw_input.raw_event_count,
                semantic_edge_count: platform.raw_input.semantic_edge_count,
                last_button: platform.raw_input.last_button,
                last_is_pressed: platform.raw_input.last_is_pressed,
                error_present: platform.raw_input.last_error.is_some(),
            },
            send_input: SendInputDiagnostic {
                available: send_input.available,
                submitted_batches: send_input.submitted_batches,
                submitted_events: send_input.submitted_events,
                error_present: send_input.last_error.is_some(),
            },
            button_mapping: ButtonMappingDiagnostic {
                enabled: platform.button_mapping.enabled,
                gate_active: platform.button_mapping.gate_active,
                listener_active: platform.button_mapping.listener_active,
                swallowed_edges: platform.button_mapping.swallowed_edges,
                leaked_downs: platform.button_mapping.leaked_downs,
                fired_gestures: platform.button_mapping.fired_gestures,
                error_present: platform.button_mapping.last_error.is_some(),
            },
            ime: ImeDiagnostic::from_status(&ime),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sayall_windows::raw_input::{RawInputSnapshot, RemoteButton};
    use sayall_windows::{AudioSnapshot, ConnectionSnapshot};

    #[test]
    fn diagnostic_report_excludes_device_identity_paths_and_error_text() {
        let platform = PlatformSnapshot {
            platform: "windows".to_owned(),
            windows_api_available: true,
            ble_scan_available: true,
            ble_voice_ready: false,
            wasapi_ready: false,
            raw_input_ready: true,
            send_input_ready: true,
            verification_status: "真机验证中".to_owned(),
            connection: ConnectionSnapshot {
                remote_name: Some("私人遥控器名称".to_owned()),
                last_error: Some("蓝牙地址 AA:BB:CC:DD:EE:FF".to_owned()),
                generation: 7,
                reconnect_attempt: 2,
                ..ConnectionSnapshot::default()
            },
            audio: AudioSnapshot {
                selected_endpoint_id: Some("private-endpoint-id".to_owned()),
                selected_endpoint_name: Some("私人音频端点".to_owned()),
                last_error: Some("%LOCALAPPDATA%\\SayAll\\fixture\\private-path".to_owned()),
                submitted_samples: 320,
                ..AudioSnapshot::default()
            },
            raw_input: RawInputSnapshot {
                phase: RawInputPhase::Ready,
                matched_device_count: 1,
                raw_event_count: 4,
                semantic_edge_count: 2,
                last_button: Some(RemoteButton::Ok),
                last_is_pressed: Some(false),
                active_buttons: Vec::new(),
                last_error: Some("\\\\?\\HID#private-device-path".to_owned()),
                stale_remote_event_count: 0,
            },
            button_mapping: sayall_windows::button_mapping::ButtonMappingSnapshot {
                enabled: true,
                gate_active: true,
                listener_active: true,
                swallowed_edges: 3,
                leaked_downs: 1,
                fired_gestures: 2,
                last_fired: None,
                last_error: Some("内部注入细节".to_owned()),
                last_focus: None,
            },
        };
        let send_input = SendInputSnapshot {
            available: true,
            submitted_batches: 1,
            submitted_events: 4,
            last_error: Some("private SendInput backend details".to_owned()),
        };

        let report = DiagnosticReport::capture(
            "0.1.0",
            &platform,
            &send_input,
            Some(VoiceInputTool::Wechat),
        );
        let json = serde_json::to_string(&report).unwrap();

        assert_eq!(report.schema_version, 3);
        assert_eq!(report.app_version, "0.1.0");
        // 环境字段必须真实可用（Build/Windows 版本是报障的定位前提）。
        assert!(!report.app_build.is_empty());
        assert!(!report.source_revision.is_empty());
        assert!(report.source_revision.len() == 40 || report.source_revision == "unknown");
        assert!(!report.process_architecture.is_empty());
        #[cfg(windows)]
        assert_eq!(report.windows_version.split('.').count(), 3);
        assert_eq!(report.connection.generation, 7);
        assert!(report.audio.endpoint_configured);
        assert_eq!(report.raw_input.last_button, Some(RemoteButton::Ok));
        assert!(report.connection.error_present);
        assert!(report.audio.error_present);
        assert!(report.raw_input.error_present);
        assert!(report.send_input.error_present);
        // 输入法段（2026-10-13）：字段是**本机实时读回**的（TSF 查询），值因机器而异，
        // 所以这里只断言结构与 IPC 契约（camelCase 键、列表上限、能序列化），
        // 不断言具体输入法名字。
        assert!(json.contains("\"ime\""));
        assert!(json.contains("\"activeName\""));
        assert!(json.contains("\"installedNames\""));
        assert!(json.contains("\"expectedName\""));
        assert!(json.contains("\"matchesExpected\""));
        assert!(
            report.ime.installed_names.len() <= sayall_windows::MAX_INSTALLED_IME_NAMES,
            "installed ime list must stay within the display limit"
        );
        // 未选工具时没有目标输入法：判定必须是"判定不出来"（界面 fail-open 不提示）。
        let without_tool = DiagnosticReport::capture("0.1.0", &platform, &send_input, None);
        assert!(without_tool.ime.expected_name.is_none());
        assert!(without_tool.ime.matches_expected.is_none());
        for secret in [
            "私人遥控器名称",
            "AA:BB:CC:DD:EE:FF",
            "private-endpoint-id",
            "私人音频端点",
            "private-path",
            "private-device-path",
            "private SendInput backend details",
            "内部注入细节",
        ] {
            assert!(!json.contains(secret), "diagnostic leaked {secret}");
        }
    }
}
