//! 应用图标：把用户选择的应用图标应用到所有显示它的地方（2026-10-02）。
//!
//! 对齐 Mac main 的 `Sources/RemoteMic/AppIconController.swift`：
//!
//! - 稳定语义 ID + 目录解析：`standard` 是内置应用图标，`faceted-duck`（几何鸭）
//!   来自 Mac `Resources/AppIcons/faceted-duck.png`，由
//!   `scripts/generate-app-icons.py` 派生为 Windows 用的尺寸档；认不出的 ID 一律
//!   回落 `standard`（Mac `AppIconCatalog.resolvedIdentifier(for:)` 同款语义）；
//! - 落日志 `app_icon action=apply phase=... result=applied|fallback reason=...`
//!   （对应 Mac 的 `APP_ICON CHANGE`）；
//! - Mac 换的是 `NSApplication.applicationIconImage`（Dock / 应用切换器 / 设置窗口）；
//!   Windows 上等价的"各个地方" = **主窗口图标（任务栏 + Alt-Tab + 标题栏）与通知
//!   区域托盘图标**，设置页顶部标识由前端按同一选择实时渲染。安装包、开始菜单快捷
//!   方式与可执行文件自身的图标属于安装产物，运行期不可改（Mac 的 bundle 图标同样不变）。

use sayall_core::AppIconIdentifier;
use tauri::image::Image;
use tauri::AppHandle;
use tauri::Manager;

/// 托盘 ID；setup 里创建托盘与这里换图标必须用同一常量。
pub const TRAY_ID: &str = "sayall-tray";

/// 托盘图标按 DPI 选档（100% / 125% / 150% / 200% 缩放）。
const TRAY_ICON_SIZES: [u32; 4] = [16, 20, 24, 32];

/// 目标尺寸向上取最近一档预生成托盘图标，超出最大档时用 32。
pub fn nearest_icon_size(dpi: u32) -> u32 {
    let desired = (16 * u64::from(dpi.max(96)) + 48) / 96;
    TRAY_ICON_SIZES
        .iter()
        .copied()
        .find(|size| u64::from(*size) >= desired)
        .unwrap_or(32)
}

fn faceted_duck_tray_bytes(size: u32) -> &'static [u8] {
    match size {
        16 => include_bytes!("../icons/app-icons/faceted-duck-16.png"),
        20 => include_bytes!("../icons/app-icons/faceted-duck-20.png"),
        24 => include_bytes!("../icons/app-icons/faceted-duck-24.png"),
        _ => include_bytes!("../icons/app-icons/faceted-duck-32.png"),
    }
}

/// 托盘尺寸的几何鸭图标；`standard` 由 `default_window_icon` 提供，不走这里。
pub fn faceted_duck_tray_image(size: u32) -> Result<Image<'static>, String> {
    Image::from_bytes(faceted_duck_tray_bytes(size))
        .map_err(|error| format!("解码内置几何鸭托盘图标失败：{error}"))
}

/// 窗口（任务栏 / Alt-Tab）尺寸的几何鸭图标。
pub fn faceted_duck_window_image() -> Result<Image<'static>, String> {
    Image::from_bytes(include_bytes!("../icons/app-icons/faceted-duck-256.png"))
        .map_err(|error| format!("解码内置几何鸭窗口图标失败：{error}"))
}

pub fn style_label(identifier: AppIconIdentifier) -> &'static str {
    match identifier {
        AppIconIdentifier::Standard => "standard",
        AppIconIdentifier::FacetedDuck => "faceted-duck",
    }
}

/// 主窗口所在显示器缩放因子 → DPI（拿不到时按 96 处理）。
///
/// 通知区域由系统按任务栏所在显示器绘制，这里用主窗口 DPI 近似；预生成的四档
/// 尺寸覆盖 100%–200%，偏差最多一档，不影响可读性。
fn window_dpi(app: &AppHandle) -> u32 {
    app.get_webview_window("main")
        .and_then(|window| window.scale_factor().ok())
        .map(|scale| (scale * 96.0).round().max(96.0) as u32)
        .unwrap_or(96)
}

/// 把选择的应用图标应用到主窗口与托盘；返回真正生效的 ID（回落时 ≠ 请求值）。
///
/// 任何一步失败都只记录并继续：图标属于表现层，绝不影响语音与按键主路径。
pub fn apply(app: &AppHandle, requested: AppIconIdentifier) -> AppIconIdentifier {
    let applied = resolve(requested);
    sayall_windows::gatt_note(format!(
        "app_icon action=apply phase=requested requested={} source=preference",
        style_label(requested)
    ));

    let window_icon = match applied {
        AppIconIdentifier::Standard => app
            .default_window_icon()
            .cloned()
            .map(|icon| icon.to_owned()),
        AppIconIdentifier::FacetedDuck => faceted_duck_window_image().ok(),
    };
    match (&window_icon, app.get_webview_window("main")) {
        (Some(icon), Some(window)) => {
            if let Err(error) = window.set_icon(icon.clone()) {
                sayall_windows::gatt_note(
                    "app_icon action=apply target=window phase=completed terminal_result=failed error_domain=window error_code=set_icon_failed retryable=true"
                        .to_owned(),
                );
                eprintln!("更新窗口图标失败：{error}");
            }
        }
        (None, _) => sayall_windows::gatt_note(
            "app_icon action=apply target=window phase=completed terminal_result=failed error_domain=icon error_code=resolve_failed retryable=true"
                .to_owned(),
        ),
        // 仿真构建没有 WebView 窗口的图标路径差异，窗口不存在时静默跳过。
        (Some(_), None) => {}
    }

    let dpi = window_dpi(app);
    let tray_icon = match applied {
        AppIconIdentifier::Standard => app
            .default_window_icon()
            .cloned()
            .map(|icon| icon.to_owned()),
        AppIconIdentifier::FacetedDuck => faceted_duck_tray_image(nearest_icon_size(dpi)).ok(),
    };
    match app.tray_by_id(TRAY_ID) {
        Some(tray) => match tray_icon {
            Some(icon) => match tray.set_icon(Some(icon)) {
                Ok(()) => sayall_windows::gatt_note(format!(
                    "app_icon action=apply target=tray phase=completed terminal_result=passed applied={} dpi={dpi}",
                    style_label(applied)
                )),
                Err(error) => {
                    sayall_windows::gatt_note(format!(
                        "app_icon action=apply target=tray phase=completed terminal_result=failed applied={} error_domain=tray error_code=set_icon_failed retryable=true",
                        style_label(applied)
                    ));
                    eprintln!("更新托盘图标失败：{error}");
                }
            },
            None => sayall_windows::gatt_note(format!(
                "app_icon action=apply target=tray phase=completed terminal_result=failed applied={} error_domain=icon error_code=resolve_failed retryable=true",
                style_label(applied)
            )),
        },
        // 仿真构建不建托盘；真机上托盘创建失败也走这里。只记日志，不报错。
        None => sayall_windows::gatt_note(format!(
            "app_icon action=apply target=tray phase=completed terminal_result=skipped applied={} reason=tray_unavailable",
            style_label(applied)
        )),
    }

    if applied == requested {
        sayall_windows::gatt_note(format!(
            "app_icon action=apply phase=completed terminal_result=passed requested={} applied={} result=applied reason=selection_available",
            style_label(requested),
            style_label(applied)
        ));
    } else {
        sayall_windows::gatt_note(format!(
            "app_icon action=apply phase=completed terminal_result=passed requested={} applied={} result=fallback reason=resource_unavailable",
            style_label(requested),
            style_label(applied)
        ));
    }
    applied
}

/// 目录解析：目前两个 ID 的内置资产都在仓库里，未知 ID 不在枚举内（反序列化时
/// 已回落 `standard`）；这里保留 Mac 同款入口，供后续新增图标时集中处理。
fn resolve(requested: AppIconIdentifier) -> AppIconIdentifier {
    match requested {
        AppIconIdentifier::Standard | AppIconIdentifier::FacetedDuck => requested,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn style_labels_match_persisted_identifiers() {
        assert_eq!(style_label(AppIconIdentifier::Standard), "standard");
        assert_eq!(style_label(AppIconIdentifier::FacetedDuck), "faceted-duck");
    }

    #[test]
    fn every_embedded_faceted_duck_size_decodes() {
        for size in TRAY_ICON_SIZES {
            let image = faceted_duck_tray_image(size)
                .unwrap_or_else(|error| panic!("{size}px 内置几何鸭图标解码失败：{error}"));
            assert_eq!((image.width(), image.height()), (size, size));
        }
        let window_icon = faceted_duck_window_image().expect("窗口尺寸的几何鸭图标必须可解码");
        assert_eq!((window_icon.width(), window_icon.height()), (256, 256));
    }

    #[test]
    fn resolution_keeps_known_identifiers() {
        assert_eq!(
            resolve(AppIconIdentifier::FacetedDuck),
            AppIconIdentifier::FacetedDuck
        );
        assert_eq!(
            resolve(AppIconIdentifier::Standard),
            AppIconIdentifier::Standard
        );
    }
}
