//! 蓝牙无线电开关探针（BLE 僵死链路自动恢复验证用）。
//!
//! 用法：cargo run --release -p sayall-windows --example radio_probe -- [off-on]
//!
//! 通过 WinRT Windows.Devices.Radios.Radio API 枚举无线电并切换蓝牙开关，
//! 验证：① 未打包桌面进程能否调用 SetStateAsync（权限）；② 开关周期能否
//! 清除 BLE 僵死链路（配合主应用重连观察）。只做公开 API 操作。

use std::future::IntoFuture;
use std::time::Duration;

use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

fn find_bluetooth_radio() -> windows::core::Result<Radio> {
    let operation = Radio::GetRadiosAsync()?;
    let radios = futures::executor::block_on(operation.into_future())?;
    let count = radios.Size()?;
    for index in 0..count {
        let radio = radios.GetAt(index)?;
        if radio.Kind()? == RadioKind::Bluetooth {
            return Ok(radio);
        }
    }
    Err(windows::core::Error::from(windows::core::HRESULT(-1)))
}

fn set_state(radio: &Radio, state: RadioState, label: &str) -> windows::core::Result<RadioState> {
    let operation = radio.SetStateAsync(state)?;
    let status = futures::executor::block_on(operation.into_future())?;
    println!("{label}: 结果={state:?} 请求状态={status:?}");
    radio.State()
}

fn main() {
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED).expect("RoInitialize");
    }
    let radio = find_bluetooth_radio().expect("未找到蓝牙无线电");
    let name = radio.Name().unwrap().to_string();
    let initial = radio.State().unwrap();
    println!("蓝牙无线电: {name}  当前状态={initial:?}");

    println!("--- 测试 SetStateAsync 权限（关闭 2 秒后恢复） ---");
    match set_state(&radio, RadioState::Off, "关闭") {
        Ok(state) => {
            println!("  关闭后实际状态={state:?}");
            std::thread::sleep(Duration::from_secs(2));
            let on_state = set_state(&radio, RadioState::On, "恢复打开");
            println!("  恢复后实际状态={on_state:?}");
        }
        Err(error) => {
            println!("  SetStateAsync(Off) 失败: {error:?}");
            // 失败也要确保无线电没有被意外留在关闭状态
            let _ = set_state(&radio, RadioState::On, "兜底恢复打开");
        }
    }
    std::thread::sleep(Duration::from_secs(1));
    let final_state = radio.State().unwrap();
    println!("最终状态={final_state:?}");
}
