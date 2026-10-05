//! The user's microphone choice and the PCM writer refer to opposite ends of a
//! virtual cable. Pair them by the adapter's PnP identity, never endpoint names.
use crate::{AudioEndpoint, AudioPhase, AudioSnapshot};
use sayall_core::CaptureInputSettings;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRoutePhase {
    Paired,
    Manual,
    Unconfigured,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRouteReason {
    CaptureMissing,
    RenderMissing,
    Ambiguous,
    Unsupported,
    IdentityChanged,
    RenderMismatch,
    AudioUnready,
    MetadataUnavailable,
    EnumerationFailed,
}

impl AudioRouteReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CaptureMissing => "capture_missing",
            Self::RenderMissing => "render_missing",
            Self::Ambiguous => "ambiguous",
            Self::Unsupported => "unsupported",
            Self::IdentityChanged => "identity_changed",
            Self::RenderMismatch => "render_mismatch",
            Self::AudioUnready => "audio_unready",
            Self::MetadataUnavailable => "metadata_unavailable",
            Self::EnumerationFailed => "enumeration_failed",
        }
    }

    pub fn user_message(self) -> &'static str {
        match self {
            Self::CaptureMissing => "所选麦克风当前不可用，请检查设备连接。",
            Self::RenderMissing => "未找到同一条虚拟音频线的声音写入端。",
            Self::Ambiguous => "检测到无法明确区分的音频端点，尚未自动连接。",
            Self::Unsupported => "此麦克风不支持自动配对，当前保留手动声音传输设置。",
            Self::IdentityChanged => "设备身份或名称已变化，请重新确认麦克风。",
            Self::RenderMismatch => "当前声音写入端与所选麦克风不属于同一条音频线。",
            Self::AudioUnready => "声音传输尚未就绪，请刷新设备状态。",
            Self::MetadataUnavailable => "无法读取音频设备的关联信息，尚未自动连接。",
            Self::EnumerationFailed => "无法读取音频设备列表，请刷新后重试。",
        }
    }
}

impl std::fmt::Display for AudioRouteReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.user_message())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioRouteSnapshot {
    pub phase: AudioRoutePhase,
    pub reason: Option<AudioRouteReason>,
    pub capture_endpoint_id: Option<String>,
    pub capture_endpoint_name: Option<String>,
    pub render_endpoint_id: Option<String>,
    pub render_endpoint_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Capture,
    Render,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cable {
    Base,
    A,
    B,
    C,
    D,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pin {
    Capture,
    Render,
    Render16,
    Other,
}

#[derive(Debug, Clone)]
struct Endpoint {
    endpoint: AudioEndpoint,
    direction: Direction,
    cable: Option<Cable>,
    pin: Pin,
    adapter_instance: Option<String>,
    metadata_complete: bool,
}

fn resolve_pair<'a>(
    endpoints: &'a [Endpoint],
    capture_id: &str,
    expected_name: &str,
    preferred_render: Option<(&str, &str)>,
) -> Result<&'a Endpoint, AudioRouteReason> {
    let capture = endpoints
        .iter()
        .find(|endpoint| {
            endpoint.direction == Direction::Capture && endpoint.endpoint.id == capture_id
        })
        .ok_or(AudioRouteReason::CaptureMissing)?;
    if capture.endpoint.name != expected_name {
        return Err(AudioRouteReason::IdentityChanged);
    }
    if !capture.metadata_complete {
        return Err(AudioRouteReason::MetadataUnavailable);
    }
    let cable = capture.cable.ok_or(AudioRouteReason::Unsupported)?;
    if capture.pin != Pin::Capture {
        return Err(AudioRouteReason::MetadataUnavailable);
    }
    let adapter = capture
        .adapter_instance
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or(AudioRouteReason::MetadataUnavailable)?;
    let same_adapter = |endpoint: &&Endpoint| endpoint.adapter_instance.as_deref() == Some(adapter);
    if endpoints
        .iter()
        .filter(same_adapter)
        .filter(|endpoint| endpoint.direction == Direction::Capture)
        .count()
        != 1
    {
        return Err(AudioRouteReason::Ambiguous);
    }
    // An unreadable VB endpoint could be another pin on this adapter. Do not
    // silently classify incomplete inventory as a unique, ready pair.
    if endpoints.iter().any(|endpoint| {
        endpoint.direction == Direction::Render
            && endpoint.cable == Some(cable)
            && (!endpoint.metadata_complete || endpoint.adapter_instance.is_none())
    }) {
        return Err(AudioRouteReason::MetadataUnavailable);
    }
    let candidates: Vec<_> = endpoints
        .iter()
        .filter(same_adapter)
        .filter(|endpoint| {
            endpoint.direction == Direction::Render
                && endpoint.cable == Some(cable)
                && matches!(endpoint.pin, Pin::Render | Pin::Render16)
        })
        .collect();
    if candidates
        .iter()
        .filter(|endpoint| endpoint.pin == Pin::Render)
        .count()
        > 1
        || candidates
            .iter()
            .filter(|endpoint| endpoint.pin == Pin::Render16)
            .count()
            > 1
    {
        return Err(AudioRouteReason::Ambiguous);
    }
    // Pack45 exposes a standard and a 16-channel input for one cable. An exact
    // valid saved pin is retained, while automatic setup chooses the standard
    // input documented for normal microphone use. Never open both inputs.
    if let Some((id, name)) = preferred_render {
        if let Some(endpoint) = candidates
            .iter()
            .find(|endpoint| endpoint.endpoint.id == id && endpoint.endpoint.name == name)
        {
            return Ok(endpoint);
        }
    }
    candidates
        .into_iter()
        .find(|endpoint| endpoint.pin == Pin::Render)
        .ok_or(AudioRouteReason::RenderMissing)
}

fn sibling_candidates(
    endpoints: &[Endpoint],
    failed_id: &str,
) -> Result<Vec<AudioEndpoint>, AudioRouteReason> {
    let failed = endpoints
        .iter()
        .find(|e| e.direction == Direction::Render && e.endpoint.id == failed_id)
        .ok_or(AudioRouteReason::RenderMissing)?;
    if !failed.metadata_complete || failed.adapter_instance.is_none() {
        return Err(AudioRouteReason::MetadataUnavailable);
    }
    let cable = failed.cable.ok_or(AudioRouteReason::Unsupported)?;
    if !matches!(failed.pin, Pin::Render | Pin::Render16) {
        return Err(AudioRouteReason::Unsupported);
    }
    let candidates: Vec<_> = endpoints
        .iter()
        .filter(|e| {
            e.direction == Direction::Render
                && e.adapter_instance == failed.adapter_instance
                && e.cable == Some(cable)
        })
        .collect();
    if candidates.iter().any(|e| !e.metadata_complete) {
        return Err(AudioRouteReason::MetadataUnavailable);
    }
    for pin in [Pin::Render, Pin::Render16] {
        if candidates.iter().filter(|e| e.pin == pin).count() > 1 {
            return Err(AudioRouteReason::Ambiguous);
        }
    }
    Ok(candidates
        .into_iter()
        .filter(|e| e.endpoint.id != failed_id && matches!(e.pin, Pin::Render | Pin::Render16))
        .map(|e| e.endpoint.clone())
        .collect())
}

pub(crate) fn same_adapter_render_candidates(
    failed_id: &str,
) -> Result<Vec<AudioEndpoint>, AudioRouteReason> {
    inventory().and_then(|endpoints| sibling_candidates(&endpoints, failed_id))
}

pub fn resolve_capture_pair(
    capture_id: &str,
    expected_name: &str,
    preferred_render: Option<(&str, &str)>,
) -> Result<AudioEndpoint, AudioRouteReason> {
    let started = std::time::Instant::now();
    let result = inventory().and_then(|endpoints| {
        resolve_pair(&endpoints, capture_id, expected_name, preferred_render)
            .map(|endpoint| endpoint.endpoint.clone())
    });
    crate::gatt_note(format!(
        "audio_route action=pair result={} reason={} saved_writer_retained={} elapsed_ms={}",
        if result.is_ok() { "passed" } else { "failed" },
        result
            .as_ref()
            .err()
            .map_or("same_adapter", |reason| reason.as_str()),
        result.as_ref().is_ok_and(|endpoint| preferred_render
            .is_some_and(|(id, name)| endpoint.id == id && endpoint.name == name)),
        started.elapsed().as_millis()
    ));
    result
}

pub fn snapshot(settings: &CaptureInputSettings, audio: &AudioSnapshot) -> AudioRouteSnapshot {
    match inventory() {
        Ok(endpoints) => snapshot_from(&endpoints, settings, audio),
        Err(reason) => AudioRouteSnapshot {
            phase: AudioRoutePhase::Unavailable,
            reason: Some(reason),
            ..base_snapshot(settings, audio)
        },
    }
}

fn base_snapshot(settings: &CaptureInputSettings, audio: &AudioSnapshot) -> AudioRouteSnapshot {
    AudioRouteSnapshot {
        phase: AudioRoutePhase::Unconfigured,
        reason: None,
        capture_endpoint_id: settings.endpoint_id.clone(),
        capture_endpoint_name: settings.endpoint_name.clone(),
        render_endpoint_id: audio.selected_endpoint_id.clone(),
        render_endpoint_name: audio.selected_endpoint_name.clone(),
    }
}

fn snapshot_from(
    endpoints: &[Endpoint],
    settings: &CaptureInputSettings,
    audio: &AudioSnapshot,
) -> AudioRouteSnapshot {
    let mut result = base_snapshot(settings, audio);
    let preferred = audio
        .selected_endpoint_id
        .as_deref()
        .zip(audio.selected_endpoint_name.as_deref());
    let writer_available = |(id, name): (&str, &str)| {
        endpoints.iter().any(|endpoint| {
            endpoint.direction == Direction::Render
                && endpoint.endpoint.id == id
                && endpoint.endpoint.name == name
        })
    };
    let ready = matches!(
        audio.phase,
        AudioPhase::Ready | AudioPhase::Streaming | AudioPhase::Draining
    );
    let decision = match settings
        .endpoint_id
        .as_deref()
        .zip(settings.endpoint_name.as_deref())
    {
        Some((id, name)) => match resolve_pair(endpoints, id, name, preferred) {
            Ok(paired)
                if preferred.is_some_and(|(id, name)| {
                    paired.endpoint.id == id && paired.endpoint.name == name
                }) =>
            {
                if ready {
                    Ok(AudioRoutePhase::Paired)
                } else {
                    Err(AudioRouteReason::AudioUnready)
                }
            }
            Ok(_) if preferred.is_none() => Err(AudioRouteReason::AudioUnready),
            Ok(_) => Err(AudioRouteReason::RenderMismatch),
            Err(AudioRouteReason::Unsupported) => {
                if preferred.is_some_and(writer_available) && ready {
                    result.reason = Some(AudioRouteReason::Unsupported);
                    Ok(AudioRoutePhase::Manual)
                } else {
                    Err(AudioRouteReason::AudioUnready)
                }
            }
            Err(reason) => Err(reason),
        },
        None if settings.endpoint_id.is_some() || settings.endpoint_name.is_some() => {
            Err(AudioRouteReason::IdentityChanged)
        }
        None => match preferred {
            Some(identity) if writer_available(identity) && ready => Ok(AudioRoutePhase::Manual),
            Some(_) => Err(AudioRouteReason::AudioUnready),
            None if audio.selected_endpoint_id.is_some()
                || audio.selected_endpoint_name.is_some() =>
            {
                Err(AudioRouteReason::AudioUnready)
            }
            None => Ok(AudioRoutePhase::Unconfigured),
        },
    };
    match decision {
        Ok(phase) => result.phase = phase,
        Err(reason) => {
            result.phase = AudioRoutePhase::Unavailable;
            result.reason = Some(reason);
        }
    }
    result
}

fn cable_from_adapter(name: &str) -> Option<Cable> {
    match name.to_ascii_lowercase().as_str() {
        "vb-audio virtual cable" => Some(Cable::Base),
        "vb-audio cable a" => Some(Cable::A),
        "vb-audio cable b" => Some(Cable::B),
        "vb-audio cable c" => Some(Cable::C),
        "vb-audio cable d" => Some(Cable::D),
        _ => None,
    }
}

fn pin_from_description(cable: Cable, description: &str, direction: Direction) -> Pin {
    let prefix = match cable {
        Cable::Base => "cable",
        Cable::A => "cable-a",
        Cable::B => "cable-b",
        Cable::C => "cable-c",
        Cable::D => "cable-d",
    };
    let description = description.to_ascii_lowercase();
    match direction {
        Direction::Capture if description == format!("{prefix} output") => Pin::Capture,
        Direction::Render if description == format!("{prefix} input") => Pin::Render,
        Direction::Render if description == format!("{prefix} in 16ch") => Pin::Render16,
        _ => Pin::Other,
    }
}

#[cfg(not(windows))]
fn inventory() -> Result<Vec<Endpoint>, AudioRouteReason> {
    Err(AudioRouteReason::Unsupported)
}

#[cfg(windows)]
fn inventory() -> Result<Vec<Endpoint>, AudioRouteReason> {
    use windows::{
        core::{GUID, HSTRING},
        Win32::{
            Foundation::{PROPERTYKEY, RPC_E_CHANGED_MODE},
            Media::Audio::{IDeviceTopology, IMMDeviceEnumerator, MMDeviceEnumerator},
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                StructuredStorage::{PropVariantClear, PropVariantToStringAlloc},
                CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
            },
        },
    };
    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
        return Err(AudioRouteReason::EnumerationFailed);
    }
    let _apartment = Apartment(initialized.is_ok());
    let en = wasapi::DeviceEnumerator::new().map_err(|_| AudioRouteReason::EnumerationFailed)?;
    let native: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|_| AudioRouteReason::EnumerationFailed)?;
    // Official PKEY_Device_InstanceId. Read it from the *adapter topology*, not
    // the endpoint (SWD/MMDEVAPI instance) or shared container. Different KS
    // filters for one cable have distinct topology IDs but the same PnP node.
    const INSTANCE: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x78c34fc8_104a_4aca_9ea4_524d52996e57),
        pid: 256,
    };
    let adapter_instance = |id: &str| -> windows::core::Result<String> {
        let device = unsafe { native.GetDevice(&HSTRING::from(id)) }?;
        let topology: IDeviceTopology = unsafe { device.Activate(CLSCTX_ALL, None) }?;
        if unsafe { topology.GetConnectorCount() }? != 1 {
            return Err(windows::core::Error::from_hresult(
                windows::Win32::Foundation::E_UNEXPECTED,
            ));
        }
        let connector = unsafe { topology.GetConnector(0) }?;
        let raw = unsafe { connector.GetDeviceIdConnectedTo() }?;
        let value = unsafe { raw.to_string() };
        unsafe { CoTaskMemFree(Some(raw.0.cast())) };
        let adapter = unsafe { native.GetDevice(&HSTRING::from(value?)) }?;
        let store = unsafe { adapter.OpenPropertyStore(STGM_READ) }?;
        let mut value = unsafe { store.GetValue(&INSTANCE) }?;
        let instance = unsafe { PropVariantToStringAlloc(&value) }.and_then(|raw| {
            let instance = unsafe { raw.to_string() };
            unsafe { CoTaskMemFree(Some(raw.0.cast())) };
            Ok(instance?)
        });
        unsafe { PropVariantClear(&mut value) }?;
        // Windows device instance IDs are case-insensitive identifiers.
        instance.map(|instance| instance.to_ascii_lowercase())
    };
    let mut endpoints = Vec::new();
    for (direction, flow) in [
        (Direction::Capture, wasapi::Direction::Capture),
        (Direction::Render, wasapi::Direction::Render),
    ] {
        let devices = en
            .get_device_collection(&flow)
            .map_err(|_| AudioRouteReason::EnumerationFailed)?;
        for device in &devices {
            let device = device.map_err(|_| AudioRouteReason::EnumerationFailed)?;
            let id = device
                .get_id()
                .map_err(|_| AudioRouteReason::EnumerationFailed)?;
            let name = device
                .get_friendlyname()
                .map_err(|_| AudioRouteReason::EnumerationFailed)?;
            let adapter = device.get_interface_friendlyname();
            let description = device.get_description();
            let metadata_complete = adapter.is_ok() && description.is_ok();
            let cable = adapter.ok().as_deref().and_then(cable_from_adapter);
            let pin = cable
                .zip(description.ok())
                .map_or(Pin::Other, |(cable, description)| {
                    pin_from_description(cable, &description, direction)
                });
            let adapter_instance = cable
                .and_then(|_| adapter_instance(&id).ok())
                .filter(|instance| !instance.is_empty());
            endpoints.push(Endpoint {
                endpoint: AudioEndpoint {
                    id,
                    name,
                    is_virtual_cable_candidate: cable.is_some() && pin != Pin::Other,
                },
                direction,
                cable,
                pin,
                adapter_instance,
                metadata_complete,
            });
        }
    }
    Ok(endpoints)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: &str, name: &str, adapter: &str, direction: Direction, pin: Pin) -> Endpoint {
        Endpoint {
            endpoint: AudioEndpoint {
                id: id.into(),
                name: name.into(),
                is_virtual_cable_candidate: true,
            },
            direction,
            cable: Some(Cable::Base),
            pin,
            adapter_instance: Some(adapter.into()),
            metadata_complete: true,
        }
    }

    fn standard() -> Vec<Endpoint> {
        vec![
            endpoint(
                "mic",
                "CABLE Output",
                "line-one",
                Direction::Capture,
                Pin::Capture,
            ),
            endpoint(
                "writer",
                "CABLE Input",
                "line-one",
                Direction::Render,
                Pin::Render,
            ),
            endpoint(
                "writer16",
                "CABLE In 16ch",
                "line-one",
                Direction::Render,
                Pin::Render16,
            ),
        ]
    }

    #[test]
    fn sibling_fallback_requires_exact_adapter_and_unique_pins() {
        let mut endpoints = standard();
        endpoints.push(endpoint(
            "other",
            "CABLE Input",
            "line-two",
            Direction::Render,
            Pin::Render,
        ));
        assert_eq!(
            sibling_candidates(&endpoints, "writer16").unwrap()[0].id,
            "writer"
        );
        endpoints[1].endpoint.name = "renamed standard writer".into();
        assert_eq!(
            sibling_candidates(&endpoints, "writer16").unwrap()[0].id,
            "writer"
        );
        endpoints.push(endpoint(
            "duplicate",
            "CABLE Input",
            "line-one",
            Direction::Render,
            Pin::Render,
        ));
        assert_eq!(
            sibling_candidates(&endpoints, "writer16"),
            Err(AudioRouteReason::Ambiguous)
        );
    }

    #[test]
    fn sibling_fallback_rejects_missing_or_incomplete_metadata() {
        let mut endpoints = standard();
        assert_eq!(
            sibling_candidates(&endpoints, "missing"),
            Err(AudioRouteReason::RenderMissing)
        );
        endpoints[2].adapter_instance = None;
        assert_eq!(
            sibling_candidates(&endpoints, "writer16"),
            Err(AudioRouteReason::MetadataUnavailable)
        );
        endpoints[2].adapter_instance = Some("line-one".into());
        endpoints[1].metadata_complete = false;
        assert_eq!(
            sibling_candidates(&endpoints, "writer16"),
            Err(AudioRouteReason::MetadataUnavailable)
        );
    }

    #[test]
    fn pairs_the_standard_pin_of_the_exact_cable_with_both_vb_render_pins() {
        let mut endpoints = standard();
        endpoints.push(endpoint(
            "other",
            "CABLE Input",
            "line-two",
            Direction::Render,
            Pin::Render,
        ));
        assert_eq!(
            resolve_pair(&endpoints, "mic", "CABLE Output", None)
                .unwrap()
                .endpoint
                .id,
            "writer"
        );
    }

    #[test]
    fn retains_a_valid_saved_sixteen_channel_writer_but_never_a_different_line() {
        let mut endpoints = standard();
        endpoints.push(endpoint(
            "other",
            "CABLE Input",
            "line-two",
            Direction::Render,
            Pin::Render,
        ));
        assert_eq!(
            resolve_pair(
                &endpoints,
                "mic",
                "CABLE Output",
                Some(("writer16", "CABLE In 16ch"))
            )
            .unwrap()
            .endpoint
            .id,
            "writer16"
        );
        assert_eq!(
            resolve_pair(
                &endpoints,
                "mic",
                "CABLE Output",
                Some(("other", "CABLE Input"))
            )
            .unwrap()
            .endpoint
            .id,
            "writer"
        );
    }

    #[test]
    fn endpoint_display_renaming_does_not_define_the_cable_or_pin() {
        let mut endpoints = standard();
        endpoints[0].endpoint.name = "My microphone".into();
        endpoints[1].endpoint.name = "Private transfer".into();
        assert_eq!(
            resolve_pair(&endpoints, "mic", "My microphone", None)
                .unwrap()
                .endpoint
                .id,
            "writer"
        );
        assert_eq!(
            resolve_pair(&endpoints, "mic", "CABLE Output", None).unwrap_err(),
            AudioRouteReason::IdentityChanged
        );
    }

    #[test]
    fn same_friendly_name_on_a_physical_adapter_never_pairs() {
        let mut endpoints = standard();
        endpoints[0].cable = None;
        assert_eq!(
            resolve_pair(&endpoints, "mic", "CABLE Output", None).unwrap_err(),
            AudioRouteReason::Unsupported
        );
    }

    #[test]
    fn rejects_missing_capture_render_and_adapter_metadata_without_another_line_fallback() {
        let mut endpoints = standard();
        assert_eq!(
            resolve_pair(&endpoints, "missing", "CABLE Output", None).unwrap_err(),
            AudioRouteReason::CaptureMissing
        );
        endpoints[1].adapter_instance = Some("different-line".into());
        endpoints[2].adapter_instance = Some("different-line".into());
        assert_eq!(
            resolve_pair(&endpoints, "mic", "CABLE Output", None).unwrap_err(),
            AudioRouteReason::RenderMissing
        );
        endpoints[0].adapter_instance = None;
        assert_eq!(
            resolve_pair(&endpoints, "mic", "CABLE Output", None).unwrap_err(),
            AudioRouteReason::MetadataUnavailable
        );
    }

    #[test]
    fn duplicate_standard_writers_or_captures_are_ambiguous_even_with_a_saved_choice() {
        let mut endpoints = standard();
        let mut duplicate = endpoints[1].clone();
        duplicate.endpoint.id = "duplicate-writer".into();
        endpoints.push(duplicate);
        assert_eq!(
            resolve_pair(
                &endpoints,
                "mic",
                "CABLE Output",
                Some(("writer", "CABLE Input"))
            )
            .unwrap_err(),
            AudioRouteReason::Ambiguous
        );
        let mut endpoints = standard();
        let mut duplicate = endpoints[0].clone();
        duplicate.endpoint.id = "duplicate-mic".into();
        endpoints.push(duplicate);
        assert_eq!(
            resolve_pair(&endpoints, "mic", "CABLE Output", None).unwrap_err(),
            AudioRouteReason::Ambiguous
        );
    }

    #[test]
    fn no_implicit_sixteen_channel_substitution_when_the_standard_pin_is_absent() {
        let mut endpoints = standard();
        endpoints.remove(1);
        assert_eq!(
            resolve_pair(&endpoints, "mic", "CABLE Output", None).unwrap_err(),
            AudioRouteReason::RenderMissing
        );
        assert_eq!(
            resolve_pair(
                &endpoints,
                "mic",
                "CABLE Output",
                Some(("writer16", "CABLE In 16ch"))
            )
            .unwrap()
            .endpoint
            .id,
            "writer16"
        );
    }

    #[test]
    fn route_readiness_describes_the_actual_writer_even_when_default_switching_is_disabled() {
        let endpoints = standard();
        let settings = CaptureInputSettings {
            enabled: false,
            endpoint_id: Some("mic".into()),
            endpoint_name: Some("CABLE Output".into()),
        };
        let mut audio = AudioSnapshot {
            phase: AudioPhase::Ready,
            selected_endpoint_id: Some("writer".into()),
            selected_endpoint_name: Some("CABLE Input".into()),
            ..Default::default()
        };
        assert_eq!(
            snapshot_from(&endpoints, &settings, &audio).phase,
            AudioRoutePhase::Paired
        );
        audio.selected_endpoint_id = Some("different-line".into());
        let mismatch = snapshot_from(&endpoints, &settings, &audio);
        assert_eq!(mismatch.phase, AudioRoutePhase::Unavailable);
        assert_eq!(mismatch.reason, Some(AudioRouteReason::RenderMismatch));
        audio.selected_endpoint_id = Some("writer".into());
        audio.phase = AudioPhase::Failed;
        assert_eq!(
            snapshot_from(&endpoints, &settings, &audio).reason,
            Some(AudioRouteReason::AudioUnready)
        );
        assert_eq!(
            snapshot_from(&endpoints, &settings, &AudioSnapshot::default()).reason,
            Some(AudioRouteReason::AudioUnready)
        );
    }

    #[test]
    fn an_existing_writer_without_a_capture_choice_is_retained_as_manual() {
        let endpoints = standard();
        let settings = CaptureInputSettings::default();
        let audio = AudioSnapshot {
            phase: AudioPhase::Ready,
            selected_endpoint_id: Some("writer".into()),
            selected_endpoint_name: Some("CABLE Input".into()),
            ..Default::default()
        };
        let route = snapshot_from(&endpoints, &settings, &audio);
        assert_eq!(route.phase, AudioRoutePhase::Manual);
        assert_eq!(route.render_endpoint_id.as_deref(), Some("writer"));
        assert_eq!(
            snapshot_from(&endpoints, &settings, &AudioSnapshot::default()).phase,
            AudioRoutePhase::Unconfigured
        );
    }

    #[test]
    fn absent_adapter_properties_are_not_reported_as_an_unsupported_manual_microphone() {
        let mut endpoints = standard();
        endpoints[0].metadata_complete = false;
        endpoints[0].cable = None;
        assert_eq!(
            resolve_pair(&endpoints, "mic", "CABLE Output", None).unwrap_err(),
            AudioRouteReason::MetadataUnavailable
        );
    }

    #[test]
    fn driver_adapter_and_pin_descriptions_must_agree_on_the_cable_family() {
        assert_eq!(
            cable_from_adapter("Speakers (VB-Audio Virtual Cable)"),
            None
        );
        assert_eq!(
            cable_from_adapter("VB-Audio Virtual Cable"),
            Some(Cable::Base)
        );
        for (name, cable, prefix) in [
            ("VB-Audio Cable A", Cable::A, "CABLE-A"),
            ("VB-Audio Cable B", Cable::B, "CABLE-B"),
            ("VB-Audio Cable C", Cable::C, "CABLE-C"),
            ("VB-Audio Cable D", Cable::D, "CABLE-D"),
        ] {
            assert_eq!(cable_from_adapter(name), Some(cable));
            assert_eq!(
                pin_from_description(cable, &format!("{prefix} Output"), Direction::Capture),
                Pin::Capture
            );
            assert_eq!(
                pin_from_description(cable, &format!("{prefix} Input"), Direction::Render),
                Pin::Render
            );
            assert_eq!(
                pin_from_description(cable, "CABLE Input", Direction::Render),
                Pin::Other
            );
            assert_eq!(
                pin_from_description(cable, &format!("{prefix} Output"), Direction::Render),
                Pin::Other
            );
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "read-only Windows audio topology observation; requires exactly one active standard VB-CABLE"]
    fn actual_vb_cable_topology_resolves_the_standard_writer_without_changing_audio() {
        let endpoints = inventory().expect("read active public endpoint properties");
        let captures: Vec<_> = endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.direction == Direction::Capture && endpoint.cable == Some(Cable::Base)
            })
            .collect();
        assert_eq!(
            captures.len(),
            1,
            "explicit base-cable observation requires one capture"
        );
        let capture = captures[0];
        let writer = resolve_capture_pair(&capture.endpoint.id, &capture.endpoint.name, None)
            .expect("fresh production resolver");
        let actual = endpoints
            .iter()
            .find(|endpoint| endpoint.endpoint.id == writer.id)
            .unwrap();
        assert_eq!(actual.pin, Pin::Render);
        assert!(actual.adapter_instance.is_some());
        assert_eq!(actual.adapter_instance, capture.adapter_instance);
        let same_line_render_count = endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.direction == Direction::Render
                    && endpoint.adapter_instance == capture.adapter_instance
            })
            .count();
        println!("audio_route_observation result=passed same_pnp_adapter=true selected_pin=standard same_line_render_count={same_line_render_count} system_default_writes=0 audio_streams_opened=0");
    }
}
