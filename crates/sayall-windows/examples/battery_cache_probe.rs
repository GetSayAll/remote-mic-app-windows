// Read-only: no hooks, SendInput, BLE connections, or settings changes.
#[cfg(windows)]
fn main() {
    let address = std::env::var("SAYALL_BATTERY_PROBE_ADDRESS")
        .ok()
        .and_then(|value| u64::from_str_radix(&value, 16).ok())
        .filter(|value| *value <= 0xffff_ffff_ffff)
        .expect("set the probe-only peer address environment variable");
    let start = std::time::Instant::now();
    let reading = sayall_windows::battery::read_cached_battery(address);
    println!(
        "{}",
        serde_json::json!({
            "kind": "read_only_windows_battery_cache",
            "batteryLevel": reading.level,
            "source": reading.source,
            "reason": reading.reason,
            "elapsedMs": start.elapsed().as_millis(),
            "voiceOrDriverActions": false,
        })
    );
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This read-only probe requires Windows.");
}
