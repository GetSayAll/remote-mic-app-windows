use serde::{Deserialize, Serialize};
use thiserror::Error;

pub struct AtvvUuids;

impl AtvvUuids {
    pub const SERVICE: &'static str = "AB5E0001-5A21-4F05-BC7D-AF01F617B664";
    pub const TRANSMIT: &'static str = "AB5E0002-5A21-4F05-BC7D-AF01F617B664";
    pub const AUDIO: &'static str = "AB5E0003-5A21-4F05-BC7D-AF01F617B664";
    pub const CONTROL: &'static str = "AB5E0004-5A21-4F05-BC7D-AF01F617B664";
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtvvControlEvent {
    Capabilities(AtvvCapabilities),
    MicrophoneOpenRequested,
    StreamStarted {
        interaction: Option<u8>,
        codec: Option<u8>,
        session_id: u8,
    },
    StreamStopped,
    DecoderSync {
        predictor: i16,
        step_index: u8,
    },
    Unknown {
        opcode: u8,
    },
}

impl AtvvControlEvent {
    pub fn parse(bytes: &[u8]) -> Result<Self, AtvvError> {
        let opcode = *bytes.first().ok_or(AtvvError::PacketTooShort)?;
        match opcode {
            0x0B => Ok(Self::Capabilities(AtvvCapabilities::parse(bytes)?)),
            0x08 => Ok(Self::MicrophoneOpenRequested),
            0x04 => Ok(Self::StreamStarted {
                interaction: bytes.get(1).copied(),
                codec: bytes.get(2).copied(),
                session_id: bytes.get(3).copied().unwrap_or(0),
            }),
            0x00 => Ok(Self::StreamStopped),
            0x0A => {
                if bytes.len() < 7 {
                    return Err(AtvvError::PacketTooShort);
                }
                Ok(Self::DecoderSync {
                    predictor: i16::from_be_bytes([bytes[4], bytes[5]]),
                    step_index: bytes[6],
                })
            }
            _ => Ok(Self::Unknown { opcode }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtvvCommand {
    GetCapabilitiesV10,
    MicrophoneOpen { version: u16, codec: u8 },
    MicrophoneClose { version: u16, session_id: u8 },
    MicrophoneExtend { version: u16, session_id: u8 },
}

impl AtvvCommand {
    pub fn encode(&self) -> Option<Vec<u8>> {
        match *self {
            Self::GetCapabilitiesV10 => Some(vec![0x0A, 0x01, 0x00, 0x00, 0x03, 0x03]),
            Self::MicrophoneOpen { version, codec: _ } if version >= 0x0100 => {
                Some(vec![0x0C, 0x00])
            }
            Self::MicrophoneOpen { codec, .. } => Some(vec![0x0C, 0x00, codec]),
            Self::MicrophoneClose {
                version,
                session_id,
            } if version >= 0x0100 => Some(vec![0x0D, session_id]),
            Self::MicrophoneClose { .. } => Some(vec![0x0D]),
            Self::MicrophoneExtend {
                version,
                session_id,
            } if version >= 0x0100 => Some(vec![0x0E, session_id]),
            Self::MicrophoneExtend { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtvvCapabilities {
    pub version: u16,
    pub codecs: u8,
    pub interaction: u8,
    pub frame_size: usize,
    pub selected_codec: u8,
    pub sample_rate: u32,
}

impl AtvvCapabilities {
    pub fn parse(bytes: &[u8]) -> Result<Self, AtvvError> {
        if bytes.len() < 7 {
            return Err(AtvvError::PacketTooShort);
        }
        if bytes[0] != 0x0B {
            return Err(AtvvError::UnexpectedMessage(bytes[0]));
        }

        let version = u16::from_be_bytes([bytes[1], bytes[2]]);
        let (mut codecs, mut interaction) = if version >= 0x0100 {
            (bytes[3], bytes[4])
        } else {
            if bytes.len() < 9 {
                return Err(AtvvError::PacketTooShort);
            }
            (bytes[4], 0)
        };

        if version >= 0x0100 && codecs == 0 && bytes.len() >= 9 && bytes[4] & 0x03 != 0 {
            codecs = bytes[4];
            interaction = 0x03;
        }

        let selected_codec = if codecs & 0x02 != 0 { 0x02 } else { 0x01 };
        let sample_rate = if selected_codec == 0x02 {
            16_000
        } else {
            8_000
        };
        let advertised_frame_size = u16::from_be_bytes([bytes[5], bytes[6]]) as usize;

        Ok(Self {
            version,
            codecs,
            interaction,
            frame_size: if advertised_frame_size == 0 {
                120
            } else {
                advertised_frame_size
            },
            selected_codec,
            sample_rate,
        })
    }

    pub fn supports_sayall_audio(&self) -> bool {
        self.sample_rate == 16_000
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AtvvError {
    #[error("ATVV packet is too short")]
    PacketTooShort,
    #[error("unexpected ATVV message 0x{0:02X}")]
    UnexpectedMessage(u8),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_one_capabilities() {
        let capabilities = AtvvCapabilities::parse(&[0x0B, 0x01, 0x00, 0x02, 0x03, 0, 120])
            .expect("valid capabilities");
        assert_eq!(capabilities.version, 0x0100);
        assert_eq!(capabilities.selected_codec, 0x02);
        assert_eq!(capabilities.sample_rate, 16_000);
        assert_eq!(capabilities.frame_size, 120);
        assert!(capabilities.supports_sayall_audio());
    }

    #[test]
    fn accepts_legacy_codec_layout_advertised_as_version_one() {
        let capabilities = AtvvCapabilities::parse(&[0x0B, 0x01, 0x00, 0x00, 0x02, 0, 120, 0, 0])
            .expect("legacy layout");
        assert_eq!(capabilities.selected_codec, 0x02);
        assert_eq!(capabilities.interaction, 0x03);
    }

    #[test]
    fn rejects_malformed_capabilities() {
        assert_eq!(AtvvCapabilities::parse(&[]), Err(AtvvError::PacketTooShort));
        assert_eq!(
            AtvvCapabilities::parse(&[0, 1, 0, 2, 3, 0, 120]),
            Err(AtvvError::UnexpectedMessage(0))
        );
    }

    #[test]
    fn encodes_version_specific_microphone_commands() {
        assert_eq!(
            AtvvCommand::MicrophoneOpen {
                version: 0x0100,
                codec: 2
            }
            .encode(),
            Some(vec![0x0C, 0])
        );
        assert_eq!(
            AtvvCommand::MicrophoneOpen {
                version: 1,
                codec: 2
            }
            .encode(),
            Some(vec![0x0C, 0, 2])
        );
        assert_eq!(
            AtvvCommand::MicrophoneClose {
                version: 0x0100,
                session_id: 7
            }
            .encode(),
            Some(vec![0x0D, 7])
        );
        assert_eq!(
            AtvvCommand::MicrophoneExtend {
                version: 1,
                session_id: 7
            }
            .encode(),
            None
        );
    }

    #[test]
    fn parses_stream_and_decoder_control_events() {
        assert_eq!(
            AtvvControlEvent::parse(&[0x04, 0x03, 0x02, 0x07]).unwrap(),
            AtvvControlEvent::StreamStarted {
                interaction: Some(0x03),
                codec: Some(0x02),
                session_id: 0x07,
            }
        );
        assert_eq!(
            AtvvControlEvent::parse(&[0x0A, 0, 0, 0, 0xFF, 0x9C, 12]).unwrap(),
            AtvvControlEvent::DecoderSync {
                predictor: -100,
                step_index: 12,
            }
        );
        assert_eq!(
            AtvvControlEvent::parse(&[0x55]).unwrap(),
            AtvvControlEvent::Unknown { opcode: 0x55 }
        );
    }

    #[test]
    fn rejects_short_decoder_sync() {
        assert_eq!(
            AtvvControlEvent::parse(&[0x0A, 0, 0]),
            Err(AtvvError::PacketTooShort)
        );
    }
}
