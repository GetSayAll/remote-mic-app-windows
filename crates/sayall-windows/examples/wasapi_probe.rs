// 本机 WASAPI 输出端点探针：定位「初始化 16 kHz WASAPI 输出失败：0x8889000A」。
//
// 背景（2026-10-02 现场）：Andy 机器上 VB-CABLE 驱动版本较新，多出一个 16 Ch
// 端点；应用里选「CABLE In 16 Ch (2- VB-Audio Virtual Cable)」时初始化失败，
// HRESULT 0x8889000A = AUDCLNT_E_DEVICE_IN_USE；而同一台虚拟设备的另一个端点
// 「扬声器 (2- VB-Audio Virtual Cable)」能正常打开并推流。
//
// 本探针用与产品完全相同的参数（16 kHz 单声道 s16、共享模式、autoconvert、
// 设备默认周期）测三件事：
//   1. 基线：无任何流时逐个端点打开，立即释放——判定端点本身是否可用；
//   2. 持有对照：先打开并持有端点 A，再打开 B——判定 0x8889000A 是否来自
//      「同一虚拟设备的第二条流」；
//   3. 同端点重开：不持有任何流时对同一端点连续开两个客户端——判定共享模式
//      多客户端是否成立。
//
// 用法：
//   cargo run -p sayall-windows --example wasapi_probe
//   cargo run -p sayall-windows --example wasapi_probe -- --hold <序号>
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

const SOURCE_SAMPLE_RATE: usize = 16_000;
const SOURCE_CHANNELS: usize = 1;

fn describe(error: &wasapi::WasapiError) -> String {
    match error {
        wasapi::WasapiError::Windows(inner) => {
            format!("{error} hresult=0x{:08X}", inner.code().0 as u32)
        }
        other => format!("{other}"),
    }
}

fn open_client(device: &wasapi::Device) -> Result<(AudioClient, i64), wasapi::WasapiError> {
    let mut client = device.get_iaudioclient()?;
    let (period, _) = client.get_device_period()?;
    let format = WaveFormat::new(
        16,
        16,
        &SampleType::Int,
        SOURCE_SAMPLE_RATE,
        SOURCE_CHANNELS,
        None,
    );
    client.initialize_client(
        &format,
        &Direction::Render,
        &StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: period,
        },
    )?;
    Ok((client, period))
}

fn main() {
    let hold_index: Option<usize> = std::env::args()
        .skip_while(|argument| argument != "--hold")
        .nth(1)
        .and_then(|value| value.parse().ok());

    let mta = wasapi::initialize_mta();
    if mta.is_err() {
        println!("initialize_mta failed: hr=0x{:08X}", mta.0 as u32);
        std::process::exit(1);
    }

    let enumerator = match DeviceEnumerator::new() {
        Ok(enumerator) => enumerator,
        Err(error) => {
            println!("DeviceEnumerator::new failed: {}", describe(&error));
            std::process::exit(1);
        }
    };
    let collection = match enumerator.get_device_collection(&Direction::Render) {
        Ok(collection) => collection,
        Err(error) => {
            println!("get_device_collection(Render) failed: {}", describe(&error));
            std::process::exit(1);
        }
    };

    let mut names = Vec::<String>::new();
    let mut candidates = Vec::<bool>::new();
    let mut devices = Vec::<wasapi::Device>::new();
    for device in &collection {
        let device = match device {
            Ok(device) => device,
            Err(error) => {
                println!("read device failed: {}", describe(&error));
                continue;
            }
        };
        let id = match device.get_id() {
            Ok(id) => id,
            Err(error) => {
                println!("read id failed: {}", describe(&error));
                continue;
            }
        };
        let name = match device.get_friendlyname() {
            Ok(name) => name,
            Err(error) => {
                println!("read name failed: {}", describe(&error));
                continue;
            }
        };
        let candidate = sayall_windows::is_virtual_cable_output_name(&name);
        println!(
            "#{} id={} name={name} virtual_cable={candidate}",
            names.len(),
            id
        );
        names.push(name);
        candidates.push(candidate);
        devices.push(device);
    }
    println!("--- endpoints: {} ---", names.len());

    println!("--- 1) baseline: open+release each endpoint ---");
    for index in 0..devices.len() {
        match open_client(&devices[index]) {
            Ok((_client, period)) => println!(
                "[baseline] #{index} ok period_hns={period} name={}",
                names[index]
            ),
            Err(error) => println!(
                "[baseline] #{index} fail {} name={}",
                describe(&error),
                names[index]
            ),
        }
    }

    let hold_index = hold_index.unwrap_or_else(|| {
        candidates
            .iter()
            .position(|candidate| *candidate)
            .unwrap_or(0)
    });
    println!("--- 2) hold #{hold_index} then open every endpoint (含同端点重开) ---");
    match open_client(&devices[hold_index]) {
        Ok((held, period)) => {
            println!(
                "[hold] #{hold_index} held ok period_hns={period} name={}",
                names[hold_index]
            );
            for index in 0..devices.len() {
                match open_client(&devices[index]) {
                    Ok((_client, _period)) => {
                        println!("[hold:#{hold_index}] #{index} ok name={}", names[index])
                    }
                    Err(error) => println!(
                        "[hold:#{hold_index}] #{index} fail {} name={}",
                        describe(&error),
                        names[index]
                    ),
                }
            }
            drop(held);
        }
        Err(error) => println!("[hold] #{hold_index} cannot be held: {}", describe(&error)),
    }

    println!("--- 3) same-endpoint double open, nothing held ---");
    for index in 0..devices.len() {
        if let Ok((first, _)) = open_client(&devices[index]) {
            match open_client(&devices[index]) {
                Ok((_client, _)) => println!(
                    "[reopen] #{index} two shared clients ok name={}",
                    names[index]
                ),
                Err(error) => println!(
                    "[reopen] #{index} second client fail {} name={}",
                    describe(&error),
                    names[index]
                ),
            }
            drop(first);
        }
    }

    println!("--- 4) mix format + parameter variants per endpoint ---");
    for index in 0..devices.len() {
        let mut client = match devices[index].get_iaudioclient() {
            Ok(client) => client,
            Err(error) => {
                println!(
                    "[variants] #{index} get_iaudioclient fail {}",
                    describe(&error)
                );
                continue;
            }
        };
        let (mix_rate, mix_channels) = match client.get_mixformat() {
            Ok(format) => {
                println!(
                    "[variants] #{index} mixformat sample_rate={} channels={} bits={} valid_bits={} name={}",
                    format.get_samplespersec(),
                    format.get_nchannels(),
                    format.get_bitspersample(),
                    format.get_validbitspersample(),
                    names[index]
                );
                (
                    format.get_samplespersec() as usize,
                    format.get_nchannels() as usize,
                )
            }
            Err(error) => {
                println!(
                    "[variants] #{index} get_mixformat fail {} name={}",
                    describe(&error),
                    names[index]
                );
                (0, 0)
            }
        };
        let variants: Vec<(String, usize, usize, bool, bool)> = vec![
            (
                "16k/mono/shared/autoconvert".to_owned(),
                SOURCE_SAMPLE_RATE,
                SOURCE_CHANNELS,
                true,
                false,
            ),
            (
                format!("mix-{mix_rate}/{mix_channels}ch/shared"),
                mix_rate.max(1),
                mix_channels.max(1),
                false,
                false,
            ),
            (
                "48k/2ch/shared/autoconvert".to_owned(),
                48_000,
                2,
                true,
                false,
            ),
            (
                "16k/mono/exclusive".to_owned(),
                SOURCE_SAMPLE_RATE,
                SOURCE_CHANNELS,
                false,
                true,
            ),
        ];
        for (label, rate, channels, autoconvert, exclusive) in variants {
            let mut client = match devices[index].get_iaudioclient() {
                Ok(client) => client,
                Err(error) => {
                    println!(
                        "[variants] #{index} {label} get_iaudioclient fail {}",
                        describe(&error)
                    );
                    continue;
                }
            };
            let (period, _) = match client.get_device_period() {
                Ok(period) => period,
                Err(error) => {
                    println!(
                        "[variants] #{index} {label} get_device_period fail {}",
                        describe(&error)
                    );
                    continue;
                }
            };
            let format = WaveFormat::new(16, 16, &SampleType::Int, rate, channels, None);
            let mode = if exclusive {
                StreamMode::PollingExclusive {
                    buffer_duration_hns: period,
                    period_hns: period,
                }
            } else {
                StreamMode::PollingShared {
                    autoconvert,
                    buffer_duration_hns: period,
                }
            };
            match client.initialize_client(&format, &Direction::Render, &mode) {
                Ok(()) => println!("[variants] #{index} {label} ok name={}", names[index]),
                Err(error) => println!(
                    "[variants] #{index} {label} fail {} name={}",
                    describe(&error),
                    names[index]
                ),
            }
        }
    }

    println!("--- 5) release→immediate reopen on the same VB device (app failure repro) ---");
    // 现场现象：应用把持有中的「扬声器 (2- VB-Audio Virtual Cable)」释放后立刻初始化
    // 「CABLE In 16 Ch (2- VB-Audio Virtual Cable)」（两者同属 ROOT\MEDIA\0001），
    // 会稳定拿到 0x8889000A；而两个端点各自单独打开都正常。这里逐档加延迟测释放窗口。
    let speaker = 0usize;
    let sixteen = names
        .iter()
        .position(|name| name.contains("CABLE In 16 Ch"));
    let sixteen = match sixteen {
        Some(index) => index,
        None => {
            println!("[race] 未找到 CABLE In 16 Ch 端点，跳过");
            println!("--- probe done ---");
            return;
        }
    };
    for delay_ms in [0u64, 5, 10, 20, 50, 100, 200, 400, 800] {
        let held = match open_client(&devices[speaker]) {
            Ok(held) => held,
            Err(error) => {
                println!("[race] hold speaker fail {}，跳过", describe(&error));
                break;
            }
        };
        drop(held);
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        let started = std::time::Instant::now();
        match open_client(&devices[sixteen]) {
            Ok((_client, _period)) => println!(
                "[race] delay={delay_ms}ms open-16Ch ok elapsed_ms={}",
                started.elapsed().as_millis()
            ),
            Err(error) => println!(
                "[race] delay={delay_ms}ms open-16Ch fail {} elapsed_ms={}",
                describe(&error),
                started.elapsed().as_millis()
            ),
        }
    }

    println!("--- probe done ---");
}
