"""从 `src-tauri/icons/icon-source.png` 派生 Windows 用的「默认」应用图标资产。

背景（2026-10-03 现场反馈）：几何鸭图标在任务栏/托盘比邻居小一圈，根因是源图沿用
macOS 图标网格的导出（图案四周留白 + 柔和投影，实心瓷贴只占画布 ~83%）。「默认」
（standard）图标是同一批导出，同样偏小；它的设计图（`icon-source.png`）所有装饰
都在瓷贴内，因此这里做**机制性**满画布处理：探测瓷贴边界 → 裁掉外部阴影 → 拉伸
铺满画布，再派生全部尺寸（含 .ico）。

判据（脚本自己卡住）：每个输出（含 .ico 各档）的可见内容外接框 ≥ 画布 95%。

用法（需要 Pillow）：

    uv run --with pillow python scripts/generate-standard-icon.py

输出（覆盖同名文件）：
- `src-tauri/icons/{32x32,64x64,128x128,128x128@2x,icon}.png`
- `src-tauri/icons/Square*Logo.png`、`src-tauri/icons/StoreLogo.png`
- `src-tauri/icons/icon.ico`（16/24/32/48/64/256，快捷方式与安装器同源）
"""

from __future__ import annotations

import pathlib
import sys

from PIL import Image

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
ICON_DIRECTORY = REPOSITORY_ROOT / "src-tauri" / "icons"
SOURCE = ICON_DIRECTORY / "icon-source.png"
MASTER_SIZE = 1024
PNG_TARGETS = {
    "32x32.png": 32,
    "64x64.png": 64,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
    "StoreLogo.png": 50,
}
ICO_NAME = "icon.ico"
ICO_SIZES = ((16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (256, 256))
TILE_ALPHA = 250
VISIBLE_ALPHA = 128
FILL_RATIO_MINIMUM = 0.95


def tile_box(image: Image.Image) -> tuple[int, int, int, int]:
    """探测瓷贴（不透明圆角方块）的外接框：逐行/逐列取 alpha ≥ 250 的行程。"""
    alpha = image.getchannel("A")
    width, height = image.size
    lefts, rights, tops, bottoms = [], [], [], []
    for y in range(height):
        row = [alpha.getpixel((x, y)) for x in range(width)]
        opaque = [x for x, value in enumerate(row) if value >= TILE_ALPHA]
        if opaque:
            lefts.append(opaque[0])
            rights.append(opaque[-1])
    for x in range(width):
        column = [alpha.getpixel((x, y)) for y in range(height)]
        opaque = [y for y, value in enumerate(column) if value >= TILE_ALPHA]
        if opaque:
            tops.append(opaque[0])
            bottoms.append(opaque[-1])
    if not (lefts and tops):
        raise SystemExit("源图没有不透明内容")
    box = (min(lefts), min(tops), max(rights) + 1, max(bottoms) + 1)
    width_ratio = (box[2] - box[0]) / width
    height_ratio = (box[3] - box[1]) / height
    if not (0.6 <= width_ratio <= 1.0 and 0.6 <= height_ratio <= 1.0):
        raise SystemExit(f"探测到的瓷贴范围不合理：{box}（{width_ratio:.3f}×{height_ratio:.3f}）")
    return box


def visible_fill_ratio(image: Image.Image) -> float:
    """可见内容（alpha ≥ 128）外接框最大边长 / 画布边长。"""
    rgba = image.convert("RGBA")
    mask = rgba.getchannel("A").point(lambda value: 255 if value >= VISIBLE_ALPHA else 0)
    box = mask.getbbox()
    if box is None:
        return 0.0
    extent = max(box[2] - box[0], box[3] - box[1])
    return extent / max(rgba.size)


def main() -> int:
    if not SOURCE.is_file():
        print(f"缺少源图：{SOURCE}", file=sys.stderr)
        return 1

    with Image.open(SOURCE) as raw:
        original = raw.convert("RGBA")

    box = tile_box(original)
    cropped = original.crop(box)
    # 瓷贴略非正方（长宽差 ~1.5%），直接拉伸到正方：肉眼不可辨，换来真正的满画布。
    master = cropped.resize((MASTER_SIZE, MASTER_SIZE), Image.LANCZOS)

    written: list[pathlib.Path] = []
    failures: list[str] = []
    for name, size in PNG_TARGETS.items():
        target = ICON_DIRECTORY / name
        image = master.resize((size, size), Image.LANCZOS)
        image.save(target, optimize=True)
        written.append(target)
        ratio = visible_fill_ratio(image)
        if ratio < FILL_RATIO_MINIMUM:
            failures.append(f"{name} 填充比 {ratio:.3f} < {FILL_RATIO_MINIMUM}")

    ico_path = ICON_DIRECTORY / ICO_NAME
    master.save(ico_path, format="ICO", sizes=list(ICO_SIZES))
    written.append(ico_path)
    with Image.open(ico_path) as ico:
        for size in sorted(ico.ico.sizes()):
            frame = ico.ico.getimage(size).convert("RGBA")
            ratio = visible_fill_ratio(frame)
            if ratio < FILL_RATIO_MINIMUM:
                failures.append(f"{ICO_NAME} {size[0]}px 填充比 {ratio:.3f} < {FILL_RATIO_MINIMUM}")

    for path in written:
        print(path.relative_to(REPOSITORY_ROOT).as_posix())
    print(f"source={SOURCE.name} tile_box={box} png_targets={len(PNG_TARGETS)} ico_sizes={len(ICO_SIZES)}")

    if failures:
        for failure in failures:
            print(f"填充比不达标：{failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
