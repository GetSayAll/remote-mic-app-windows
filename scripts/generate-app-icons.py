"""从仓库内的源图派生 Windows 用的应用图标资产。

来源（只读，不修改）：`src-tauri/icons/app-icons/faceted-duck-source.png`。
此前这份源图来自 Mac 仓库 `Resources/AppIcons/faceted-duck.png`，那份导出沿用
macOS 图标网格的留白（图案只占画布 ~87%），在 Windows 上任务栏与托盘的图标就
会比邻居"小一圈"（2026-10-03 现场反馈）。现在改用满画布的 Windows 版导出
（图案占比 ≥ 95%，脚本用 `FILL_RATIO_MINIMUM` 把这条约定卡住），设计本身不变。

Windows 侧要四处用图（2026-10-04 起几何鸭同时是应用自身的图标）：
- `src-tauri/icons/app-icons/faceted-duck-{16,20,24,32}.png`：通知区域（托盘）
  按 DPI 选档；
- `src-tauri/icons/app-icons/faceted-duck-256.png`：运行时 `window.set_icon`
  换窗口/任务栏图标，同时是 `bundle.icon` 的 PNG 条目；
- `src-tauri/icons/app-icons/faceted-duck.ico`（16/20/24/32/48/256，最大档在前）：
  `bundle.icon` 的 .ico 条目——`tauri-build` 用它写 exe 图标资源、`tauri-codegen`
  取 `entries()[0]` 当窗口默认图标，NSIS 的安装器/卸载器图标同指它；
- `public/app-icon-faceted-duck.png`：设置页选项预览与顶部应用标识。

用法（需要 Pillow）：

    uv run --with pillow python scripts/generate-app-icons.py

也可以 `--source` 指向别处的源图（文件路径，或含 `faceted-duck.png` 的目录）。
脚本不联网、不修改 Mac 仓库。
"""

from __future__ import annotations

import argparse
import pathlib
import struct
import sys

from PIL import Image

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_SOURCE = REPOSITORY_ROOT / "src-tauri" / "icons" / "app-icons" / "faceted-duck-source.png"
ICON_DIRECTORY = REPOSITORY_ROOT / "src-tauri" / "icons" / "app-icons"
PUBLIC_DIRECTORY = REPOSITORY_ROOT / "public"
TRAY_SIZES = (16, 20, 24, 32)
WINDOW_SIZE = 256
PREVIEW_SIZE = 256
ICO_NAME = "faceted-duck.ico"
ICO_SIZES = ((16, 16), (20, 20), (24, 24), (32, 32), (48, 48), (256, 256))
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


def visible_fill_ratio(image: Image.Image) -> float:
    """可见内容（alpha ≥ 128，抗锯齿边缘也算）外接框的最大边长 / 画布边长。

    小尺寸（16–32px）缩放的边缘像素会落到 `SOLID_ALPHA` 以下，源图检查用实心口径、
    输出检查用可见口径——与 Rust 侧 `app_icon` 的 0.98 断言同一口径。
    """
    rgba = image.convert("RGBA")
    mask = rgba.getchannel("A").point(lambda value: 255 if value >= 128 else 0)
    bounding_box = mask.getbbox()
    if bounding_box is None:
        return 0.0
    extent = max(bounding_box[2] - bounding_box[0], bounding_box[3] - bounding_box[1])
    return extent / max(rgba.size)


def reorder_ico_largest_first(path: pathlib.Path) -> list[tuple[int, int]]:
    """把 .ico 的条目表改成"大图在前"，并返回新顺序。

    `tauri-codegen`（`image.rs::CachedIcon::new_ico`）只看 `entries()[0]`；它解码成
    RGBA 当作窗口默认图标——不排好序就会把 16px 那档当成默认图（32/36px 槽位被
    放大 → 糊）。PIL 的 ICO writer 固定从小到大写，所以这里重排条目表；数据块
    不动，偏移量各自有效。
    """
    data = bytearray(path.read_bytes())
    count = struct.unpack_from("<H", data, 4)[0]
    entries = [bytes(data[6 + index * 16 : 6 + (index + 1) * 16]) for index in range(count)]

    def area(entry: bytes) -> int:
        width = entry[0] or 256
        height = entry[1] or 256
        return width * height

    ordered = sorted(entries, key=area, reverse=True)
    for index, entry in enumerate(ordered):
        data[6 + index * 16 : 6 + (index + 1) * 16] = entry
    path.write_bytes(bytes(data))
    return [(entry[0] or 256, entry[1] or 256) for entry in ordered]


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
    failures: list[str] = []
    for size in TRAY_SIZES:
        target = ICON_DIRECTORY / f"faceted-duck-{size}.png"
        scaled = source.resize((size, size), Image.LANCZOS)
        scaled.save(target, optimize=True)
        written.append(target)
        ratio = visible_fill_ratio(scaled)
        if ratio < FILL_RATIO_MINIMUM:
            failures.append(f"faceted-duck-{size}.png 填充比 {ratio:.3f} < {FILL_RATIO_MINIMUM}")

    window_icon = ICON_DIRECTORY / f"faceted-duck-{WINDOW_SIZE}.png"
    scaled = source.resize((WINDOW_SIZE, WINDOW_SIZE), Image.LANCZOS)
    scaled.save(window_icon, optimize=True)
    written.append(window_icon)
    ratio = visible_fill_ratio(scaled)
    if ratio < FILL_RATIO_MINIMUM:
        failures.append(f"faceted-duck-{WINDOW_SIZE}.png 填充比 {ratio:.3f} < {FILL_RATIO_MINIMUM}")

    preview = PUBLIC_DIRECTORY / "app-icon-faceted-duck.png"
    scaled = source.resize((PREVIEW_SIZE, PREVIEW_SIZE), Image.LANCZOS)
    scaled.save(preview, optimize=True)
    written.append(preview)

    ico_path = ICON_DIRECTORY / ICO_NAME
    source.save(ico_path, format="ICO", sizes=list(ICO_SIZES))
    written.append(ico_path)
    order = reorder_ico_largest_first(ico_path)
    if order[0] != (WINDOW_SIZE, WINDOW_SIZE):
        failures.append(f"{ICO_NAME} 首条目是 {order[0]}，应为 {WINDOW_SIZE}px（tauri-codegen 只看 entries()[0]）")
    with Image.open(ico_path) as ico:
        for size in sorted(ico.ico.sizes()):
            frame = ico.ico.getimage(size).convert("RGBA")
            ratio = visible_fill_ratio(frame)
            if ratio < FILL_RATIO_MINIMUM:
                failures.append(f"{ICO_NAME} {size[0]}px 填充比 {ratio:.3f} < {FILL_RATIO_MINIMUM}")

    for path in written:
        print(path.relative_to(REPOSITORY_ROOT).as_posix())
    print(f"source={source_path.name} fill_ratio={fill_ratio:.3f} ico_order={order}")

    if failures:
        for failure in failures:
            print(f"填充比不达标：{failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
