"""从仓库内的源图派生 Windows 用的应用图标资产。

来源（只读，不修改）：`src-tauri/icons/app-icons/faceted-duck-source.png`。
此前这份源图来自 Mac 仓库 `Resources/AppIcons/faceted-duck.png`，那份导出沿用
macOS 图标网格的留白（图案只占画布 ~87%），在 Windows 上任务栏与托盘的图标就
会比邻居"小一圈"（2026-10-03 现场反馈）。现在改用满画布的 Windows 版导出
（图案占比 ≥ 95%，脚本用 `FILL_RATIO_MINIMUM` 把这条约定卡住），设计本身不变。

Windows 侧要三处用图：
- `src-tauri/icons/app-icons/faceted-duck-{16,20,24,32}.png`：通知区域（托盘）
  按 DPI 选档；
- `src-tauri/icons/app-icons/faceted-duck-256.png`：运行时 `window.set_icon`
  换窗口/任务栏图标；
- `public/app-icon-faceted-duck.png`：设置页选项预览与顶部应用标识。

用法（需要 Pillow）：

    uv run --with pillow python scripts/generate-app-icons.py

也可以 `--source` 指向别处的源图（文件路径，或含 `faceted-duck.png` 的目录）。
脚本不联网、不修改 Mac 仓库。
"""

from __future__ import annotations

import argparse
import pathlib
import sys

from PIL import Image

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_SOURCE = REPOSITORY_ROOT / "src-tauri" / "icons" / "app-icons" / "faceted-duck-source.png"
ICON_DIRECTORY = REPOSITORY_ROOT / "src-tauri" / "icons" / "app-icons"
PUBLIC_DIRECTORY = REPOSITORY_ROOT / "public"
TRAY_SIZES = (16, 20, 24, 32)
WINDOW_SIZE = 256
PREVIEW_SIZE = 256
# Windows 图标要贴满画布；低于这个比例说明源图带着 macOS 式留白/阴影边距。
FILL_RATIO_MINIMUM = 0.95
SOLID_ALPHA = 250


def solid_fill_ratio(image: Image.Image) -> float:
    """实心内容（alpha ≥ `SOLID_ALPHA`）外接框的最大边长 / 画布边长。"""
    rgba = image.convert("RGBA")
    mask = rgba.getchannel("A").point(lambda value: 255 if value >= SOLID_ALPHA else 0)
    bounding_box = mask.getbbox()
    if bounding_box is None:
        return 0.0
    extent = max(bounding_box[2] - bounding_box[0], bounding_box[3] - bounding_box[1])
    return extent / max(rgba.size)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=pathlib.Path,
        default=DEFAULT_SOURCE,
        help="faceted-duck 源图（文件路径，或包含 faceted-duck.png 的目录）",
    )
    arguments = parser.parse_args()

    source_path = arguments.source
    if source_path.is_dir():
        source_path = source_path / "faceted-duck.png"
    if not source_path.is_file():
        print(f"缺少应用图标源文件：{source_path}", file=sys.stderr)
        return 1

    with Image.open(source_path) as image:
        source = image.convert("RGBA")

    fill_ratio = solid_fill_ratio(source)
    if fill_ratio < FILL_RATIO_MINIMUM:
        print(
            f"源图内容只占画布 {fill_ratio:.3f}（要求 ≥ {FILL_RATIO_MINIMUM}）："
            "带留白的导出在 Windows 任务栏/托盘会比别的应用小一圈，"
            f"请换成满画布导出（与 {DEFAULT_SOURCE.name} 同款）。",
            file=sys.stderr,
        )
        return 1

    ICON_DIRECTORY.mkdir(parents=True, exist_ok=True)
    written: list[pathlib.Path] = []
    for size in TRAY_SIZES:
        target = ICON_DIRECTORY / f"faceted-duck-{size}.png"
        source.resize((size, size), Image.LANCZOS).save(target, optimize=True)
        written.append(target)

    window_icon = ICON_DIRECTORY / f"faceted-duck-{WINDOW_SIZE}.png"
    source.resize((WINDOW_SIZE, WINDOW_SIZE), Image.LANCZOS).save(window_icon, optimize=True)
    written.append(window_icon)

    preview = PUBLIC_DIRECTORY / "app-icon-faceted-duck.png"
    source.resize((PREVIEW_SIZE, PREVIEW_SIZE), Image.LANCZOS).save(preview, optimize=True)
    written.append(preview)

    for path in written:
        print(path.relative_to(REPOSITORY_ROOT).as_posix())
    print(f"source={source_path.name} fill_ratio={fill_ratio:.3f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
