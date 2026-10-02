"""从 Mac 仓库的 StatusIcon 模板图派生 Windows 托盘状态图标。

来源（只读，不修改）：`<mac-repo>/Resources/StatusIconTemplate@2x.png`
（Mac 菜单栏模板图：遥控器 + 语音波，黑色 + alpha 蒙版，由系统按明暗自动着色）。
Mac main 的 `StatusIconActiveTemplate(@2x).png` 与该文件逐字节相同（2026-10-02 用
SHA-256 核对），因此这里也只有一个形状；连接/断开的状态差异在 Mac 侧由
`appearsDisabled` 表达，Windows 侧对应"运行时按 alpha 变暗"。

Windows 托盘图标没有"模板图"机制，因此这里把蒙版按两种图标颜色和四种尺寸
（16/20/24/32，对应 100%–200% 缩放）预生成 PNG，Rust 侧按系统托盘明暗与 DPI
选择；"未连接变暗"在运行时按 alpha 缩放实现，不额外生成资产。

用法（需要 Pillow）：

    uv run --with pillow python scripts/generate-tray-icons.py \
        --source "D:/SayAll/src/GetSayAll/remote-mic-app/Resources"

不传 --source 时使用上面的默认路径。输出目录：`src-tauri/icons/tray/`，
并同步一张 32px 预览到 `public/status-icon-<color>.png` 供设置页图标选择控件使用。
"""

from __future__ import annotations

import argparse
import pathlib
import sys

from PIL import Image, ImageChops

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_SOURCE = pathlib.Path("D:/SayAll/src/GetSayAll/remote-mic-app/Resources")
ICON_DIRECTORY = REPOSITORY_ROOT / "src-tauri" / "icons" / "tray"
PUBLIC_DIRECTORY = REPOSITORY_ROOT / "public"
SIZES = (16, 20, 24, 32)
STATES = {"idle": "StatusIconTemplate@2x.png"}
COLORS = {"black": (0, 0, 0), "white": (255, 255, 255)}


def load_mask(path: pathlib.Path) -> Image.Image:
    """把模板图归一为单通道蒙版：alpha × (1 - 亮度)，保留抗锯齿边缘。"""
    with Image.open(path) as image:
        rgba = image.convert("RGBA")
        alpha = rgba.getchannel("A")
        luminance = rgba.convert("L")
        return ImageChops.multiply(alpha, ImageChops.invert(luminance))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=pathlib.Path, default=DEFAULT_SOURCE)
    arguments = parser.parse_args()

    source_directory: pathlib.Path = arguments.source
    for name in (*STATES.values(),):
        path = source_directory / name
        if not path.is_file():
            print(f"缺少 Mac 状态图标源文件：{path}", file=sys.stderr)
            return 1

    ICON_DIRECTORY.mkdir(parents=True, exist_ok=True)
    written: list[pathlib.Path] = []
    for state, filename in STATES.items():
        mask = load_mask(source_directory / filename)
        for color_name, rgb in COLORS.items():
            for size in SIZES:
                resized = mask.resize((size, size), Image.LANCZOS)
                icon = Image.new("RGBA", (size, size), rgb + (0,))
                icon.putalpha(resized)
                target = ICON_DIRECTORY / f"tray-status-{state}-{color_name}-{size}.png"
                icon.save(target, optimize=True)
                written.append(target)
                if size == 32 and state == "idle":
                    preview = PUBLIC_DIRECTORY / f"status-icon-{color_name}.png"
                    icon.save(preview, optimize=True)
                    written.append(preview)

    for path in written:
        print(path.relative_to(REPOSITORY_ROOT).as_posix())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
