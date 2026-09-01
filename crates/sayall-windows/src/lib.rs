use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformSnapshot {
    pub platform: &'static str,
    pub windows_api_available: bool,
    pub ble_scan_available: bool,
    pub ble_voice_ready: bool,
    pub wasapi_ready: bool,
    pub raw_input_ready: bool,
    pub send_input_ready: bool,
    pub verification_status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedRemote {
    pub id: String,
    pub name: String,
    pub is_supported_candidate: bool,
}

#[derive(Debug, Clone, Default)]
pub struct WindowsPlatform;

impl WindowsPlatform {
    pub fn snapshot(&self) -> PlatformSnapshot {
        #[cfg(windows)]
        {
            PlatformSnapshot {
                platform: "windows",
                windows_api_available: true,
                ble_scan_available: true,
                ble_voice_ready: false,
                wasapi_ready: false,
                raw_input_ready: false,
                send_input_ready: false,
                verification_status: "等待 Windows 主机与 RC003 真机验证",
            }
        }

        #[cfg(not(windows))]
        {
            PlatformSnapshot {
                platform: std::env::consts::OS,
                windows_api_available: false,
                ble_scan_available: false,
                ble_voice_ready: false,
                wasapi_ready: false,
                raw_input_ready: false,
                send_input_ready: false,
                verification_status: "当前主机不是 Windows，仅可验证界面与纯 Rust 核心",
            }
        }
    }

    pub fn scan_paired_remotes(&self) -> Result<Vec<PairedRemote>, PlatformError> {
        scan_paired_remotes()
    }
}

pub fn is_supported_remote_name(raw_name: &str) -> bool {
    matches!(
        raw_name.trim().to_lowercase().as_str(),
        "mi rc"
            | "xiaomi bluetooth remote 2"
            | "xiaomi bluetooth remote 2 pro"
            | "小米蓝牙语音遥控器"
            | "小米蓝牙遥控器2"
            | "小米蓝牙遥控器2 pro"
            | "arn9"
    )
}

#[cfg(windows)]
fn scan_paired_remotes() -> Result<Vec<PairedRemote>, PlatformError> {
    use std::future::IntoFuture;
    use windows::Devices::Bluetooth::BluetoothLEDevice;
    use windows::Devices::Enumeration::DeviceInformation;
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

    struct WinRtApartment;

    impl Drop for WinRtApartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }

    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(windows_error)?;
    let _apartment = WinRtApartment;
    let selector = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)
        .map_err(|error| PlatformError::WindowsApi(error.to_string()))?;
    let operation = DeviceInformation::FindAllAsyncAqsFilter(&selector).map_err(windows_error)?;
    let devices = futures::executor::block_on(operation.into_future()).map_err(windows_error)?;

    let mut remotes = Vec::new();
    for index in 0..devices.Size().map_err(windows_error)? {
        let device = devices.GetAt(index).map_err(windows_error)?;
        let name = device.Name().map_err(windows_error)?.to_string();
        if !is_supported_remote_name(&name) {
            continue;
        }
        remotes.push(PairedRemote {
            id: device.Id().map_err(windows_error)?.to_string(),
            name,
            is_supported_candidate: true,
        });
    }
    Ok(remotes)
}

#[cfg(windows)]
fn windows_error(error: windows::core::Error) -> PlatformError {
    PlatformError::WindowsApi(error.to_string())
}

#[cfg(not(windows))]
fn scan_paired_remotes() -> Result<Vec<PairedRemote>, PlatformError> {
    Err(PlatformError::UnsupportedPlatform)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlatformError {
    #[error("Windows platform APIs are unavailable on this host")]
    UnsupportedPlatform,
    #[error("Windows API failed: {0}")]
    WindowsApi(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_approved_remote_names() {
        for name in [
            "MI RC",
            "  mi rc  ",
            "Xiaomi Bluetooth Remote 2",
            "Xiaomi Bluetooth Remote 2 Pro",
            "小米蓝牙语音遥控器",
            "小米蓝牙遥控器2",
            "ARN9",
        ] {
            assert!(is_supported_remote_name(name), "expected match: {name}");
        }

        for name in ["", "Mi Mouse", "MI RC2", "小米", "Unknown Remote"] {
            assert!(!is_supported_remote_name(name), "unexpected match: {name}");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_host_reports_unsupported_instead_of_fake_devices() {
        let platform = WindowsPlatform;
        assert_eq!(
            platform.scan_paired_remotes(),
            Err(PlatformError::UnsupportedPlatform)
        );
        assert!(!platform.snapshot().windows_api_available);
    }
}
