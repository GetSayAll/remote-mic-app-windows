use serde::{Deserialize, Serialize};
#[cfg(any(windows, test))]
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalDictationStatus {
    Idle,
    Armed,
    Capturing,
    Ready,
    Cancelled,
    Expired,
}

#[cfg(any(windows, test))]
mod capture {
    use super::{Duration, Instant, LocalDictationStatus};

    const ARM_TTL: Duration = Duration::from_secs(30);
    const CAPTURE_TTL: Duration = Duration::from_secs(60);
    const READY_TTL: Duration = Duration::from_secs(30);
    const MAX_SAMPLES: usize = 60 * 16_000;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Stream {
        pub connection: u64,
        pub generation: u64,
        pub session_id: u8,
    }

    enum Phase {
        Armed(u64),
        Reserved(u64),
        Capturing { stream: Stream, samples: Vec<i16> },
        Ready(Vec<i16>),
        Cancelled,
        Expired,
        Idle,
    }

    pub(crate) struct Capture {
        token: u64,
        deadline: Option<Instant>,
        phase: Phase,
    }

    impl Default for Capture {
        fn default() -> Self {
            Self {
                token: 0,
                deadline: None,
                phase: Phase::Idle,
            }
        }
    }

    impl Capture {
        pub fn expire(&mut self, now: Instant) {
            if self.deadline.is_some_and(|deadline| now >= deadline) {
                self.phase = Phase::Expired;
                self.deadline = None;
            }
        }

        pub fn deadline(&self) -> Option<Instant> {
            self.deadline
        }

        pub fn status(&mut self, token: u64, now: Instant) -> LocalDictationStatus {
            self.expire(now);
            if token != self.token {
                return LocalDictationStatus::Idle;
            }
            match self.phase {
                Phase::Armed(_) | Phase::Reserved(_) => LocalDictationStatus::Armed,
                Phase::Capturing { .. } => LocalDictationStatus::Capturing,
                Phase::Ready(_) => LocalDictationStatus::Ready,
                Phase::Cancelled => LocalDictationStatus::Cancelled,
                Phase::Expired => LocalDictationStatus::Expired,
                Phase::Idle => LocalDictationStatus::Idle,
            }
        }

        pub fn arm(&mut self, connection: u64, now: Instant) -> Option<u64> {
            self.expire(now);
            if self.deadline.is_some() {
                return None;
            }
            self.token = self.token.checked_add(1)?;
            self.phase = Phase::Armed(connection);
            self.deadline = Some(now + ARM_TTL);
            Some(self.token)
        }

        pub fn reserve(&mut self, connection: u64, now: Instant) -> Option<u64> {
            self.expire(now);
            if !matches!(self.phase, Phase::Armed(expected) if expected == connection) {
                return None;
            }
            self.phase = Phase::Reserved(connection);
            Some(self.token)
        }

        pub fn start(&mut self, token: u64, stream: Stream, now: Instant) -> bool {
            self.expire(now);
            if token != self.token
                || !matches!(self.phase, Phase::Reserved(expected) if expected == stream.connection)
            {
                return false;
            }
            self.phase = Phase::Capturing {
                stream,
                samples: Vec::new(),
            };
            self.deadline = Some(now + CAPTURE_TTL);
            true
        }

        pub fn append(&mut self, token: u64, key: Stream, batch: &[i16], now: Instant) -> usize {
            self.expire(now);
            if token != self.token {
                return 0;
            }
            let Phase::Capturing { stream, samples } = &mut self.phase else {
                return 0;
            };
            if *stream != key {
                return 0;
            }
            if batch.len() > MAX_SAMPLES - samples.len() {
                self.phase = Phase::Expired;
                self.deadline = None;
                return 0;
            }
            samples.extend_from_slice(batch);
            batch.len()
        }

        pub fn stop(&mut self, token: u64, key: Stream, now: Instant) -> bool {
            self.expire(now);
            if token != self.token
                || !matches!(&self.phase, Phase::Capturing { stream, .. } if *stream == key)
            {
                return false;
            }
            let Phase::Capturing { samples, .. } =
                std::mem::replace(&mut self.phase, Phase::Cancelled)
            else {
                unreachable!("verified matching capture")
            };
            if samples.is_empty() {
                self.deadline = None;
                return false;
            }
            self.phase = Phase::Ready(samples);
            self.deadline = Some(now + READY_TTL);
            true
        }

        pub fn take(&mut self, token: u64, now: Instant) -> Option<Vec<i16>> {
            self.expire(now);
            if token != self.token || !matches!(self.phase, Phase::Ready(_)) {
                return None;
            }
            self.deadline = None;
            let Phase::Ready(samples) = std::mem::replace(&mut self.phase, Phase::Idle) else {
                unreachable!("verified ready capture")
            };
            Some(samples)
        }

        pub fn cancel(&mut self, token: u64, now: Instant) -> bool {
            self.expire(now);
            token == self.token && self.invalidate()
        }

        pub fn invalidate(&mut self) -> bool {
            if self.deadline.take().is_none() {
                return false;
            }
            self.phase = Phase::Cancelled;
            true
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use sayall_core::{AtvvVoicePipeline, PipelineOutput};

        const STREAM: Stream = Stream {
            connection: 2,
            generation: 3,
            session_id: 4,
        };

        #[test]
        fn accepts_only_matching_stream_and_hands_off_once() {
            let now = Instant::now();
            let mut capture = Capture::default();
            let token = capture.arm(STREAM.connection, now).unwrap();
            assert_eq!(capture.arm(STREAM.connection, now), None);
            assert_eq!(capture.reserve(1, now), None);
            assert_eq!(capture.reserve(STREAM.connection, now), Some(token));
            assert_eq!(capture.reserve(STREAM.connection, now), None);
            assert!(capture.start(token, STREAM, now));
            assert_eq!(capture.status(token, now), LocalDictationStatus::Capturing);
            assert_eq!(capture.append(token, STREAM, &[42, -42], now), 2);
            assert_eq!(capture.take(token, now), None);
            for stale in [
                Stream {
                    connection: 1,
                    ..STREAM
                },
                Stream {
                    generation: 1,
                    ..STREAM
                },
                Stream {
                    session_id: 1,
                    ..STREAM
                },
            ] {
                assert_eq!(capture.append(token, stale, &[1], now), 0);
                assert!(!capture.stop(token, stale, now));
            }
            assert!(capture.stop(token, STREAM, now));
            assert!(!capture.stop(token, STREAM, now));
            assert_eq!(capture.take(token + 1, now), None);
            assert_eq!(capture.take(token, now), Some(vec![42, -42]));
            assert_eq!(capture.take(token, now), None);
        }

        #[test]
        fn cancellation_after_open_blocks_late_start_and_ready_can_be_revoked() {
            let now = Instant::now();
            let mut capture = Capture::default();
            let token = capture.arm(STREAM.connection, now).unwrap();
            capture.reserve(STREAM.connection, now);
            assert!(capture.cancel(token, now));
            assert!(!capture.start(token, STREAM, now));
            assert_eq!(capture.status(token, now), LocalDictationStatus::Cancelled);
            let next = capture.arm(STREAM.connection, now).unwrap();
            assert!(!capture.cancel(token, now));
            capture.reserve(STREAM.connection, now);
            assert!(capture.start(next, STREAM, now));
            capture.append(next, STREAM, &[7], now);
            assert!(capture.stop(next, STREAM, now));
            assert!(capture.invalidate());
            assert_eq!(capture.take(next, now), None);
        }

        #[test]
        fn expires_arm_and_capture_and_rejects_empty_or_oversized_audio() {
            let now = Instant::now();
            let mut capture = Capture::default();
            let token = capture.arm(STREAM.connection, now).unwrap();
            assert_eq!(capture.deadline(), Some(now + ARM_TTL));
            assert_eq!(capture.reserve(STREAM.connection, now + ARM_TTL), None);
            assert_eq!(
                capture.status(token, now + ARM_TTL),
                LocalDictationStatus::Expired
            );

            for batch in [vec![], vec![1; MAX_SAMPLES + 1]] {
                let time = now + ARM_TTL;
                let token = capture.arm(STREAM.connection, time).unwrap();
                capture.reserve(STREAM.connection, time);
                capture.start(token, STREAM, time);
                assert_eq!(capture.append(token, STREAM, &batch, time), 0);
                assert!(!capture.stop(token, STREAM, time));
                assert_eq!(capture.take(token, time), None);
            }

            let time = now + ARM_TTL;
            let token = capture.arm(STREAM.connection, time).unwrap();
            capture.reserve(STREAM.connection, time);
            capture.start(token, STREAM, time);
            capture.append(token, STREAM, &[1], time);
            assert!(!capture.stop(token, STREAM, time + CAPTURE_TTL));
            assert_eq!(
                capture.status(token, time + CAPTURE_TTL),
                LocalDictationStatus::Expired
            );
        }

        #[test]
        fn decoded_chromecast_pcm_is_only_ready_after_protocol_stop() {
            let now = Instant::now();
            let mut pipeline = AtvvVoicePipeline::default();
            pipeline.set_streaming_packets(true);
            pipeline
                .handle_control(&[0x0b, 0x01, 0, 2, 3, 0, 4])
                .unwrap();
            let PipelineOutput::StreamStarted {
                session_id,
                generation,
            } = pipeline.handle_control(&[0x04, 0, 2, 7]).unwrap()
            else {
                panic!("stream did not start")
            };
            let stream = Stream {
                connection: 1,
                generation,
                session_id,
            };
            let mut capture = Capture::default();
            let token = capture.arm(stream.connection, now).unwrap();
            capture.reserve(stream.connection, now);
            capture.start(token, stream, now);
            let PipelineOutput::Samples { samples, .. } =
                pipeline.handle_audio(&[0x11, 0x22]).unwrap()
            else {
                panic!("no PCM")
            };
            assert!(!samples.is_empty());
            capture.append(token, stream, &samples, now);
            assert_eq!(capture.take(token, now), None);
            assert!(matches!(
                pipeline.handle_control(&[0]),
                Ok(PipelineOutput::StreamStopped { .. })
            ));
            assert!(capture.stop(token, stream, now));
            assert_eq!(capture.take(token, now), Some(samples));
        }

        #[test]
        fn completed_audio_is_discarded_when_handoff_expires() {
            let now = Instant::now();
            let mut capture = Capture::default();
            let token = capture.arm(STREAM.connection, now).unwrap();
            capture.reserve(STREAM.connection, now);
            capture.start(token, STREAM, now);
            capture.append(token, STREAM, &[1], now);
            assert!(capture.stop(token, STREAM, now));
            assert_eq!(capture.status(token, now), LocalDictationStatus::Ready);
            assert_eq!(capture.take(token, now + READY_TTL), None);
            assert_eq!(
                capture.status(token, now + READY_TTL),
                LocalDictationStatus::Expired
            );
        }
    }
}

#[cfg(any(windows, test))]
pub(crate) use capture::{Capture, Stream};
