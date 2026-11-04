//! 回眸应用图标探针：把当前代码真正产出的 64×64 PNG 落盘，肉眼核对颜色与透明度。
//!
//! 用法：`cargo run -p sayall-windows --example icon_probe`
//!
//! 只按显示名扫描正在运行的进程（与回眸运行时同一条路径），所以目标应用要先开着。
//! 输出目录是 `<临时目录>/sayall-icon-probe`。判据：正常情况下图标是彩色的，
//! 且存在 alpha=0 的像素；整图只有黑/白两色说明 `LR_MONOCHROME` 又被传进来了。

fn main() {
    let out = std::env::temp_dir().join("sayall-icon-probe");
    if let Err(error) = std::fs::create_dir_all(&out) {
        eprintln!("创建输出目录失败：{error}");
        return;
    }
    for name in ["chrome", "Chrome 浏览器", "记事本", "微信", "WorkBuddy"] {
        match sayall_windows::transcript_capture_windows::application_icon_for_display_name(name) {
            Some(png) => {
                let file = out.join(format!("{}.png", name.replace(' ', "_")));
                match std::fs::write(&file, &png) {
                    Ok(()) => println!("{name}: {} bytes -> {}", png.len(), file.display()),
                    Err(error) => println!("{name}: 写盘失败 {error}"),
                }
            }
            None => println!("{name}: 未找到（应用没在运行，或名称不匹配）"),
        }
    }
}
