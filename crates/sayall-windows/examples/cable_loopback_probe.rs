// VB-CABLE 回环取证探针：判定「哪个渲染端点真正接到 CABLE Output 录音端」。
//
// 为什么需要它（2026-10-02 现场）：Andy 机器上 VB-CABLE 驱动换成带 16 Ch 端点的新版后，
// 渲染端同时存在「扬声器 (2- VB-Audio Virtual Cable)」与「CABLE In 16 Ch (2- VB-Audio
// Virtual Cable)」两个候选，录音端只有「CABLE Output (2- VB-Audio Virtual Cable)」。
// 应用把声音写进哪个渲染端点才能被输入法从 CABLE Output 读到，只能实测。
//
// 同时覆盖 TODO「Windows 真机验证 WASAPI 端点初始化、VB-CABLE 回环」。
//
// 用法：
//   cargo run --release -p sayall-windows --example cable_loopback_probe
//   cargo run --release -p sayall-windows --example cable_loopback_probe -- --render "CABLE In 16 Ch"
use std::time::{Duration, Instant};
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

const SAMPLE_RATE: usize = 16_000;
const CHANNELS: usize = 1;
const TONE_HZ: f32 = 1000.0;
const RENDER_MS: u64 = 900;
const CHUNK_MS: u64 = 10;

fn describe(error: &wasapi::WasapiError) -> String {
    match error {
        wasapi::WasapiError::Windows(inner) => {
            format!("{error} hresult=0x{:08X}", inner.code().0 as u32)
        }
        other => format!("{other}"),
    }
}

struct Endpoint {
    id: String,
    name: String,
}

fn collect(enumerator: &DeviceEnumerator, direction: &Direction) -> Vec<Endpoint> {
    let collection = match enumerator.get_device_collection(direction) {
        Ok(collection) => collection,
        Err(error) => {
            println!("枚举 {:?} 端点失败：{}", direction, describe(&error));
            return Vec::new();
        }
    };
    let mut endpoints = Vec::new();
    for device in &collection {
        let device = match device {
            Ok(device) => device,
            Err(error) => {
                println!("读取端点失败：{}", describe(&error));
                continue;
            }
        };
        let (id, name) = match (device.get_id(), device.get_friendlyname()) {
            (Ok(id), Ok(name)) => (id, name),
            _ => continue,
        };
        endpoints.push(Endpoint { id, name });
    }
    endpoints
}

fn open_render(
    enumerator: &DeviceEnumerator,
    id: &str,
) -> Result<(AudioClient, wasapi::AudioRenderClient), wasapi::WasapiError> {
    let device = enumerator.get_device(id)?;
    let mut client = device.get_iaudioclient()?;
    let (period, _) = client.get_device_period()?;
    let format = WaveFormat::new(16, 16, &SampleType::Int, SAMPLE_RATE, CHANNELS, None);
    client.initialize_client(
        &format,
        &Direction::Render,
        &StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: period,
        },
    )?;
    let render = client.get_audiorenderclient()?;
    Ok((client, render))
}

fn open_capture(
    enumerator: &DeviceEnumerator,
    id: &str,
) -> Result<(AudioClient, wasapi::AudioCaptureClient), wasapi::WasapiError> {
    let device = enumerator.get_device(id)?;
    let mut client = device.get_iaudioclient()?;
    let (period, _) = client.get_device_period()?;
    let format = WaveFormat::new(16, 16, &SampleType::Int, SAMPLE_RATE, CHANNELS, None);
    client.initialize_client(
        &format,
        &Direction::Capture,
        &StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: period,
        },
    )?;
    let capture = client.get_audiocaptureclient()?;
    Ok((client, capture))
}

fn tone_chunk(chunk_index: usize, chunks: usize) -> Vec<u8> {
    let frames = SAMPLE_RATE as u64 * CHUNK_MS / 1000;
    let mut bytes = Vec::with_capacity(frames as usize * 2);
    let fade_chunks = 3.max(chunks / 20);
    let gain = if chunk_index < fade_chunks || chunk_index + fade_chunks >= chunks {
        0.4
    } else {
        0.7
    };
    for frame in 0..frames {
        let t = (chunk_index as f32 * frames as f32 + frame as f32) / SAMPLE_RATE as f32;
        let value = (2.0 * std::f32::consts::PI * TONE_HZ * t).sin() * gain * i16::MAX as f32;
        bytes.extend_from_slice(&(value as i16).to_le_bytes());
    }
    bytes
}

fn peak_of(bytes: &[u8]) -> i32 {
    bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as i32)
        .map(i32::abs)
        .max()
        .unwrap_or(0)
}

fn probe_render(
    enumerator: &DeviceEnumerator,
    render_id: &str,
    render_name: &str,
    capture_id: &str,
    capture_name: &str,
) {
    println!("--- render = {render_name} ---");
    let (render_client, render) = match open_render(enumerator, render_id) {
        Ok(pair) => pair,
        Err(error) => {
            println!("[loop] 渲染端点打不开：{}", describe(&error));
            return;
        }
    };
    let (capture_client, capture) = match open_capture(enumerator, capture_id) {
        Ok(pair) => pair,
        Err(error) => {
            println!("[loop] 录音端点打不开：{}", describe(&error));
            return;
        }
    };
    if let Err(error) = render_client.start_stream() {
        println!("[loop] 渲染流启动失败：{}", describe(&error));
        return;
    }
    if let Err(error) = capture_client.start_stream() {
        println!("[loop] 录音流启动失败：{}", describe(&error));
        let _ = render_client.stop_stream();
        return;
    }

    let chunks = (RENDER_MS / CHUNK_MS) as usize;
    let started = Instant::now();
    let mut render_chunk = 0usize;
    let mut peak = 0i32;
    let mut packet_bytes = vec![0u8; 4096];
    let mut next_render = Instant::now();
    while started.elapsed() < Duration::from_millis(RENDER_MS + 250) {
        if render_chunk < chunks && Instant::now() >= next_render {
            let data = tone_chunk(render_chunk, chunks);
            let frames = data.len() / 2;
            if let Ok(space) = render_client.get_available_space_in_frames() {
                if space as usize >= frames {
                    let _ = render.write_to_device(frames, &data, None);
                }
            }
            render_chunk += 1;
            next_render += Duration::from_millis(CHUNK_MS);
        }
        if let Ok(Some(frames)) = capture.get_next_packet_size() {
            if frames > 0 {
                if let Ok((read, _info)) = capture.read_from_device(&mut packet_bytes) {
                    let bytes = (read as usize) * 2;
                    peak = peak.max(peak_of(&packet_bytes[..bytes.min(packet_bytes.len())]));
                }
            }
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let _ = capture_client.stop_stream();
    let _ = render_client.stop_stream();
    let reached = peak > 300;
    println!(
        "[loop] render={render_name} -> capture={capture_name} peak={peak} reached={reached} rendered_ms={}",
        render_chunk as u64 * CHUNK_MS
    );
}

fn main() {
    let filter: Option<String> = std::env::args()
        .skip_while(|argument| argument != "--render")
        .nth(1);

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

    let renders = collect(&enumerator, &Direction::Render);
    let captures = collect(&enumerator, &Direction::Capture);
    println!("渲染端点：");
    for endpoint in &renders {
        println!("  {} | {}", endpoint.name, endpoint.id);
    }
    println!("录音端点：");
    for endpoint in &captures {
        println!("  {} | {}", endpoint.name, endpoint.id);
    }

    let capture = match captures
        .iter()
        .find(|endpoint| endpoint.name.to_lowercase().contains("cable output"))
    {
        Some(endpoint) => endpoint,
        None => {
            println!("未找到 CABLE Output 录音端点，结束");
            return;
        }
    };

    let candidates: Vec<&Endpoint> = renders
        .iter()
        .filter(|endpoint| {
            let name = endpoint.name.to_lowercase();
            name.contains("vb-audio") || name.contains("cable")
        })
        .filter(|endpoint| match &filter {
            Some(needle) => endpoint.name.contains(needle.as_str()),
            None => true,
        })
        .collect();
    if candidates.is_empty() {
        println!("没有可测的 VB-Audio 渲染端点，结束");
        return;
    }
    for endpoint in candidates {
        probe_render(
            &enumerator,
            &endpoint.id,
            &endpoint.name,
            &capture.id,
            &capture.name,
        );
    }
    println!("--- probe done ---");
}
