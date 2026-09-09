//! Observe WeType microphone use through the public Windows ConsentStore.
//! Updates leave historical executable entries behind; enumeration order is
//! unrelated to the version that is currently recording.

use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    REG_QWORD, REG_VALUE_TYPE,
};

const CONSENT_NONPACKAGED: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone\\NonPackaged";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MicObservation {
    latest_start: u64,
    active: bool,
    entries: usize,
}

impl MicObservation {
    fn record(&mut self, start: u64, stop: u64) {
        self.latest_start = self.latest_start.max(start);
        self.active |= start > 0 && stop == 0;
        self.entries += 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MicResponse {
    Observed,
    NotObserved,
    Unknown,
}

pub(crate) fn response_since(
    baseline: Option<MicObservation>,
    current: Option<MicObservation>,
) -> MicResponse {
    match (baseline, current) {
        (_, Some(current)) if current.active => MicResponse::Observed,
        (Some(base), Some(current)) if current.latest_start > base.latest_start => {
            MicResponse::Observed
        }
        (Some(base), Some(current))
            if current.entries >= base.entries && current.latest_start == base.latest_start =>
        {
            MicResponse::NotObserved
        }
        _ => MicResponse::Unknown,
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn read_qword(key: HKEY, name: &str) -> Option<u64> {
    let value = wide(name);
    let mut data = 0_u64;
    let mut size = std::mem::size_of::<u64>() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    let result = unsafe {
        RegQueryValueExW(
            key,
            windows::core::PCWSTR(value.as_ptr()),
            None,
            Some(&mut kind),
            Some((&mut data as *mut u64).cast()),
            Some(&mut size),
        )
    };
    (result.0 == 0 && kind == REG_QWORD && size == 8).then_some(data)
}

/// Missing or partially unreadable observations must not trigger recovery.
pub(crate) fn wetype_mic_observation() -> Option<MicObservation> {
    let subkey = wide(CONSENT_NONPACKAGED);
    let mut root = HKEY::default();
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(subkey.as_ptr()),
            None,
            KEY_READ,
            &mut root,
        )
    }
    .0 != 0
    {
        return None;
    }
    let mut index = 0;
    let mut observation = MicObservation::default();
    let mut complete = true;
    loop {
        let mut name = [0u16; 260];
        let mut len = name.len() as u32;
        let result = unsafe {
            RegEnumKeyExW(
                root,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if result == ERROR_NO_MORE_ITEMS {
            break;
        }
        if result.0 != 0 {
            complete = false;
            break;
        }
        index += 1;
        let entry = String::from_utf16_lossy(&name[..len as usize]);
        if !entry.to_ascii_lowercase().contains("wetype") {
            continue;
        }
        let mut key = HKEY::default();
        let entry_wide = wide(&entry);
        if unsafe {
            RegOpenKeyExW(
                root,
                windows::core::PCWSTR(entry_wide.as_ptr()),
                None,
                KEY_READ,
                &mut key,
            )
        }
        .0 != 0
        {
            complete = false;
            continue;
        }
        let start = read_qword(key, "LastUsedTimeStart");
        let stop = read_qword(key, "LastUsedTimeStop");
        unsafe {
            let _ = RegCloseKey(key);
        }
        match (start, stop) {
            (Some(start), Some(stop)) => observation.record(start, stop),
            _ => complete = false,
        }
    }
    unsafe {
        let _ = RegCloseKey(root);
    }
    (complete && observation.entries > 0).then_some(observation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires installed WeType microphone history; read-only"]
    fn live_consentstore_observation_is_readable() {
        let observation = wetype_mic_observation().expect("WeType observation unavailable");
        assert!(observation.entries > 0);
        println!(
            "wetype_observation entries={} active={}",
            observation.entries, observation.active
        );
    }

    fn stopped(start: u64) -> MicObservation {
        let mut result = MicObservation::default();
        result.record(start, start + 1);
        result
    }

    #[test]
    fn historical_versions_do_not_hide_current_recording() {
        let mut baseline = MicObservation::default();
        for start in [10, 20, 30, 40, 50, 60, 70] {
            baseline.record(start, start + 1);
        }
        let mut current = MicObservation::default();
        for start in [10, 20, 30, 40, 50, 60] {
            current.record(start, start + 1);
        }
        current.record(80, 0);
        assert_eq!(
            response_since(Some(baseline), Some(current)),
            MicResponse::Observed
        );
        let mut reversed = MicObservation::default();
        reversed.record(80, 0);
        for start in [60, 50, 40, 30, 20, 10] {
            reversed.record(start, start + 1);
        }
        assert_eq!(current, reversed);
    }

    #[test]
    fn active_recording_is_not_retried_even_if_baseline_already_contains_start() {
        let mut active = stopped(10);
        active.active = true;
        assert_eq!(
            response_since(Some(active), Some(active)),
            MicResponse::Observed
        );
        assert_eq!(response_since(None, Some(active)), MicResponse::Observed);
    }

    #[test]
    fn new_completed_recording_is_a_response() {
        assert_eq!(
            response_since(Some(stopped(10)), Some(stopped(20))),
            MicResponse::Observed
        );
    }

    #[test]
    fn unchanged_inactive_recording_allows_recovery() {
        assert_eq!(
            response_since(Some(stopped(10)), Some(stopped(10))),
            MicResponse::NotObserved
        );
    }

    #[test]
    fn missing_or_regressed_observations_do_not_authorize_recovery() {
        assert_eq!(response_since(None, None), MicResponse::Unknown);
        assert_eq!(
            response_since(Some(stopped(10)), None),
            MicResponse::Unknown
        );
        assert_eq!(
            response_since(None, Some(stopped(10))),
            MicResponse::Unknown
        );
        assert_eq!(
            response_since(Some(stopped(20)), Some(stopped(10))),
            MicResponse::Unknown
        );
    }
}
