//! Read-only Raw Input inventory. No registration, hook, key injection or driver
//! operation. Checks device identity rules only, not real keys.

#[cfg(windows)]
fn main() -> Result<(), String> {
    use sayall_windows::raw_input::device_path_matches_xiaomi_remote;
    use sayall_windows::rc003_filter::device_path_matches_filter_target;
    use std::mem::size_of;
    use windows::Win32::UI::Input::{
        GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RIDI_DEVICENAME,
        RIM_TYPEHID, RIM_TYPEKEYBOARD,
    };

    let mut count = 0;
    let item_size = size_of::<RAWINPUTDEVICELIST>() as u32;
    if unsafe { GetRawInputDeviceList(None, &mut count, item_size) } == u32::MAX {
        return Err("Raw Input inventory size query failed".into());
    }
    let mut devices = vec![RAWINPUTDEVICELIST::default(); count as usize];
    let written = if count == 0 {
        0
    } else {
        unsafe { GetRawInputDeviceList(Some(devices.as_mut_ptr()), &mut count, item_size) }
    };
    if written == u32::MAX {
        return Err("Raw Input inventory failed".into());
    }
    let mut matched = 0;
    let mut eligible = 0;
    let mut shapes = Vec::new();
    for device in devices.into_iter().take(written as usize) {
        if device.dwType != RIM_TYPEKEYBOARD && device.dwType != RIM_TYPEHID {
            continue;
        }
        let mut size = 0;
        if unsafe { GetRawInputDeviceInfoW(Some(device.hDevice), RIDI_DEVICENAME, None, &mut size) }
            == u32::MAX
            || size == 0
        {
            continue;
        }
        let mut name = vec![0u16; size as usize];
        if unsafe {
            GetRawInputDeviceInfoW(
                Some(device.hDevice),
                RIDI_DEVICENAME,
                Some(name.as_mut_ptr().cast()),
                &mut size,
            )
        } == u32::MAX
        {
            continue;
        }
        let end = name
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(name.len());
        let path = String::from_utf16_lossy(&name[..end]);
        if device_path_matches_xiaomi_remote(&path) {
            matched += 1;
            let normalized = path.to_ascii_lowercase();
            let hardware = normalized.split('#').nth(1).unwrap_or("");
            let suffix = hardware.strip_prefix(
                "{00001812-0000-1000-8000-00805f9b34fb}_dev_vid&012717_pid&32b8_rev&00a4",
            );
            shapes.push(serde_json::json!({
                "keyboard": device.dwType == RIM_TYPEKEYBOARD,
                "hidPrefix": normalized.starts_with(r"\\?\hid#"),
                "contractComponentPrefix": suffix.is_some(),
                "suffixLength": suffix.map(str::len),
                "ampersandCollection": suffix.is_some_and(|tail| tail.starts_with("&col")),
                "underscoreCollection": suffix.is_some_and(|tail| tail.starts_with("_col")),
                "underscoreSuffix": suffix.is_some_and(|tail| tail.starts_with('_')),
                "bleVendor": normalized.contains("dev_vid&012717"),
                "classicVendor": normalized.contains("vid_2717"),
                "revision": normalized.contains("rev&00a4") || normalized.contains("rev_00a4"),
            }));
            if device.dwType == RIM_TYPEKEYBOARD && device_path_matches_filter_target(&path) {
                eligible += 1;
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({
            "kind": "read_only_identity",
            "matchedRawDevices": matched,
            "identityMatchesFilterContract": eligible,
            "singleEligibleDevice": matched == 1 && eligible == 1,
        "driverEventsVerified": false,
        "pathShapeChecks": shapes,
        })
    );
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("deferred: this identity check requires Windows");
}
