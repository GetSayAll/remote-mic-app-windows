use sayall_core::{AtvvCapabilities, VoiceSessionState};
use sayall_windows::button_mapping::{ButtonMappingSnapshot, FiredGesture};
use sayall_windows::raw_input::{RawInputPhase, RawInputSnapshot, RemoteButton};
use sayall_windows::send_input::ButtonTrigger;
use sayall_windows::{
    AudioPhase, AudioSnapshot, ConnectionPhase, ConnectionSnapshot, PairedRemote, PlatformSnapshot,
    RemoteModel,
};
use serde_json::{json, Value};

#[test]
fn rust_serialization_matches_the_shared_windows_runtime_contract() {
    let actual = json!({
        "platformSnapshot": PlatformSnapshot {
            platform: "windows".into(),
            windows_api_available: true,
            ble_scan_available: true,
            ble_voice_ready: true,
            wasapi_ready: true,
            raw_input_ready: true,
            send_input_ready: true,
            verification_status: "fixture-only".into(),
            connection: ConnectionSnapshot {
                phase: ConnectionPhase::Ready,
                remote_name: Some("Xiaomi Bluetooth Remote 2".into()),
                remote_model: RemoteModel::Rc001,
                capabilities: Some(AtvvCapabilities {
                    version: 256,
                    codecs: 1,
                    interaction: 1,
                    frame_size: 120,
                    selected_codec: 1,
                    sample_rate: 16_000,
                }),
                voice_state: VoiceSessionState::Idle,
                decoded_samples: 240,
                generation: 7,
                reconnect_attempt: 2,
                power_notifications_available: true,
                last_error: None,
            },
            audio: AudioSnapshot {
                phase: AudioPhase::Ready,
                selected_endpoint_id: Some("fixture-endpoint-id".into()),
                selected_endpoint_name: Some("Fixture Virtual Cable".into()),
                queued_samples: 80,
                submitted_samples: 160,
                generation: 4,
                last_error: None,
            },
            raw_input: RawInputSnapshot {
                phase: RawInputPhase::Ready,
                matched_device_count: 1,
                raw_event_count: 12,
                semantic_edge_count: 8,
                last_button: Some(RemoteButton::Home),
                last_is_pressed: Some(true),
                active_buttons: vec![RemoteButton::Home],
                last_error: None,
            },
            button_mapping: ButtonMappingSnapshot {
                enabled: true,
                gate_active: true,
                listener_active: true,
                swallowed_edges: 4,
                leaked_downs: 0,
                fired_gestures: 3,
                last_fired: Some(FiredGesture {
                    button: RemoteButton::Home,
                    trigger: ButtonTrigger::Single,
                }),
                last_error: None,
            },
        },
        "pairedRemotes": [
            PairedRemote {
                id: "fixture-rc001".into(),
                name: "Xiaomi Bluetooth Remote 2".into(),
                model: RemoteModel::Rc001,
                is_supported_candidate: true,
            },
            PairedRemote {
                id: "fixture-rc003".into(),
                name: "Xiaomi Bluetooth Remote 2 Pro".into(),
                model: RemoteModel::Rc003,
                is_supported_candidate: true,
            },
            PairedRemote {
                id: "fixture-unknown".into(),
                name: "Approved Fixture Remote".into(),
                model: RemoteModel::Unknown,
                is_supported_candidate: true,
            },
        ],
    });
    let expected: Value =
        serde_json::from_str(include_str!("../../../contracts/ipc/windows-runtime.json"))
            .expect("shared IPC contract fixture must be valid JSON");

    assert_no_snake_case_keys(&actual);
    assert_eq!(actual, expected);
}

fn assert_no_snake_case_keys(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                assert!(!key.contains('_'), "IPC field must be camelCase: {key}");
                assert_no_snake_case_keys(child);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_no_snake_case_keys),
        _ => {}
    }
}
