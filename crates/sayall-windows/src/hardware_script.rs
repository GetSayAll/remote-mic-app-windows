//! 硬件信号脚本：把 `hardware-simulation` 仓库导出的 `export-app-script` 结果
//! 读成 Windows 侧可回放的事件序列。
//!
//! 背景（2026-10-05）：遥控器按键、语音会话等硬件信号此前只能在真机上产生，
//! onboarding 向导与按键链路因此无法在没有物理遥控器的机器上回归。模拟器
//! 在 `GetSayAll/hardware-simulation` 仓库里生成信号脚本（与 macOS 侧同源的
//! Profile/Scenario 数据），应用在仿真构建里回放脚本，把事件喂进**生产**解析
//! 链路（`decode_report_usages` + `ButtonStateMerger` + `AtvvVoicePipeline`），
//! 因此回放走的语义与真机一致。
//!
//! 边界：本模块只做"脚本 -> 事件"的解析与校验，不产生任何副作用，也不被基础
//! 语音/按键路径引用；只有 `runtime-simulation` 构建的仿真平台会消费它。
//!
//! 脚本契约（与 hardware-simulation 仓库 `windows/README.md` 同步维护）：
//!
//! ```json
//! {
//!   "schemaVersion": 1,
//!   "id": "xiaomi-voice-remote.rc001-short-voice",
//!   "sourceRevision": null,
//!   "events": [
//!     { "atMilliseconds": 0,   "kind": "ble_connected" },
//!     { "atMilliseconds": 10,  "kind": "hid_report", "reportID": 1, "dataHex": "280000000000" },
//!     { "atMilliseconds": 100, "kind": "voice_control", "dataHex": "04030207" },
//!     { "atMilliseconds": 110, "kind": "voice_audio", "dataHex": "1111" }
//!   ]
//! }
//! ```

use serde::{Deserialize, Serialize};

/// 脚本 schema 版本；解析时拒绝不认识的版本，避免静默误读新格式。
pub const HARDWARE_SCRIPT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareSignalScript {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub source_revision: Option<String>,
    pub events: Vec<HardwareSignalEntry>,
}

/// 时间线条目：`atMilliseconds` 与事件平铺（事件由 `kind` 内部标记）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareSignalEntry {
    pub at_milliseconds: u64,
    #[serde(flatten)]
    pub event: HardwareSignalEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind")]
pub enum HardwareSignalEvent {
    /// 遥控器 BLE 连接建立（对应 scenario 的 `connection.accepted`）。
    #[serde(rename = "ble_connected")]
    BleConnected,
    /// 遥控器连接断开。
    #[serde(rename = "ble_disconnected")]
    BleDisconnected,
    /// HID 设备挂载（Raw Input 设备可用）。
    #[serde(rename = "hid_attached")]
    HidAttached,
    /// HID 设备移除。
    #[serde(rename = "hid_removed")]
    HidRemoved,
    /// 原始 HID input report（按键按下/释放）。
    #[serde(rename = "hid_report")]
    HidReport {
        #[serde(rename = "reportID", default)]
        report_id: Option<u8>,
        #[serde(rename = "dataHex")]
        data_hex: String,
    },
    /// ATVV 控制通道字节（能力包、STREAM_START/STOP 等）。
    #[serde(rename = "voice_control")]
    VoiceControl {
        #[serde(rename = "dataHex")]
        data_hex: String,
    },
    /// ATVV 音频通道字节（ADPCM 帧）。
    #[serde(rename = "voice_audio")]
    VoiceAudio {
        #[serde(rename = "dataHex")]
        data_hex: String,
    },
    /// 其他 GATT 值（保留原始 UUID，便于后续扩展）。
    #[serde(rename = "gatt_value")]
    GattValue {
        #[serde(rename = "characteristicUUID")]
        characteristic_uuid: String,
        #[serde(rename = "dataHex")]
        data_hex: String,
    },
    /// 未识别的原始事件（保留原文，信息不丢失）。
    ///
    /// 注意：原事件的 `kind` 由导出器改写为 `originalKind`——内部标记
    /// （`tag = "kind"`）不允许变体字段与标记同名。
    #[serde(rename = "raw")]
    Raw {
        transport: String,
        #[serde(rename = "originalKind", default)]
        original_kind: Option<String>,
        #[serde(default)]
        payload: serde_json::Value,
    },
}

impl HardwareSignalEvent {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::BleConnected => "ble_connected",
            Self::BleDisconnected => "ble_disconnected",
            Self::HidAttached => "hid_attached",
            Self::HidRemoved => "hid_removed",
            Self::HidReport { .. } => "hid_report",
            Self::VoiceControl { .. } => "voice_control",
            Self::VoiceAudio { .. } => "voice_audio",
            Self::GattValue { .. } => "gatt_value",
            Self::Raw { .. } => "raw",
        }
    }
}

impl HardwareSignalEntry {
    pub fn hex_payload(&self) -> Option<&str> {
        match &self.event {
            HardwareSignalEvent::HidReport { data_hex, .. }
            | HardwareSignalEvent::VoiceControl { data_hex }
            | HardwareSignalEvent::VoiceAudio { data_hex }
            | HardwareSignalEvent::GattValue { data_hex, .. } => Some(data_hex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HardwareScriptError {
    #[error("hardware signal script JSON is invalid: {0}")]
    InvalidJson(String),
    #[error("hardware signal script schemaVersion {found} is not supported (expected {expected})")]
    UnsupportedSchemaVersion { found: u32, expected: u32 },
    #[error("hardware signal script has no id")]
    MissingId,
    #[error("hardware signal event #{index} has an invalid hex payload")]
    InvalidHex { index: usize },
}

impl HardwareSignalScript {
    pub fn parse(json: &str) -> Result<Self, HardwareScriptError> {
        // Windows 上手工编辑/编辑器保存的 JSON 常带 BOM；serde_json 不接受，
        // 直接在入口剥掉，避免"文件看起来没问题却报 expected value"。
        let json = json.trim_start_matches('\u{feff}');
        let script: Self = serde_json::from_str(json)
            .map_err(|error| HardwareScriptError::InvalidJson(error.to_string()))?;
        if script.schema_version != HARDWARE_SCRIPT_SCHEMA_VERSION {
            return Err(HardwareScriptError::UnsupportedSchemaVersion {
                found: script.schema_version,
                expected: HARDWARE_SCRIPT_SCHEMA_VERSION,
            });
        }
        if script.id.trim().is_empty() {
            return Err(HardwareScriptError::MissingId);
        }
        // 逐条校验 hex（含 report/voice 载荷），避免回放中途才发现脚本损坏。
        for (index, entry) in script.events.iter().enumerate() {
            if let Some(value) = entry.hex_payload() {
                if decode_hex(value).is_none() {
                    return Err(HardwareScriptError::InvalidHex { index });
                }
            }
        }
        Ok(script)
    }

    /// 回放顺序：按 `atMilliseconds` 升序，同刻保持脚本内原始顺序（稳定）。
    pub fn replay_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.events.len()).collect();
        order.sort_by_key(|index| self.events[*index].at_milliseconds);
        order
    }
}

/// 解码十六进制载荷；长度为奇数或含非十六进制字符时返回 `None`。
pub fn decode_hex(raw: &str) -> Option<Vec<u8>> {
    let trimmed = raw.trim();
    if trimmed.len() % 2 != 0 {
        return None;
    }
    trimmed
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(text, 16).ok()
        })
        .collect()
}

/// 便捷入口：HID report 字节 -> 生产解析 + 边沿合并 -> 语义按键边沿。
pub fn button_edges_for_hid_report(
    merger: &mut crate::raw_input::ButtonStateMerger,
    report: &[u8],
) -> Option<Vec<crate::raw_input::ButtonEdge>> {
    let usages = crate::raw_input::decode_report_usages(report).ok()?;
    Some(merger.update_hid_usages(usages))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "schemaVersion": 1,
      "id": "xiaomi-voice-remote.hid-button",
      "sourceRevision": null,
      "events": [
        {"atMilliseconds": 0, "kind": "hid_attached"},
        {"atMilliseconds": 10, "kind": "hid_report", "reportID": 1, "dataHex": "280000000000"},
        {"atMilliseconds": 20, "kind": "hid_report", "reportID": 1, "dataHex": "000000000000"},
        {"atMilliseconds": 30, "kind": "ble_connected"},
        {"atMilliseconds": 40, "kind": "voice_control", "dataHex": "0b010002030078"},
        {"atMilliseconds": 50, "kind": "voice_audio", "dataHex": "1111"},
        {"atMilliseconds": 60, "kind": "raw", "transport": "ble-gatt", "payload": {"kind": "advertisement", "rssi": -48}}
      ]
    }"#;

    #[test]
    fn parses_supported_script_and_keeps_event_order() {
        let script = HardwareSignalScript::parse(SAMPLE).expect("sample must parse");
        assert_eq!(script.id, "xiaomi-voice-remote.hid-button");
        assert_eq!(script.events.len(), 7);
        assert_eq!(script.events[1].event.kind_label(), "hid_report");
        assert_eq!(script.events[1].at_milliseconds, 10);
        assert_eq!(script.replay_order(), vec![0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn replay_order_is_stable_for_equal_timestamps() {
        let json = r#"{
          "schemaVersion": 1,
          "id": "order",
          "events": [
            {"atMilliseconds": 5, "kind": "ble_connected"},
            {"atMilliseconds": 1, "kind": "hid_attached"},
            {"atMilliseconds": 5, "kind": "hid_removed"}
          ]
        }"#;
        let script = HardwareSignalScript::parse(json).unwrap();
        assert_eq!(script.replay_order(), vec![1, 0, 2]);
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let json = SAMPLE.replace("\"schemaVersion\": 1", "\"schemaVersion\": 2");
        let error = HardwareSignalScript::parse(&json).unwrap_err();
        assert!(matches!(
            error,
            HardwareScriptError::UnsupportedSchemaVersion { found: 2, .. }
        ));
    }

    #[test]
    fn accepts_cli_exported_rc001_app_script() {
        // 契约锁定：这个文件是 hardware-simulation 仓库 `export-app-script` 的
        // 真实输出（Testing/hardware-scripts/rc001-short-voice-app-script.json），
        // 解析失败即表示两侧契约漂移。
        let json = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../Testing/hardware-scripts/rc001-short-voice-app-script.json"
        ));
        let script = HardwareSignalScript::parse(json).expect("exported script must parse");
        assert_eq!(script.id, "xiaomi-voice-remote.rc001-short-voice");
        assert_eq!(script.events.len(), 10);
        let kinds: Vec<&str> = script
            .events
            .iter()
            .map(|entry| entry.event.kind_label())
            .collect();
        assert_eq!(
            kinds,
            vec![
                "raw",
                "raw",
                "ble_connected",
                "raw",
                "raw",
                "raw",
                "voice_control",
                "voice_audio",
                "voice_audio",
                "voice_control"
            ]
        );
        let raw = script
            .events
            .iter()
            .find_map(|entry| match &entry.event {
                HardwareSignalEvent::Raw {
                    original_kind,
                    transport,
                    ..
                } => Some((transport.clone(), original_kind.clone())),
                _ => None,
            })
            .expect("exported script must contain raw events");
        assert_eq!(
            raw,
            ("ble-gatt".to_owned(), Some("adapter.state".to_owned()))
        );
        // 语音事件必须能过 hex 校验（解析期已做），这里复核可解码。
        let voice = script
            .events
            .iter()
            .find(|entry| entry.event.kind_label() == "voice_control")
            .and_then(|entry| entry.hex_payload())
            .expect("voice control payload");
        assert_eq!(decode_hex(voice).unwrap().len(), 4);
    }

    #[test]
    fn accepts_cli_exported_hid_button_app_script() {
        let json = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../Testing/hardware-scripts/hid-button-app-script.json"
        ));
        let script = HardwareSignalScript::parse(json).expect("exported HID script must parse");
        assert_eq!(script.events.len(), 4);
        let mut merger = crate::raw_input::ButtonStateMerger::default();
        let mut edges = 0;
        for entry in &script.events {
            if let HardwareSignalEvent::HidReport { data_hex, .. } = &entry.event {
                let bytes = decode_hex(data_hex).expect("hex payload");
                edges += button_edges_for_hid_report(&mut merger, &bytes)
                    .expect("report must decode")
                    .len();
            }
        }
        assert_eq!(edges, 2, "press + release from the exported fixture");
    }

    #[test]
    fn accepts_utf8_bom_prefixed_script() {
        let json = format!("\u{feff}{SAMPLE}");
        let script = HardwareSignalScript::parse(&json).expect("BOM-prefixed script must parse");
        assert_eq!(script.events.len(), 7);
    }

    #[test]
    fn rejects_invalid_hex_payload() {
        let json = SAMPLE.replace("\"280000000000\"", "\"zz\"");
        let error = HardwareSignalScript::parse(&json).unwrap_err();
        assert!(matches!(error, HardwareScriptError::InvalidHex { .. }));
    }

    #[test]
    fn decodes_hex_payloads() {
        assert_eq!(decode_hex("280000000000").unwrap().len(), 6);
        assert_eq!(decode_hex("0b01").unwrap(), vec![0x0B, 0x01]);
        assert!(decode_hex("0B0").is_none());
        assert!(decode_hex("0G").is_none());
    }

    #[test]
    fn hid_report_feeds_production_parser_and_merger() {
        // 6 字节报告：0x0028 -> 某个按键按下；全零 -> 释放（走生产解析链路）。
        let mut merger = crate::raw_input::ButtonStateMerger::default();
        let pressed =
            button_edges_for_hid_report(&mut merger, &[0x28, 0x00, 0x00, 0x00, 0x00, 0x00])
                .expect("press report must decode");
        assert_eq!(pressed.len(), 1);
        assert!(pressed[0].is_pressed);
        let released =
            button_edges_for_hid_report(&mut merger, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00])
                .expect("release report must decode");
        assert_eq!(released.len(), 1);
        assert!(!released[0].is_pressed);
        assert_eq!(released[0].button, pressed[0].button);
    }
}
