//! 通知区域（托盘）图标：样式与状态呈现（2026-10-02 设置页「托盘图标」）。
//!
//! 对齐 Mac main 的菜单栏图标逻辑（`Sources/RemoteMic/StatusItemPresentation.swift`）：
//!
//! - 形状取自 Mac `Resources/StatusIconTemplate`，由
//!   `scripts/generate-tray-icons.py` 派生为 Windows 用的双色、四尺寸 PNG
//!   （Mac 的 `StatusIconActiveTemplate` 与基础模板逐字节相同，因此"语音中"
//!   不另造图标，与 Mac main 的呈现一致）；
//! - 未连接遥控器时整体变暗，对应 Mac 的 `NSStatusBarButton.appearsDisabled`；
//! - 图标颜色跟随任务栏明暗（`SystemUsesLightTheme`），尺寸按窗口 DPI 选最近档。
//!
//! `AppIcon` 样式沿用应用图标（历史行为与老配置默认），不做变暗。

use sayall_core::{TrayIconStyle, VoiceSessionState};
use sayall_windows::{ConnectionPhase, PlatformSnapshot};
use tauri::image::Image;
use tauri::AppHandle;
use tauri::Manager;

/// 托盘 ID；setup 里创建托盘与这里换图必须用同一常量。
pub const TRAY_ID: &str = "sayall-tray";

/// 未连接时的 alpha 比例（百分比）——对应 Mac `appearsDisabled` 的变暗。
const DISCONNECTED_ALPHA_PERCENT: u32 = 45;

/// 预生成尺寸（100% / 125% / 150% / 200% 缩放）。
const ICON_SIZES: [u32; 4] = [16, 20, 24, 32];

/// 前端按运行快照推导后上报的托盘呈现状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayIconState {
    pub connected: bool,
    pub streaming: bool,
}

impl TrayIconState {
    /// 尚无运行快照时的初值：按未连接处理（与 Mac 启动时 `disconnected` 一致）。
    pub const DISCONNECTED: Self = Self {
        connected: false,
        streaming: false,
    };

    pub fn from_platform(snapshot: &PlatformSnapshot) -> Self {
        let connected = matches!(
            snapshot.connection.phase,
            ConnectionPhase::Ready | ConnectionPhase::Streaming | ConnectionPhase::Draining
        );
        Self {
            connected,
            streaming: connected && snapshot.connection.voice_state == VoiceSessionState::Streaming,
        }
    }

    /// 日志用的状态名（不含任何用户信息）。
    pub fn label(self) -> &'static str {
        match (self.connected, self.streaming) {
            (false, _) => "disconnected",
            (true, true) => "streaming",
            (true, false) => "connected",
        }
    }
}

/// 单色图标的取色：深色任务栏用白图标，浅色任务栏用黑图标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIconTone {
    Black,
    White,
}

impl TrayIconTone {
    pub fn label(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::White => "white",
        }
    }
}

pub fn tone_for_taskbar(uses_light_theme: bool) -> TrayIconTone {
    if uses_light_theme {
        TrayIconTone::Black
    } else {
        TrayIconTone::White
    }
}

/// 目标尺寸向上取最近一档预生成图标（16/20/24/32），超出最大档时用 32。
pub fn nearest_icon_size(dpi: u32) -> u32 {
    let desired = (16 * u64::from(dpi.max(96)) + 48) / 96;
    ICON_SIZES
        .iter()
        .copied()
        .find(|size| u64::from(*size) >= desired)
        .unwrap_or(32)
}

fn status_icon_bytes(tone: TrayIconTone, size: u32) -> &'static [u8] {
    match (tone, size) {
        (TrayIconTone::Black, 16) => include_bytes!("../icons/tray/tray-status-idle-black-16.png"),
        (TrayIconTone::Black, 20) => include_bytes!("../icons/tray/tray-status-idle-black-20.png"),
        (TrayIconTone::Black, 24) => include_bytes!("../icons/tray/tray-status-idle-black-24.png"),
        (TrayIconTone::Black, _) => include_bytes!("../icons/tray/tray-status-idle-black-32.png"),
        (TrayIconTone::White, 16) => include_bytes!("../icons/tray/tray-status-idle-white-16.png"),
        (TrayIconTone::White, 20) => include_bytes!("../icons/tray/tray-status-idle-white-20.png"),
        (TrayIconTone::White, 24) => include_bytes!("../icons/tray/tray-status-idle-white-24.png"),
        (TrayIconTone::White, _) => include_bytes!("../icons/tray/tray-status-idle-white-32.png"),
    }
}

/// 单色状态图标位图：解码内置 PNG，未连接时按比例压低 alpha（变暗）。
pub fn status_icon_image(
    tone: TrayIconTone,
    size: u32,
    state: TrayIconState,
) -> Result<Image<'static>, String> {
    let image = Image::from_bytes(status_icon_bytes(tone, size))
        .map_err(|error| format!("解码内置托盘图标失败：{error}"))?;
    if state.connected {
        return Ok(image);
    }
    let mut rgba = image.rgba().to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = (u32::from(pixel[3]) * DISCONNECTED_ALPHA_PERCENT / 100) as u8;
    }
    Ok(Image::new_owned(rgba, image.width(), image.height()))
}

/// 任务栏（通知区域）是否使用浅色主题：HKCU\...\Themes\Personalize\SystemUsesLightTheme。
///
/// 读失败返回 None（调用方按浅色任务栏处理并在日志里记明），不需要管理员权限。
#[cfg(windows)]
fn taskbar_uses_light_theme() -> Option<bool> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    };

    const PERSONALIZE_KEY: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
    const VALUE_NAME: &str = "SystemUsesLightTheme";

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let subkey = wide(PERSONALIZE_KEY);
    let name = wide(VALUE_NAME);
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if status.is_err() {
        return None;
    }
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            None,
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        )
    };
    unsafe {
        let _ = RegCloseKey(key);
    };
    if status.is_err() || size != std::mem::size_of::<u32>() as u32 {
        return None;
    }
    Some(value != 0)
}

#[cfg(not(windows))]
fn taskbar_uses_light_theme() -> Option<bool> {
    None
}

/// 主窗口所在显示器缩放因子 → DPI（拿不到时按 96 处理）。
///
/// 通知区域由系统按任务栏所在显示器绘制，这里用主窗口 DPI 近似；预生成的
/// 四档尺寸覆盖 100%–200%，偏差最多一档，不影响可读性。
fn window_dpi(app: &AppHandle) -> u32 {
    app.get_webview_window("main")
        .and_then(|window| window.scale_factor().ok())
        .map(|scale| (scale * 96.0).round().max(96.0) as u32)
        .unwrap_or(96)
}

fn resolve_icon(
    app: &AppHandle,
    style: TrayIconStyle,
    state: TrayIconState,
    dpi: u32,
) -> Result<(Image<'static>, String), String> {
    match style {
        TrayIconStyle::AppIcon => app
            .default_window_icon()
            .cloned()
            // `default_window_icon` 借用 AppHandle，换到自有缓冲才能与状态图标走同一返回类型。
            .map(|icon| (icon.to_owned(), "tone=app size=default".to_owned()))
            .ok_or_else(|| "缺少应用图标".to_owned()),
        TrayIconStyle::StatusIcon => {
            let light_taskbar = taskbar_uses_light_theme().unwrap_or(true);
            let tone = tone_for_taskbar(light_taskbar);
            let size = nearest_icon_size(dpi);
            let icon = status_icon_image(tone, size, state)?;
            Ok((icon, format!("tone={} size={size}", tone.label())))
        }
    }
}

/// 把样式与状态应用到托盘图标。托盘不存在（仿真构建、创建失败）时只记日志：
/// 图标样式属于增强表现，绝不影响语音与按键主路径。
pub fn apply(app: &AppHandle, style: TrayIconStyle, state: TrayIconState) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        sayall_windows::gatt_note(format!(
            "tray_icon feature=tray action=apply phase=completed terminal_result=skipped style={} state={} reason=tray_unavailable",
            style_label(style),
            state.label()
        ));
        return;
    };
    let dpi = window_dpi(app);
    let (icon, detail) = match resolve_icon(app, style, state, dpi) {
        Ok(resolved) => resolved,
        Err(error) => {
            sayall_windows::gatt_note(format!(
                "tray_icon feature=tray action=apply phase=completed terminal_result=failed style={} state={} error_domain=icon error_code=resolve_failed retryable=true",
                style_label(style),
                state.label()
            ));
            eprintln!("{error}");
            return;
        }
    };
    match tray.set_icon(Some(icon)) {
        Ok(()) => sayall_windows::gatt_note(format!(
            "tray_icon feature=tray action=apply phase=completed terminal_result=passed style={} state={} dpi={dpi} {detail}",
            style_label(style),
            state.label()
        )),
        Err(error) => {
            sayall_windows::gatt_note(format!(
                "tray_icon feature=tray action=apply phase=completed terminal_result=failed style={} state={} error_domain=tray error_code=set_icon_failed retryable=true",
                style_label(style),
                state.label()
            ));
            eprintln!("更新托盘图标失败：{error}");
        }
    }
}

pub fn style_label(style: TrayIconStyle) -> &'static str {
    match style {
        TrayIconStyle::AppIcon => "app_icon",
        TrayIconStyle::StatusIcon => "status_icon",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with(phase: ConnectionPhase, voice_state: VoiceSessionState) -> PlatformSnapshot {
        let mut snapshot = PlatformSnapshot {
            platform: "test".to_owned(),
            windows_api_available: true,
            ble_scan_available: true,
            ble_voice_ready: true,
            wasapi_ready: true,
            raw_input_ready: true,
            send_input_ready: true,
            verification_status: "test".to_owned(),
            connection: Default::default(),
            audio: Default::default(),
            raw_input: Default::default(),
            button_mapping: Default::default(),
        };
        snapshot.connection.phase = phase;
        snapshot.connection.voice_state = voice_state;
        snapshot
    }

    #[test]
    fn platform_snapshot_maps_to_presentation_states() {
        // 与 Mac StatusItemPresentation.resolve 相同的判定：语音中 > 已连接 > 未连接。
        let disconnected = TrayIconState::from_platform(&snapshot_with(
            ConnectionPhase::Disconnected,
            VoiceSessionState::Idle,
        ));
        assert_eq!(disconnected.label(), "disconnected");

        let connecting = TrayIconState::from_platform(&snapshot_with(
            ConnectionPhase::Connecting,
            VoiceSessionState::Idle,
        ));
        assert_eq!(connecting.label(), "disconnected");

        let connected = TrayIconState::from_platform(&snapshot_with(
            ConnectionPhase::Ready,
            VoiceSessionState::Idle,
        ));
        assert_eq!(connected.label(), "connected");

        let streaming = TrayIconState::from_platform(&snapshot_with(
            ConnectionPhase::Streaming,
            VoiceSessionState::Streaming,
        ));
        assert_eq!(streaming.label(), "streaming");
        assert!(streaming.connected);
    }

    #[test]
    fn icon_size_follows_window_dpi() {
        assert_eq!(nearest_icon_size(96), 16);
        assert_eq!(nearest_icon_size(120), 20);
        assert_eq!(nearest_icon_size(144), 24);
        assert_eq!(nearest_icon_size(192), 32);
        // 低于 100%（异常值）与高于 200% 都收敛到可用档位。
        assert_eq!(nearest_icon_size(48), 16);
        assert_eq!(nearest_icon_size(384), 32);
    }

    #[test]
    fn tone_follows_taskbar_theme() {
        assert_eq!(tone_for_taskbar(true), TrayIconTone::Black);
        assert_eq!(tone_for_taskbar(false), TrayIconTone::White);
    }

    #[test]
    fn status_icon_dims_only_when_disconnected() {
        let connected = TrayIconState {
            connected: true,
            streaming: false,
        };
        let normal = status_icon_image(TrayIconTone::White, 16, connected).unwrap();
        assert_eq!((normal.width(), normal.height()), (16, 16));
        let opaque = normal
            .rgba()
            .chunks_exact(4)
            .map(|pixel| pixel[3])
            .max()
            .unwrap();
        assert_eq!(opaque, 255, "已连接的图标不应变暗");

        let dimmed =
            status_icon_image(TrayIconTone::White, 16, TrayIconState::DISCONNECTED).unwrap();
        let dimmest = dimmed
            .rgba()
            .chunks_exact(4)
            .map(|pixel| pixel[3])
            .max()
            .unwrap();
        assert!(dimmest > 0, "变暗不是消失");
        assert!(
            u32::from(dimmest) <= u32::from(opaque) * DISCONNECTED_ALPHA_PERCENT / 100,
            "未连接时 alpha 应压到 {DISCONNECTED_ALPHA_PERCENT}% 以内"
        );

        // 取色：浅色任务栏给黑色像素，深色任务栏给白色像素。
        let black = status_icon_image(TrayIconTone::Black, 16, connected).unwrap();
        let colored_pixel = |image: &Image<'static>| {
            image
                .rgba()
                .chunks_exact(4)
                .find(|pixel| pixel[3] > 200)
                .map(|pixel| (pixel[0], pixel[1], pixel[2]))
        };
        assert_eq!(colored_pixel(&black), Some((0, 0, 0)));
        assert_eq!(colored_pixel(&normal), Some((255, 255, 255)));
    }

    #[test]
    fn every_embedded_size_decodes() {
        for size in ICON_SIZES {
            for tone in [TrayIconTone::Black, TrayIconTone::White] {
                let image = status_icon_image(tone, size, TrayIconState::DISCONNECTED)
                    .unwrap_or_else(|error| panic!("{size}px/{tone:?} 内置图标解码失败：{error}"));
                assert_eq!((image.width(), image.height()), (size, size));
            }
        }
    }
}
