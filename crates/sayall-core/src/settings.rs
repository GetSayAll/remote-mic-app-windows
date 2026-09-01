use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTriggerMode {
    #[default]
    Hold,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub schema_version: u32,
    pub selected_remote_id: Option<String>,
    pub audio_endpoint_id: Option<String>,
    pub audio_endpoint_name: Option<String>,
    pub gain_db: f32,
    pub voice_trigger_mode: VoiceTriggerMode,
    pub launch_at_login: bool,
    pub open_window_at_launch: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            selected_remote_id: None,
            audio_endpoint_id: None,
            audio_endpoint_name: None,
            gain_db: 0.0,
            voice_trigger_mode: VoiceTriggerMode::Hold,
            launch_at_login: false,
            open_window_at_launch: true,
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.gain_db = if self.gain_db.is_finite() {
            self.gain_db.clamp(0.0, 24.0)
        } else {
            0.0
        };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_gain_and_keeps_hold_as_only_voice_mode() {
        let settings = AppSettings {
            gain_db: 30.0,
            ..AppSettings::default()
        }
        .normalized();
        assert_eq!(settings.gain_db, 24.0);
        assert_eq!(settings.voice_trigger_mode, VoiceTriggerMode::Hold);
    }

    #[test]
    fn older_settings_without_endpoint_name_remain_compatible() {
        let settings: AppSettings = serde_json::from_str(
            r#"{"schema_version":1,"audio_endpoint_id":"endpoint-1","gain_db":0.0,"voice_trigger_mode":"hold","launch_at_login":false,"open_window_at_launch":true}"#,
        )
        .unwrap();

        assert_eq!(settings.audio_endpoint_id.as_deref(), Some("endpoint-1"));
        assert_eq!(settings.audio_endpoint_name, None);
    }
}
