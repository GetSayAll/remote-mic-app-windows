use crate::{
    process_pcm, AtvvCapabilities, AtvvControlEvent, AtvvError, FrameAccumulator, ImaAdpcmDecoder,
    VoiceSession, VoiceSessionError, VoiceSessionEvent, VoiceSessionState,
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
        self.gain_db = if gain_db.is_finite() {
            gain_db.clamp(0.0, 24.0)
        } else {
            0.0
        };
    }

    pub fn handle_control(&mut self, bytes: &[u8]) -> Result<PipelineOutput, PipelineError> {
        match AtvvControlEvent::parse(bytes)? {
            AtvvControlEvent::Capabilities(capabilities) => {
                if !capabilities.supports_sayall_audio() {
                    return Err(PipelineError::UnsupportedSampleRate(
                        capabilities.sample_rate,
                    ));
                }
                self.capabilities = Some(capabilities.clone());
                Ok(PipelineOutput::Ready(capabilities))
            }
            AtvvControlEvent::MicrophoneOpenRequested => {
                self.require_capabilities()?;
                Ok(PipelineOutput::MicrophoneOpenRequested)
            }
            AtvvControlEvent::StreamStarted {
                interaction,
                codec,
                session_id,
            } => {
                let capabilities = self.require_capabilities_mut()?;
                if let Some(interaction) = interaction {
                    capabilities.interaction = interaction;
                }
                if let Some(codec) = codec {
                    capabilities.selected_codec = codec;
                    capabilities.sample_rate = if codec == 0x02 { 16_000 } else { 8_000 };
                }
                if !capabilities.supports_sayall_audio() {
                    return Err(PipelineError::UnsupportedSampleRate(
                        capabilities.sample_rate,
                    ));
                }
                self.accumulator.reset();
                self.pending_sync = None;
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
                if self.session.state() != VoiceSessionState::Streaming {
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
        let frame_size = self.require_capabilities()?.frame_size;
        let frames = self.accumulator.append(bytes, frame_size);
        let mut samples = Vec::with_capacity(frames.len() * frame_size * 2);
        for frame in frames {
            if let Some((predictor, step_index)) = self.pending_sync.take() {
                self.decoder
                    .reset(i32::from(predictor), i32::from(step_index));
            }
            let decoded = self.decoder.decode(&frame);
            samples.extend(process_pcm(&decoded, self.gain_db));
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
    #[error(transparent)]
    Protocol(#[from] AtvvError),
    #[error(transparent)]
    Session(#[from] VoiceSessionError),
    #[error("ATVV capabilities are not confirmed")]
    CapabilitiesNotConfirmed,
    #[error("ATVV sample rate {0} Hz is unsupported")]
    UnsupportedSampleRate(u32),
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
