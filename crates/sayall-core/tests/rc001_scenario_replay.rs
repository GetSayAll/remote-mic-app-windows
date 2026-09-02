use sayall_core::{AtvvVoicePipeline, PipelineOutput, VoiceSessionState};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rc001Scenario {
    schema_version: u32,
    id: String,
    source_scenario: String,
    model_number: String,
    capabilities_hex: String,
    events: Vec<ScenarioEvent>,
    expected: ExpectedResult,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScenarioEvent {
    at_milliseconds: u64,
    channel: ScenarioChannel,
    value_hex: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ScenarioChannel {
    Control,
    Audio,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedResult {
    session_id: u8,
    frame_size: usize,
    decoded_samples: usize,
}

fn load_scenario() -> Rc001Scenario {
    serde_json::from_str(include_str!("fixtures/rc001-short-voice.json"))
        .expect("RC001 replay fixture must be valid JSON")
}

fn decode_hex(raw: &str) -> Vec<u8> {
    assert_eq!(raw.len() % 2, 0, "hex fixture must contain complete bytes");
    raw.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).expect("hex fixture must be ASCII");
            u8::from_str_radix(text, 16).expect("hex fixture must contain hexadecimal bytes")
        })
        .collect()
}

fn prepared_pipeline(scenario: &Rc001Scenario) -> AtvvVoicePipeline {
    let mut pipeline = AtvvVoicePipeline::default();
    let output = pipeline
        .handle_control(&decode_hex(&scenario.capabilities_hex))
        .expect("RC001 capabilities must be accepted");
    let PipelineOutput::Ready(capabilities) = output else {
        panic!("expected RC001 capabilities output");
    };
    assert_eq!(capabilities.sample_rate, 16_000);
    assert_eq!(capabilities.frame_size, scenario.expected.frame_size);
    pipeline
}

fn events_for(scenario: &Rc001Scenario, channel: ScenarioChannel) -> Vec<ScenarioEvent> {
    scenario
        .events
        .iter()
        .filter(|event| event.channel == channel)
        .cloned()
        .collect()
}

#[test]
fn replays_rc001_short_voice_fixture_without_microphone_open() {
    let scenario = load_scenario();
    assert_eq!(scenario.schema_version, 1);
    assert_eq!(scenario.id, "sayall.rc001-short-voice");
    assert_eq!(scenario.model_number, "RC001");
    assert!(scenario.source_scenario.contains("hardware-simulation"));
    assert!(scenario
        .events
        .windows(2)
        .all(|events| events[0].at_milliseconds < events[1].at_milliseconds));

    let mut pipeline = prepared_pipeline(&scenario);
    let mut decoded_chunks = Vec::new();
    let mut microphone_open_requested = false;

    for event in &scenario.events {
        let bytes = decode_hex(&event.value_hex);
        match event.channel {
            ScenarioChannel::Control => match pipeline.handle_control(&bytes).unwrap() {
                PipelineOutput::MicrophoneOpenRequested => microphone_open_requested = true,
                PipelineOutput::StreamStarted {
                    session_id,
                    generation,
                } => {
                    assert_eq!(session_id, scenario.expected.session_id);
                    assert_eq!(generation, 1);
                }
                PipelineOutput::StreamStopped {
                    session_id,
                    generation,
                    discarded_partial_bytes,
                } => {
                    assert_eq!(session_id, scenario.expected.session_id);
                    assert_eq!(generation, 1);
                    assert_eq!(discarded_partial_bytes, 0);
                    pipeline.complete_drain(generation).unwrap();
                }
                output => panic!("unexpected control output: {output:?}"),
            },
            ScenarioChannel::Audio => {
                let PipelineOutput::Samples {
                    generation,
                    samples,
                } = pipeline.handle_audio(&bytes).unwrap()
                else {
                    panic!("expected decoded RC001 samples");
                };
                assert_eq!(generation, 1);
                decoded_chunks.push(samples.len());
            }
        }
    }

    assert!(!microphone_open_requested);
    assert_eq!(decoded_chunks, vec![0, scenario.expected.decoded_samples]);
    assert_eq!(pipeline.state(), VoiceSessionState::Idle);
}

#[test]
fn handles_twenty_rapid_rc001_press_release_sessions() {
    let scenario = load_scenario();
    let control = events_for(&scenario, ScenarioChannel::Control);
    let start = decode_hex(&control[0].value_hex);
    let stop = decode_hex(&control[1].value_hex);
    let mut pipeline = prepared_pipeline(&scenario);

    for index in 1..=20_u64 {
        assert!(matches!(
            pipeline.handle_control(&start).unwrap(),
            PipelineOutput::StreamStarted { generation, .. } if generation == index
        ));
        let PipelineOutput::StreamStopped {
            generation,
            discarded_partial_bytes,
            ..
        } = pipeline.handle_control(&stop).unwrap()
        else {
            panic!("expected stream stop");
        };
        assert_eq!(generation, index);
        assert_eq!(discarded_partial_bytes, 0);
        pipeline.complete_drain(generation).unwrap();
    }

    assert_eq!(pipeline.generation(), 20);
    assert_eq!(pipeline.state(), VoiceSessionState::Idle);
}

#[test]
fn handles_twenty_complete_rc001_voice_sessions() {
    let scenario = load_scenario();
    let control = events_for(&scenario, ScenarioChannel::Control);
    let audio = events_for(&scenario, ScenarioChannel::Audio);
    let start = decode_hex(&control[0].value_hex);
    let stop = decode_hex(&control[1].value_hex);
    let mut pipeline = prepared_pipeline(&scenario);

    for index in 1..=20_u64 {
        pipeline.handle_control(&start).unwrap();
        let mut decoded_samples = 0;
        for event in &audio {
            let PipelineOutput::Samples {
                generation,
                samples,
            } = pipeline
                .handle_audio(&decode_hex(&event.value_hex))
                .unwrap()
            else {
                panic!("expected decoded samples");
            };
            assert_eq!(generation, index);
            decoded_samples += samples.len();
        }
        assert_eq!(decoded_samples, scenario.expected.decoded_samples);
        let PipelineOutput::StreamStopped { generation, .. } =
            pipeline.handle_control(&stop).unwrap()
        else {
            panic!("expected stream stop");
        };
        pipeline.complete_drain(generation).unwrap();
    }

    assert_eq!(pipeline.generation(), 20);
    assert_eq!(pipeline.state(), VoiceSessionState::Idle);
}

#[test]
fn clears_partial_rc001_audio_before_recovery_session() {
    let scenario = load_scenario();
    let control = events_for(&scenario, ScenarioChannel::Control);
    let audio = events_for(&scenario, ScenarioChannel::Audio);
    let start = decode_hex(&control[0].value_hex);
    let stop = decode_hex(&control[1].value_hex);
    let mut pipeline = prepared_pipeline(&scenario);

    pipeline.handle_control(&start).unwrap();
    let first_partial = pipeline
        .handle_audio(&decode_hex(&audio[0].value_hex))
        .unwrap();
    assert!(matches!(
        first_partial,
        PipelineOutput::Samples { ref samples, .. } if samples.is_empty()
    ));
    pipeline.interrupt();
    assert_eq!(pipeline.state(), VoiceSessionState::Idle);

    assert!(matches!(
        pipeline.handle_control(&start).unwrap(),
        PipelineOutput::StreamStarted { generation: 2, .. }
    ));
    let mut decoded_samples = 0;
    for event in &audio {
        let PipelineOutput::Samples { samples, .. } = pipeline
            .handle_audio(&decode_hex(&event.value_hex))
            .unwrap()
        else {
            panic!("expected decoded samples");
        };
        decoded_samples += samples.len();
    }
    assert_eq!(decoded_samples, scenario.expected.decoded_samples);
    let PipelineOutput::StreamStopped {
        generation,
        discarded_partial_bytes,
        ..
    } = pipeline.handle_control(&stop).unwrap()
    else {
        panic!("expected stream stop");
    };
    assert_eq!(generation, 2);
    assert_eq!(discarded_partial_bytes, 0);
    pipeline.complete_drain(generation).unwrap();
    assert_eq!(pipeline.state(), VoiceSessionState::Idle);
}
