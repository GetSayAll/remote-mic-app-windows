use sayall_windows::raw_input::{RawInputPhase, RemoteButton};
use sayall_windows::send_input::SendInputSnapshot;
use sayall_windows::{AudioPhase, ConnectionPhase, PlatformSnapshot};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub schema_version: u32,
    pub app_version: String,
    pub platform: String,
    pub verification_status: String,
    pub capabilities: DiagnosticCapabilities,
    pub connection: ConnectionDiagnostic,
    pub audio: AudioDiagnostic,
    pub raw_input: RawInputDiagnostic,
    pub send_input: SendInputDiagnostic,
    pub button_mapping: ButtonMappingDiagnostic,
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

impl DiagnosticReport {
    pub fn capture(
        app_version: &str,
        platform: &PlatformSnapshot,
        send_input: &SendInputSnapshot,
    ) -> Self {
        let atvv = platform.connection.capabilities.as_ref();
        Self {
            schema_version: 1,
            app_version: app_version.to_owned(),
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
                last_error: Some("C:\\Users\\person\\private-path".to_owned()),
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
            },
        };
        let send_input = SendInputSnapshot {
            available: true,
            submitted_batches: 1,
            submitted_events: 4,
            last_error: Some("private SendInput backend details".to_owned()),
        };

        let report = DiagnosticReport::capture("0.1.0", &platform, &send_input);
        let json = serde_json::to_string(&report).unwrap();

        assert_eq!(report.connection.generation, 7);
        assert!(report.audio.endpoint_configured);
        assert_eq!(report.raw_input.last_button, Some(RemoteButton::Ok));
        assert!(report.connection.error_present);
        assert!(report.audio.error_present);
        assert!(report.raw_input.error_present);
        assert!(report.send_input.error_present);
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
