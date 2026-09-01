use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSessionState {
    Idle,
    Streaming,
    Draining,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceSessionEvent {
    StreamStarted { session_id: u8 },
    AudioAccepted { sample_count: usize },
    StreamStopped { session_id: u8 },
    DrainCompleted { generation: u64 },
    Interrupted,
}

#[derive(Debug, Clone)]
pub struct VoiceSession {
    state: VoiceSessionState,
    generation: u64,
    session_id: Option<u8>,
    accepted_samples: u64,
}

impl Default for VoiceSession {
    fn default() -> Self {
        Self {
            state: VoiceSessionState::Idle,
            generation: 0,
            session_id: None,
            accepted_samples: 0,
        }
    }
}

impl VoiceSession {
    pub fn state(&self) -> VoiceSessionState {
        self.state
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn session_id(&self) -> Option<u8> {
        self.session_id
    }

    pub fn accepted_samples(&self) -> u64 {
        self.accepted_samples
    }

    pub fn apply(&mut self, event: VoiceSessionEvent) -> Result<(), VoiceSessionError> {
        match event {
            VoiceSessionEvent::StreamStarted { session_id } => {
                if self.state != VoiceSessionState::Idle {
                    return Err(VoiceSessionError::AlreadyActive);
                }
                self.generation = self.generation.wrapping_add(1);
                self.session_id = Some(session_id);
                self.accepted_samples = 0;
                self.state = VoiceSessionState::Streaming;
            }
            VoiceSessionEvent::AudioAccepted { sample_count } => {
                if self.state != VoiceSessionState::Streaming {
                    return Err(VoiceSessionError::AudioOutsideStream);
                }
                self.accepted_samples = self.accepted_samples.saturating_add(sample_count as u64);
            }
            VoiceSessionEvent::StreamStopped { session_id } => {
                if self.state != VoiceSessionState::Streaming {
                    return Err(VoiceSessionError::NotStreaming);
                }
                if self.session_id != Some(session_id) {
                    return Err(VoiceSessionError::StaleSession);
                }
                self.state = VoiceSessionState::Draining;
            }
            VoiceSessionEvent::DrainCompleted { generation } => {
                if self.state != VoiceSessionState::Draining {
                    return Err(VoiceSessionError::NotDraining);
                }
                if generation != self.generation {
                    return Err(VoiceSessionError::StaleGeneration);
                }
                self.finish();
            }
            VoiceSessionEvent::Interrupted => self.finish(),
        }
        Ok(())
    }

    fn finish(&mut self) {
        self.state = VoiceSessionState::Idle;
        self.session_id = None;
        self.accepted_samples = 0;
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VoiceSessionError {
    #[error("a voice session is already active")]
    AlreadyActive,
    #[error("audio arrived outside an active stream")]
    AudioOutsideStream,
    #[error("voice session is not streaming")]
    NotStreaming,
    #[error("voice session is not draining")]
    NotDraining,
    #[error("stream stop belongs to another session")]
    StaleSession,
    #[error("drain completion belongs to an earlier generation")]
    StaleGeneration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_drain_before_returning_to_idle() {
        let mut session = VoiceSession::default();
        session
            .apply(VoiceSessionEvent::StreamStarted { session_id: 7 })
            .unwrap();
        session
            .apply(VoiceSessionEvent::AudioAccepted { sample_count: 160 })
            .unwrap();
        session
            .apply(VoiceSessionEvent::StreamStopped { session_id: 7 })
            .unwrap();
        assert_eq!(session.state(), VoiceSessionState::Draining);
        let generation = session.generation();
        session
            .apply(VoiceSessionEvent::DrainCompleted { generation })
            .unwrap();
        assert_eq!(session.state(), VoiceSessionState::Idle);
    }

    #[test]
    fn stale_stop_cannot_end_current_session() {
        let mut session = VoiceSession::default();
        session
            .apply(VoiceSessionEvent::StreamStarted { session_id: 2 })
            .unwrap();
        assert_eq!(
            session.apply(VoiceSessionEvent::StreamStopped { session_id: 1 }),
            Err(VoiceSessionError::StaleSession)
        );
        assert_eq!(session.state(), VoiceSessionState::Streaming);
    }

    #[test]
    fn interruption_releases_every_state() {
        let mut session = VoiceSession::default();
        session
            .apply(VoiceSessionEvent::StreamStarted { session_id: 9 })
            .unwrap();
        session.apply(VoiceSessionEvent::Interrupted).unwrap();
        assert_eq!(session.state(), VoiceSessionState::Idle);
    }
}
