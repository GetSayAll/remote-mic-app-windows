use crate::{
    normalize_gain_db, process_pcm, AtvvCapabilities, AtvvControlEvent, AtvvError,
    FrameAccumulator, ImaAdpcmDecoder, VoiceSession, VoiceSessionError, VoiceSessionEvent,
    VoiceSessionState,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum PipelineOutput {
    Ready(AtvvCapabilities),
    MicrophoneOpenRequested,
    StreamStarted {
        session_id: u8,
        generation: u64,
    },
    Samples {
        generation: u64,
        samples: Vec<i16>,
    },
    StreamStopped {
        session_id: u8,
        generation: u64,
        discarded_partial_bytes: usize,
    },
    DecoderSynchronized {
        predictor: i16,
        step_index: u8,
        discarded_partial_bytes: usize,
    },
    UnknownControl {
        opcode: u8,
    },
}

#[derive(Debug, Clone)]
pub struct AtvvVoicePipeline {
    capabilities: Option<AtvvCapabilities>,
    session: VoiceSession,
    accumulator: FrameAccumulator,
    decoder: ImaAdpcmDecoder,
    pending_sync: Option<(i16, u8)>,
    streaming_packets: bool,
    gain_db: f32,
}

impl Default for AtvvVoicePipeline {
    fn default() -> Self {
        Self {
            capabilities: None,
            session: VoiceSession::default(),
            accumulator: FrameAccumulator::default(),
            decoder: ImaAdpcmDecoder::default(),
            pending_sync: None,
            streaming_packets: false,
            gain_db: 0.0,
        }
    }
}

impl AtvvVoicePipeline {
    pub fn capabilities(&self) -> Option<&AtvvCapabilities> {
        self.capabilities.as_ref()
    }

    pub fn state(&self) -> VoiceSessionState {
        self.session.state()
    }

    pub fn generation(&self) -> u64 {
        self.session.generation()
    }

    pub fn session_id(&self) -> Option<u8> {
        self.session.session_id()
    }

    pub fn set_gain_db(&mut self, gain_db: f32) {
        self.gain_db = normalize_gain_db(gain_db);
    }

    /// Chromecast ATVV v1 notifications are continuous ADPCM, not fixed-size frames.
    /// Keep Xiaomi's existing frame/sync rules as the default.
    pub fn set_streaming_packets(&mut self, enabled: bool) {
        self.streaming_packets = enabled;
    }

    pub fn handle_control(&mut self, bytes: &[u8]) -> Result<PipelineOutput, PipelineError> {
        let event = if self.streaming_packets && bytes.first() == Some(&0x0B) {
            AtvvControlEvent::Capabilities(AtvvCapabilities::parse_chromecast(bytes)?)
        } else {
            AtvvControlEvent::parse(bytes)?
        };
        match event {
            AtvvControlEvent::Capabilities(capabilities) => {
                if !capabilities.supports_sayall_audio()
                    && !(self.streaming_packets && capabilities.selected_codec == 1)
                {
                    return Err(PipelineError::UnsupportedSampleRate(
                        capabilities.sample_rate,
                    ));
                }
                self.capabilities = Some(capabilities.clone());
                Ok(PipelineOutput::Ready(capabilities))
            }
            AtvvControlEvent::MicrophoneOpenRequested => {
                self.require_capabilities()?;
                if self.streaming_packets && self.session.state() == VoiceSessionState::Idle {
                    self.pending_sync = None;
                    self.decoder.reset(0, 0);
                }
                Ok(PipelineOutput::MicrophoneOpenRequested)
            }
            AtvvControlEvent::StreamStarted {
                interaction,
                codec,
                session_id,
            } => {
                let streaming_packets = self.streaming_packets;
                let capabilities = self.require_capabilities_mut()?;
                let (interaction, codec) = if !streaming_packets {
                    (interaction, codec)
                } else if capabilities.version == 0x0004 {
                    // v0.4 has no codec or interaction fields in START.
                    (None, None)
                } else {
                    if bytes.len() < 4 {
                        return Err(AtvvError::PacketTooShort.into());
                    }
                    let codec = bytes[2];
                    if !capabilities.supports_chromecast_codec(codec) {
                        return Err(PipelineError::UnsupportedChromecastCodec(codec));
                    }
                    // START byte 1 is the reason, not the negotiated interaction.
                    (None, Some(codec))
                };
                if let Some(interaction) = interaction {
                    capabilities.interaction = interaction;
                }
                if let Some(codec) = codec {
                    capabilities.selected_codec = codec;
                    capabilities.sample_rate = if codec == 0x02 { 16_000 } else { 8_000 };
                }
                if !capabilities.supports_sayall_audio()
                    && !(streaming_packets && capabilities.selected_codec == 1)
                {
                    return Err(PipelineError::UnsupportedSampleRate(
                        capabilities.sample_rate,
                    ));
                }
                self.accumulator.reset();
                if !self.streaming_packets {
                    self.pending_sync = None;
                }
                self.decoder.reset(0, 0);
                self.session
                    .apply(VoiceSessionEvent::StreamStarted { session_id })?;
                Ok(PipelineOutput::StreamStarted {
                    session_id,
                    generation: self.session.generation(),
                })
            }
            AtvvControlEvent::StreamStopped => {
                let session_id = self
                    .session
                    .session_id()
                    .ok_or(PipelineError::MissingSession)?;
                let discarded_partial_bytes = self.accumulator.pending().len();
                self.accumulator.reset();
                self.pending_sync = None;
                self.session
                    .apply(VoiceSessionEvent::StreamStopped { session_id })?;
                Ok(PipelineOutput::StreamStopped {
                    session_id,
                    generation: self.session.generation(),
                    discarded_partial_bytes,
                })
            }
            AtvvControlEvent::DecoderSync {
                predictor,
                step_index,
            } => {
                let capabilities = self.require_capabilities()?;
                if self.streaming_packets && capabilities.version == 0x0100 {
                    let codec = bytes[1];
                    if !capabilities.supports_chromecast_codec(codec) {
                        return Err(PipelineError::UnsupportedChromecastCodec(codec));
                    }
                    if step_index > 88 {
                        return Err(PipelineError::InvalidAudioPacket);
                    }
                }
                if self.session.state() != VoiceSessionState::Streaming
                    && !(self.streaming_packets && self.session.state() == VoiceSessionState::Idle)
                {
                    return Err(PipelineError::SyncOutsideStream);
                }
                let discarded_partial_bytes = self.accumulator.pending().len();
                self.accumulator.reset();
                self.pending_sync = Some((predictor, step_index));
                Ok(PipelineOutput::DecoderSynchronized {
                    predictor,
                    step_index,
                    discarded_partial_bytes,
                })
            }
            AtvvControlEvent::Unknown { opcode } => Ok(PipelineOutput::UnknownControl { opcode }),
        }
    }

    pub fn handle_audio(&mut self, bytes: &[u8]) -> Result<PipelineOutput, PipelineError> {
        if self.session.state() != VoiceSessionState::Streaming {
            return Err(PipelineError::AudioOutsideStream);
        }
        let capabilities = self.require_capabilities()?.clone();
        let frame_size = capabilities.frame_size;
        if self.streaming_packets && capabilities.version < 0x0100 {
            if bytes.len() != frame_size || bytes.len() < 6 || bytes[5] > 88 {
                return Err(PipelineError::InvalidAudioPacket);
            }
            let predictor = i16::from_be_bytes([bytes[3], bytes[4]]);
            self.decoder
                .reset(i32::from(predictor), i32::from(bytes[5]));
            let mut decoded = vec![predictor];
            decoded.extend(self.decoder.decode(&bytes[6..]));
            let samples = process_pcm(&decoded, self.gain_db);
            let samples = if capabilities.sample_rate == 8_000 {
                samples
                    .into_iter()
                    .flat_map(|sample| [sample, sample])
                    .collect()
            } else {
                samples
            };
            self.session.apply(VoiceSessionEvent::AudioAccepted {
                sample_count: samples.len(),
            })?;
            return Ok(PipelineOutput::Samples {
                generation: self.session.generation(),
                samples,
            });
        }
        let frames = if self.streaming_packets {
            if bytes.is_empty() {
                Vec::new()
            } else {
                vec![bytes.to_vec()]
            }
        } else {
            self.accumulator.append(bytes, frame_size)
        };
        let mut samples = Vec::with_capacity(frames.len() * frame_size * 2);
        for frame in frames {
            if let Some((predictor, step_index)) = self.pending_sync.take() {
                self.decoder
                    .reset(i32::from(predictor), i32::from(step_index));
            }
            let decoded = self.decoder.decode(&frame);
            samples.extend(process_pcm(&decoded, self.gain_db));
        }
        if self.streaming_packets && capabilities.sample_rate == 8_000 {
            samples = samples
                .into_iter()
                .flat_map(|sample| [sample, sample])
                .collect();
        }
        self.session.apply(VoiceSessionEvent::AudioAccepted {
            sample_count: samples.len(),
        })?;
        Ok(PipelineOutput::Samples {
            generation: self.session.generation(),
            samples,
        })
    }

    pub fn complete_drain(&mut self, generation: u64) -> Result<(), PipelineError> {
        self.session
            .apply(VoiceSessionEvent::DrainCompleted { generation })?;
        self.decoder.reset(0, 0);
        Ok(())
    }

    pub fn interrupt(&mut self) {
        let _ = self.session.apply(VoiceSessionEvent::Interrupted);
        self.accumulator.reset();
        self.pending_sync = None;
        self.decoder.reset(0, 0);
    }

    fn require_capabilities(&self) -> Result<&AtvvCapabilities, PipelineError> {
        self.capabilities
            .as_ref()
            .ok_or(PipelineError::CapabilitiesNotConfirmed)
    }

    fn require_capabilities_mut(&mut self) -> Result<&mut AtvvCapabilities, PipelineError> {
        self.capabilities
            .as_mut()
            .ok_or(PipelineError::CapabilitiesNotConfirmed)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PipelineError {
    #[error("invalid ATVV audio packet")]
    InvalidAudioPacket,
    #[error(transparent)]
    Protocol(#[from] AtvvError),
    #[error(transparent)]
    Session(#[from] VoiceSessionError),
    #[error("ATVV capabilities are not confirmed")]
    CapabilitiesNotConfirmed,
    #[error("ATVV sample rate {0} Hz is unsupported")]
    UnsupportedSampleRate(u32),
    #[error("Chromecast selected an unsupported ATVV codec 0x{0:02X}")]
    UnsupportedChromecastCodec(u8),
    #[error("audio arrived outside an active ATVV stream")]
    AudioOutsideStream,
    #[error("decoder synchronization arrived outside an active stream")]
    SyncOutsideStream,
    #[error("ATVV stream stop arrived without a session")]
    MissingSession,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromecast_v04_decodes_packet_headers_and_converts_8k_to_output_16k() {
        let mut pipeline = AtvvVoicePipeline::default();
        pipeline.set_streaming_packets(true);
        pipeline
            .handle_control(&[0x0B, 0, 4, 0, 1, 0, 8, 0, 0])
            .unwrap();
        pipeline.handle_control(&[4]).unwrap();
        let PipelineOutput::Samples { samples, .. } =
            pipeline.handle_audio(&[0, 1, 0, 3, 0xE8, 0, 0, 0]).unwrap()
        else {
            panic!("expected v0.4 audio");
        };
        assert_eq!(samples, vec![1000; 10]);
        assert!(pipeline.handle_audio(&[0, 1]).is_err());
        pipeline.interrupt();
        assert!(pipeline.handle_audio(&[0; 8]).is_err());
    }

    #[test]
    fn chromecast_v1_decodes_notifications_without_waiting_for_fixed_frames() {
        let mut pipeline = AtvvVoicePipeline::default();
        pipeline.set_streaming_packets(true);
        pipeline.handle_control(&CAPS).unwrap();
        pipeline.handle_control(&[4, 3, 2, 1]).unwrap();
        let PipelineOutput::Samples { samples, .. } = pipeline.handle_audio(&[0x11]).unwrap()
        else {
            panic!("expected partial notification");
        };
        assert_eq!(samples.len(), 2);
    }

    #[test]
    fn chromecast_rejects_unknown_protocol_and_unadvertised_codecs() {
        let mut pipeline = AtvvVoicePipeline::default();
        pipeline.set_streaming_packets(true);
        assert_eq!(
            pipeline.handle_control(&[0x0B, 0x01, 0x01, 0x02, 0x03, 0, 4]),
            Err(AtvvError::UnsupportedVersion(0x0101).into())
        );
        // A nonzero interaction field is not a codec mask.
        assert_eq!(
            pipeline.handle_control(&[0x0B, 0x01, 0x00, 0, 0x03, 0, 4, 0, 0]),
            Err(AtvvError::UnsupportedCodecMask(0).into())
        );
        assert_eq!(
            pipeline.handle_control(&[0x0B, 0x00, 0x04, 0, 0x80, 0, 8, 0, 0]),
            Err(AtvvError::UnsupportedCodecMask(0x80).into())
        );
        assert_eq!(
            pipeline.handle_control(&[0x0B, 0x00, 0x04, 0, 0x01, 0, 5, 0, 0]),
            Err(AtvvError::InvalidChromecastFrameSize(5).into())
        );
        pipeline.handle_control(&CAPS).unwrap();
        assert_eq!(
            pipeline.handle_control(&[0x04, 0, 1, 7]),
            Err(PipelineError::UnsupportedChromecastCodec(1))
        );
        assert_eq!(
            pipeline.handle_control(&[0x0A, 1, 0, 0, 0, 0, 0]),
            Err(PipelineError::UnsupportedChromecastCodec(1))
        );
        assert_eq!(pipeline.state(), VoiceSessionState::Idle);
        assert_eq!(pipeline.capabilities().unwrap().selected_codec, 2);
        pipeline.handle_control(&[0x04, 1, 2, 7]).unwrap();
        assert_eq!(pipeline.capabilities().unwrap().interaction, 3);
    }

    #[test]
    fn chromecast_sync_before_start_survives_until_first_audio() {
        let mut pipeline = AtvvVoicePipeline::default();
        pipeline.set_streaming_packets(true);
        pipeline.handle_control(&CAPS).unwrap();
        pipeline.handle_control(&[0x08]).unwrap();
        pipeline
            .handle_control(&[0x0A, 2, 0, 0, 0x03, 0xE8, 10])
            .unwrap();
        pipeline.handle_control(&[0x04, 3, 2, 1]).unwrap();
        let PipelineOutput::Samples { samples, .. } = pipeline.handle_audio(&[0; 4]).unwrap()
        else {
            panic!("expected decoded audio");
        };
        assert!(samples.iter().all(|sample| *sample >= 1000));
        pipeline.handle_control(&[0]).unwrap();
        pipeline.complete_drain(pipeline.generation()).unwrap();
        pipeline.handle_control(&[0x04, 3, 2, 2]).unwrap();
        let PipelineOutput::Samples { samples, .. } = pipeline.handle_audio(&[0; 4]).unwrap()
        else {
            panic!("expected next stream");
        };
        assert!(
            samples.iter().all(|sample| *sample == 0),
            "sync cannot leak to next stream"
        );
    }

    const CAPS: [u8; 7] = [0x0B, 0x01, 0x00, 0x02, 0x03, 0, 4];

    #[test]
    fn runs_first_complete_voice_journey() {
        let mut pipeline = AtvvVoicePipeline::default();
        assert!(matches!(
            pipeline.handle_control(&CAPS).unwrap(),
            PipelineOutput::Ready(_)
        ));
        assert_eq!(
            pipeline.handle_control(&[0x04, 0x03, 0x02, 0x07]).unwrap(),
            PipelineOutput::StreamStarted {
                session_id: 7,
                generation: 1,
            }
        );
        assert_eq!(
            pipeline.handle_audio(&[0x00, 0x7F]).unwrap(),
            PipelineOutput::Samples {
                generation: 1,
                samples: Vec::new(),
            }
        );
        let samples = pipeline.handle_audio(&[0x80, 0xFF]).unwrap();
        assert_eq!(
            samples,
            PipelineOutput::Samples {
                generation: 1,
                samples: vec![0, 2, 0, -13, -22, -34, -87, -184],
            }
        );
        assert_eq!(
            pipeline.handle_control(&[0x00]).unwrap(),
            PipelineOutput::StreamStopped {
                session_id: 7,
                generation: 1,
                discarded_partial_bytes: 0,
            }
        );
        assert_eq!(pipeline.state(), VoiceSessionState::Draining);
        pipeline.complete_drain(1).unwrap();
        assert_eq!(pipeline.state(), VoiceSessionState::Idle);
    }

    /// 增益（对齐 Mac `PCMPostprocessor` 的 dB 缩放）：6.02 dB ≈ ×2；
    /// 越界值按 0–24 dB 钳制，非有限值回落 0 dB。平台层在每批音频前
    /// `set_gain_db`，因此这里的输入就是"当前生效值"。
    #[test]
    fn applies_decibel_gain_to_decoded_frames() {
        fn decode_with_gain(gain_db: f32, bytes: &[u8]) -> Vec<i16> {
            let mut pipeline = AtvvVoicePipeline::default();
            pipeline.handle_control(&CAPS).unwrap();
            pipeline.handle_control(&[0x04, 0x03, 0x02, 0x01]).unwrap();
            pipeline.set_gain_db(gain_db);
            let PipelineOutput::Samples { samples, .. } = pipeline.handle_audio(bytes).unwrap()
            else {
                panic!("expected samples");
            };
            samples
        }

        let frame = [0x00, 0x7F, 0x80, 0xFF];
        assert_eq!(
            decode_with_gain(0.0, &frame),
            vec![0, 2, 0, -13, -22, -34, -87, -184]
        );
        assert_eq!(
            decode_with_gain(6.0206, &frame),
            vec![0, 4, 0, -26, -44, -68, -174, -368]
        );
        // 越界与非法值：按 supported range 钳制，而不是透传或清零整帧。
        assert_eq!(
            decode_with_gain(30.0, &frame),
            decode_with_gain(24.0, &frame)
        );
        assert_eq!(
            decode_with_gain(f32::NAN, &frame),
            decode_with_gain(0.0, &frame)
        );
    }

    #[test]
    fn runs_rc001_short_voice_without_microphone_open() {
        let mut pipeline = AtvvVoicePipeline::default();
        assert!(matches!(
            pipeline.handle_control(&CAPS).unwrap(),
            PipelineOutput::Ready(_)
        ));
        assert!(matches!(
            pipeline.handle_control(&[0x04, 0x03, 0x02, 0x01]).unwrap(),
            PipelineOutput::StreamStarted {
                session_id: 1,
                generation: 1,
            }
        ));
        assert!(matches!(
            pipeline.handle_audio(&[0x11; 4]).unwrap(),
            PipelineOutput::Samples { generation: 1, .. }
        ));
        assert!(matches!(
            pipeline.handle_control(&[0x00]).unwrap(),
            PipelineOutput::StreamStopped {
                session_id: 1,
                generation: 1,
                ..
            }
        ));
    }

    #[test]
    fn applies_sync_to_next_complete_frame() {
        let mut pipeline = AtvvVoicePipeline::default();
        pipeline.handle_control(&CAPS).unwrap();
        pipeline.handle_control(&[0x04, 0x03, 0x02, 0x01]).unwrap();
        pipeline.handle_audio(&[0x00, 0x00]).unwrap();
        assert_eq!(
            pipeline
                .handle_control(&[0x0A, 0, 0, 0, 0, 100, 10])
                .unwrap(),
            PipelineOutput::DecoderSynchronized {
                predictor: 100,
                step_index: 10,
                discarded_partial_bytes: 2,
            }
        );
        let output = pipeline.handle_audio(&[0, 0, 0, 0]).unwrap();
        let PipelineOutput::Samples { samples, .. } = output else {
            panic!("expected samples");
        };
        assert!(samples.iter().all(|sample| *sample >= 100));
    }

    #[test]
    fn rejects_audio_before_start_and_after_stop() {
        let mut pipeline = AtvvVoicePipeline::default();
        pipeline.handle_control(&CAPS).unwrap();
        assert_eq!(
            pipeline.handle_audio(&[0; 4]),
            Err(PipelineError::AudioOutsideStream)
        );
        pipeline.handle_control(&[0x04, 0x03, 0x02, 0x01]).unwrap();
        pipeline.handle_control(&[0x00]).unwrap();
        assert_eq!(
            pipeline.handle_audio(&[0; 4]),
            Err(PipelineError::AudioOutsideStream)
        );
    }

    #[test]
    fn rejects_eight_kilohertz_capabilities() {
        let mut pipeline = AtvvVoicePipeline::default();
        assert_eq!(
            pipeline.handle_control(&[0x0B, 0x01, 0, 0x01, 0x03, 0, 120]),
            Err(PipelineError::UnsupportedSampleRate(8_000))
        );
    }
}
