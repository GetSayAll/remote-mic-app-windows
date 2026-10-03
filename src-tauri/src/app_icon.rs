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
//!   Windows 上等价的"各个地方" = **主窗口图标（任务栏 + Alt-Tab + 标题栏）、通知
//!   区域托盘图标、以及开始菜单 / 桌面 / 固定到任务栏的快捷方式图标**，设置页顶部
//!   标识由前端按同一选择实时渲染。安装包与可执行文件自身的图标属于安装产物，运行期
//!   不可改（Mac 的 bundle 图标同样不变）；快捷方式图标由
//!   `crate::shortcut_icons::sync`（2026-10-03：快捷方式 `IconLocation=,0` 取的是 exe
//!   内嵌图标，不额外同步就不会跟随切换）。

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

/// 任务栏 / Alt-Tab 用的是 `ICON_BIG`，标题栏用的是 `ICON_SMALL`。
///
/// Tauri 的 `WebviewWindow::set_icon` 在 Windows 上只写 `ICON_SMALL`
/// （`tao::platform_impl::windows::window::set_window_icon` → `WM_SETICON(ICON_SMALL)`，
/// 2026-10-02 源码核对），`ICON_BIG` 只有 `set_taskbar_icon` 会写、而它不在 Tauri 的
/// 公开路径上——这正是用户现场"任务栏没有跟随切换图标"的根因（按钮图标不动、
/// 标题栏/通知区域却变了）。这里自己补 `ICON_BIG`，并顺带按 DPI 重设 `ICON_SMALL`。
#[cfg(windows)]
mod window_icons {
    use std::sync::Mutex;
    use windows::Win32::Foundation::{HWND, LPARAM, TRUE, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconIndirect, DestroyIcon, SendMessageW, HICON, ICONINFO, WM_SETICON,
    };

    /// 自己持有一份图标位图（RGBA，行主序）。
    pub struct IconBitmap {
        pub rgba: Vec<u8>,
        pub width: u32,
        pub height: u32,
    }

    /// `WM_SETICON` 不会接管句柄所有权：Windows 只保存指针，因此句柄必须由我们
    /// 保持有效，直到窗口换成下一张（提前 `DestroyIcon` 会让任务栏读到已释放的位图）。
    pub struct OwnedIconHandle(HICON);

    impl OwnedIconHandle {
        fn from_raw(handle: HICON) -> Self {
            Self(handle)
        }

        pub fn as_raw(&self) -> isize {
            self.0 .0 as isize
        }
    }

    impl Drop for OwnedIconHandle {
        fn drop(&mut self) {
            // SAFETY: 句柄由 CreateIconIndirect 创建且只在这里释放一次。
            unsafe {
                let _ = DestroyIcon(self.0);
            }
        }
    }

    // SAFETY: HICON 是进程级的 user32 句柄，任意线程都可以持有/使用；我们只用它
    // 保活（WM_SETICON 不接管所有权）并在替换时销毁一次，没有跨线程别名。
    unsafe impl Send for OwnedIconHandle {}

    /// 进程内最近一次交给主窗口的两个句柄；换新图后才释放上一代。
    static APPLIED_WINDOW_ICONS: Mutex<Option<(OwnedIconHandle, OwnedIconHandle)>> =
        Mutex::new(None);

    fn lock_applied() -> std::sync::MutexGuard<'static, Option<(OwnedIconHandle, OwnedIconHandle)>>
    {
        APPLIED_WINDOW_ICONS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// RGBA → HICON（`CreateIconIndirect`，与 tao 的 `RgbaIcon::into_windows_icon` 同路）。
    pub fn create_icon(bitmap: &IconBitmap) -> Result<OwnedIconHandle, String> {
        let (width, height) = (bitmap.width, bitmap.height);
        if width == 0 || height == 0 || bitmap.rgba.len() < (width * height * 4) as usize {
            return Err("图标位图尺寸与数据长度不一致".to_owned());
        }
        unsafe {
            let mut info = BITMAPINFO::default();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = width as i32;
            // 负高度 = 自上而下，与 RGBA 缓冲一致。
            info.bmiHeader.biHeight = -(height as i32);
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB.0;
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .map_err(|error| format!("创建图标位图失败：{error}"))?;
            {
                let pixels =
                    std::slice::from_raw_parts_mut(bits as *mut u8, (width * height * 4) as usize);
                // DIB 是 BGRA，输入是 RGBA。
                for (dst, src) in pixels.chunks_exact_mut(4).zip(bitmap.rgba.chunks_exact(4)) {
                    dst[0] = src[2];
                    dst[1] = src[1];
                    dst[2] = src[0];
                    dst[3] = src[3];
                }
            }
            // 全 0 掩码：alpha 通道已经给出透明度（32bpp 图标的标准做法）。
            let mask = CreateBitmap(width as i32, height as i32, 1, 1, None);
            let icon_info = ICONINFO {
                fIcon: TRUE,
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask,
                hbmColor: color,
            };
            let created = CreateIconIndirect(&icon_info);
            // 句柄创建后位图即可释放：HICON 自带副本。
            let _ = DeleteObject(HGDIOBJ(color.0));
            let _ = DeleteObject(HGDIOBJ(mask.0));
            created
                .map(OwnedIconHandle::from_raw)
                .map_err(|error| format!("创建图标句柄失败：{error}"))
        }
    }

    /// 把一张图标写到窗口的指定槽位（`ICON_BIG` / `ICON_SMALL`）。
    pub fn send_icon(hwnd: HWND, icon_type: u32, icon: &OwnedIconHandle) {
        unsafe {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(icon_type as usize)),
                Some(LPARAM(icon.as_raw())),
            );
        }
    }

    /// 读回窗口当前挂的图标句柄（0 = 未设置）。测试与诊断用。
    // 产品路径只写不读，非测试构建下没有调用点。
    #[allow(dead_code)]
    pub fn read_icon(hwnd: HWND, icon_type: u32) -> isize {
        use windows::Win32::UI::WindowsAndMessaging::WM_GETICON;
        unsafe {
            SendMessageW(
                hwnd,
                WM_GETICON,
                Some(WPARAM(icon_type as usize)),
                Some(LPARAM(0)),
            )
            .0
        }
    }

    /// 给窗口换任务栏（`ICON_BIG`）与标题栏（`ICON_SMALL`）图标。
    ///
    /// 调用方传裸句柄值：Tauri 自带的是 windows 0.61 的 `HWND`（tao 依赖），
    /// 本 crate 用的是 0.62；跨版本不能直接传类型，只能传指针值。
    pub fn apply(hwnd_raw: isize, big: &IconBitmap, small: &IconBitmap) -> Result<(), String> {
        let big_handle = create_icon(big)?;
        let small_handle = create_icon(small)?;
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        send_icon(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::ICON_BIG,
            &big_handle,
        );
        send_icon(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::ICON_SMALL,
            &small_handle,
        );
        // 换图成功后再让上一代句柄失效：窗口此刻已不再引用它们。
        *lock_applied() = Some((big_handle, small_handle));
        Ok(())
    }
}

#[cfg(windows)]
fn icon_bitmap_from_image(image: &Image<'_>) -> window_icons::IconBitmap {
    window_icons::IconBitmap {
        rgba: image.rgba().to_vec(),
        width: image.width(),
        height: image.height(),
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
    let dpi = window_dpi(app);
    let tray_icon = match applied {
        AppIconIdentifier::Standard => app
            .default_window_icon()
            .cloned()
            .map(|icon| icon.to_owned()),
        AppIconIdentifier::FacetedDuck => faceted_duck_tray_image(nearest_icon_size(dpi)).ok(),
    };

    match (&window_icon, app.get_webview_window("main")) {
        (Some(icon), Some(window)) => {
            // 跨平台路径（Windows 上只覆盖标题栏的 ICON_SMALL）。
            if let Err(error) = window.set_icon(icon.clone()) {
                sayall_windows::gatt_note(
                    "app_icon action=apply target=window phase=completed terminal_result=failed error_domain=window error_code=set_icon_failed retryable=true"
                        .to_owned(),
                );
                eprintln!("更新窗口图标失败：{error}");
            }
            // 任务栏 / Alt-Tab 走 ICON_BIG，必须自己补（见 window_icons 注释）。
            // 标题栏小图标同时按 DPI 重设，与通知区域用同一张。
            #[cfg(windows)]
            {
                let small_source = tray_icon.as_ref().unwrap_or(icon);
                let big = icon_bitmap_from_image(icon);
                let small = icon_bitmap_from_image(small_source);
                match window.hwnd() {
                    Ok(hwnd) => {
                        if let Err(error) = window_icons::apply(hwnd.0 as isize, &big, &small) {
                            sayall_windows::gatt_note(format!(
                                "app_icon action=apply target=taskbar phase=completed terminal_result=failed applied={} error_domain=windows error_code=set_icon_failed retryable=true",
                                style_label(applied)
                            ));
                            eprintln!("更新任务栏图标失败：{error}");
                        } else {
                            sayall_windows::gatt_note(format!(
                                "app_icon action=apply target=taskbar phase=completed terminal_result=passed applied={} big={}x{} small={}x{}",
                                style_label(applied),
                                big.width,
                                big.height,
                                small.width,
                                small.height
                            ));
                        }
                    }
                    Err(error) => {
                        sayall_windows::gatt_note(
                            "app_icon action=apply target=taskbar phase=completed terminal_result=failed error_domain=window error_code=hwnd_unavailable retryable=true"
                                .to_owned(),
                        );
                        eprintln!("读取主窗口句柄失败：{error}");
                    }
                }
            }
        }
        (None, _) => sayall_windows::gatt_note(
            "app_icon action=apply target=window phase=completed terminal_result=failed error_domain=icon error_code=resolve_failed retryable=true"
                .to_owned(),
        ),
        // 仿真构建没有 WebView 窗口的图标路径差异，窗口不存在时静默跳过。
        (Some(_), None) => {}
    }

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

    // 快捷方式（开始菜单 / 桌面 / 固定到任务栏）的图标不来自窗口句柄，单独同步
    // （2026-10-03：切换后开始菜单磁贴与固定项不变，就是缺这一步）。
    match crate::shortcut_icons::sync(applied) {
        Ok(report) => sayall_windows::gatt_note(format!(
            "app_icon action=sync_shortcuts phase=completed terminal_result=passed style={} considered={} updated={} failed={}",
            style_label(applied),
            report.considered,
            report.updated,
            report.failed
        )),
        Err(_) => sayall_windows::gatt_note(format!(
            "app_icon action=sync_shortcuts phase=completed terminal_result=failed style={} error_domain=shell error_code=sync_failed retryable=true",
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

    /// 回归测试（2026-10-03 现场："图标在任务栏和托盘都比别人的小一圈"）：
    ///
    /// 源图若是 macOS 图标网格的导出（图案只占画布 ~87%，四周留白/阴影），缩到
    /// Windows 尺寸后贴不满画布，任务栏与托盘就会显得比邻居小。这里把"内容必须
    /// 铺满画布"钉成断言：阈值取 alpha ≥ 128（抗锯齿边缘也算可见），要求外接框
    /// ≥ 98%。旧资产实测 0.90–0.94，会失败；新资产实测 1.000。
    #[test]
    fn faceted_duck_artwork_fills_the_canvas() {
        fn visible_fill_ratio(image: &Image<'_>) -> f64 {
            let (width, height) = (image.width() as i64, image.height() as i64);
            let rgba = image.rgba();
            let (mut min_x, mut min_y, mut max_x, mut max_y) = (width, height, -1_i64, -1_i64);
            for (index, pixel) in rgba.chunks_exact(4).enumerate() {
                if pixel[3] < 128 {
                    continue;
                }
                let x = (index as i64) % width;
                let y = (index as i64) / width;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
            if max_x < min_x || max_y < min_y {
                return 0.0;
            }
            let extent = (max_x - min_x + 1).max(max_y - min_y + 1) as f64;
            extent / width.max(height) as f64
        }

        let mut checked = Vec::new();
        for size in TRAY_ICON_SIZES {
            let image = faceted_duck_tray_image(size).expect("托盘图标必须可解码");
            checked.push((format!("{size}px"), visible_fill_ratio(&image)));
        }
        let window_icon = faceted_duck_window_image().expect("窗口图标必须可解码");
        checked.push(("256px".to_owned(), visible_fill_ratio(&window_icon)));

        for (label, ratio) in checked {
            assert!(
                ratio >= 0.98,
                "几何鸭 {label} 图标内容只占画布 {ratio:.3}：源图带了 macOS 式留白，\
                 在任务栏/托盘会比别的应用小一圈（见 scripts/generate-app-icons.py 的 FILL_RATIO_MINIMUM）"
            );
        }
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

    /// 回归测试（2026-10-02 用户现场："任务栏没有跟随切换图标"）：
    ///
    /// - 只写 `ICON_SMALL`（= Tauri/tao 的 `set_window_icon` 路径）时，任务栏用的
    ///   `ICON_BIG` 保持不变——这是根因，不是浏览器缓存或 Explorer 刷新问题；
    /// - 我们的修复同时写 `ICON_BIG` + `ICON_SMALL`，读回 `ICON_BIG` 必须等于刚设的
    ///   句柄，且再次切换会换成新句柄（旧句柄在替换后才释放）。
    #[cfg(windows)]
    #[test]
    fn taskbar_icon_follows_only_when_icon_big_is_written() {
        use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, ICON_BIG, ICON_SMALL};

        let hwnd = hidden_test_window();
        let solid = |size: u32, rgb: [u8; 3]| {
            let mut rgba = Vec::with_capacity((size * size * 4) as usize);
            for _ in 0..(size * size) {
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
            window_icons::IconBitmap {
                rgba,
                width: size,
                height: size,
            }
        };
        let raw = hwnd.0 as isize;

        // 1) 复现根因：只写 ICON_SMALL。
        let small_only =
            window_icons::create_icon(&solid(16, [0, 0, 255])).expect("小图标创建失败");
        window_icons::send_icon(hwnd, ICON_SMALL, &small_only);
        assert_ne!(
            window_icons::read_icon(hwnd, ICON_SMALL),
            0,
            "ICON_SMALL 应当已写入"
        );
        assert_eq!(
            window_icons::read_icon(hwnd, ICON_BIG),
            0,
            "只设 ICON_SMALL 时任务栏图标（ICON_BIG）不该变化——正是用户看到的症状"
        );

        // 2) 修复路径：ICON_BIG + ICON_SMALL 一起写。
        let big = solid(32, [255, 0, 0]);
        let small = solid(16, [0, 255, 0]);
        window_icons::apply(raw, &big, &small).expect("应用窗口图标失败");
        let applied_big = window_icons::read_icon(hwnd, ICON_BIG);
        assert_ne!(applied_big, 0, "ICON_BIG 必须写入，任务栏才会跟随");
        assert_ne!(
            applied_big,
            small_only.as_raw(),
            "任务栏图标不能还是小图标句柄"
        );
        assert_ne!(
            window_icons::read_icon(hwnd, ICON_SMALL),
            0,
            "ICON_SMALL 应保持已设置"
        );

        // 3) 再次切换：任务栏图标换成新句柄（证明每次切换都会更新）。
        let big_again = solid(32, [255, 255, 0]);
        window_icons::apply(raw, &big_again, &small).expect("二次应用窗口图标失败");
        let reapplied_big = window_icons::read_icon(hwnd, ICON_BIG);
        assert_ne!(reapplied_big, 0);
        assert_ne!(
            reapplied_big, applied_big,
            "再次切换后任务栏图标必须换新句柄"
        );

        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    #[cfg(windows)]
    #[test]
    fn icon_bitmap_validation_rejects_size_mismatch() {
        // 长度不足的缓冲必须在触碰任何系统 API 之前被拒。
        let bitmap = window_icons::IconBitmap {
            rgba: vec![0; 3],
            width: 4,
            height: 4,
        };
        assert!(window_icons::create_icon(&bitmap).is_err());

        let empty = window_icons::IconBitmap {
            rgba: Vec::new(),
            width: 0,
            height: 0,
        };
        assert!(window_icons::create_icon(&empty).is_err());
    }

    /// 隐藏的顶层窗口：只用于读回 `WM_GETICON`，从不 ShowWindow。
    #[cfg(windows)]
    fn hidden_test_window() -> windows::Win32::Foundation::HWND {
        use windows::core::w;
        use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, RegisterClassExW, WNDCLASSEXW, WS_EX_TOOLWINDOW,
            WS_POPUP,
        };

        unsafe extern "system" fn test_wnd_proc(
            hwnd: windows::Win32::Foundation::HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }

        unsafe {
            let class = WNDCLASSEXW {
                cbSize: u32::try_from(std::mem::size_of::<WNDCLASSEXW>()).unwrap_or_default(),
                lpfnWndProc: Some(test_wnd_proc),
                lpszClassName: w!("SayAllAppIconTestWindow"),
                ..Default::default()
            };
            // 已注册时重复注册会失败，可忽略。
            let _ = RegisterClassExW(&class);
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                w!("SayAllAppIconTestWindow"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
            .expect("测试窗口创建失败")
        }
    }
}
