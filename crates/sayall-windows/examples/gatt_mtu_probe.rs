//! 只读探针：读取当前已连接遥控器的 ATT MTU（`GattSession.MaxPduSize`）。
//!
//! 用途（2026-10-24 现场）：语音条出现但无文字时，先区分「远端没送够音频」与
//! 「窗口/输入法没收到文字」。音频会话日志已给出每秒样本数；本探针补上链路层
//! 变量：MTU 43 时单次通知最多约 40 字节，正好是 16 kHz ADPCM 满速的 1/3。
//!
//! 用法（需要 Visual Studio Developer PowerShell）：
//!   cargo run --example gatt_mtu_probe
//!   cargo run --example gatt_mtu_probe -- <12 位十六进制地址>
//!
//! 输出不含蓝牙地址、设备名或语音内容。默认读取 `%LOCALAPPDATA%\SayAll\rc003-target.ini`。

use windows::Devices::Bluetooth::BluetoothLEDevice;
use windows::Devices::Bluetooth::GenericAttributeProfile::GattSession;
use windows::Win32::System::WinRT::RoInitialize;
use windows::Win32::System::WinRT::RO_INIT_MULTITHREADED;

fn main() {
    let address = match std::env::args().nth(1) {
        Some(raw) => parse_address(&raw),
        None => read_target_address(),
    };
    let address = match address {
        Ok(address) => address,
        Err(error) => {
            println!("target_address_unavailable reason={error}");
            std::process::exit(2);
        }
    };

    if let Err(error) = unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        println!(
            "ro_initialize_failed hresult=0x{:08X}",
            error.code().0 as u32
        );
        std::process::exit(1);
    }

    let device = match BluetoothLEDevice::FromBluetoothAddressAsync(address)
        .and_then(|operation| operation.join())
    {
        Ok(device) => device,
        Err(error) => {
            println!("device_open_failed hresult=0x{:08X}", error.code().0 as u32);
            std::process::exit(1);
        }
    };
    let connection = device
        .ConnectionStatus()
        .map(|status| format!("{status:?}"))
        .unwrap_or_else(|error| format!("error:0x{:08X}", error.code().0 as u32));
    let device_id = match device.BluetoothDeviceId() {
        Ok(id) => id,
        Err(error) => {
            println!(
                "device_id_unavailable hresult=0x{:08X} connection={connection}",
                error.code().0 as u32
            );
            std::process::exit(1);
        }
    };
    let session =
        match GattSession::FromDeviceIdAsync(&device_id).and_then(|operation| operation.join()) {
            Ok(session) => session,
            Err(error) => {
                println!(
                    "gatt_session_failed hresult=0x{:08X} connection={connection}",
                    error.code().0 as u32
                );
                std::process::exit(1);
            }
        };
    let mtu = session.MaxPduSize();
    let can_maintain = session.CanMaintainConnection();
    let maintain_before = session.MaintainConnection();
    let hold_seconds: u64 = std::env::var("SAYALL_GATT_MTU_HOLD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    if hold_seconds > 0 {
        let _ = session.SetMaintainConnection(true);
    }
    let maintain_after = session.MaintainConnection();
    let status = session.SessionStatus();
    println!(
        "gatt_session mtu={} maintain_before={:?} can_maintain={:?} maintain_after={:?} status={:?} connection={connection} hold_seconds={hold_seconds}",
        mtu.map_or_else(|e| format!("error:0x{:08X}", e.code().0 as u32), |v| v.to_string()),
        maintain_before.ok(),
        can_maintain.ok(),
        maintain_after.ok(),
        status.ok(),
    );
    if hold_seconds > 0 {
        let started = std::time::Instant::now();
        while started.elapsed().as_secs() < hold_seconds {
            std::thread::sleep(std::time::Duration::from_secs(2));
            println!(
                "gatt_hold elapsed_s={} mtu={:?} maintain={:?} status={:?}",
                started.elapsed().as_secs(),
                session.MaxPduSize().ok(),
                session.MaintainConnection().ok(),
                session.SessionStatus().ok(),
            );
        }
        let _ = session.SetMaintainConnection(false);
    }
    let _ = session.Close();
    let _ = device.Close();
}

fn read_target_address() -> Result<u64, String> {
    let path = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA unavailable".to_owned())?
        .join("SayAll")
        .join("rc003-target.ini");
    let text =
        std::fs::read_to_string(&path).map_err(|error| format!("read failed: {}", error.kind()))?;
    let raw = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("address="))
        .ok_or_else(|| "address key missing".to_owned())?;
    parse_address(raw)
}

fn parse_address(raw: &str) -> Result<u64, String> {
    let cleaned: String = raw.chars().filter(|ch| ch.is_ascii_hexdigit()).collect();
    if cleaned.len() != 12 {
        return Err(format!("expected 12 hex digits, got {}", cleaned.len()));
    }
    u64::from_str_radix(&cleaned, 16).map_err(|error| error.to_string())
}
