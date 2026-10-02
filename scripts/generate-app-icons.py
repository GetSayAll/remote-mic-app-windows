"""从 Mac 仓库的 AppIcons 源图派生 Windows 用的应用图标资产。

来源（只读，不修改）：`<mac-repo>/Resources/AppIcons/faceted-duck.png`
（Mac main `Sources/RemoteMic/AppIconController.swift` 的 `AppIconCatalog` 把
`Resources/AppIcons/<resourceName>.png` 注册为可切换的应用图标；本脚本只做
缩放与尺寸派生，不改设计）。

Windows 侧要三处用图：
- `src-tauri/icons/app-icons/faceted-duck-{16,20,24,32}.png`：通知区域（托盘）
  按 DPI 选档；
- `src-tauri/icons/app-icons/faceted-duck-256.png`：运行时 `window.set_icon`
  换窗口/任务栏图标；
- `public/app-icon-faceted-duck.png`：设置页选项预览与顶部应用标识。

用法（需要 Pillow）：

    uv run --with pillow python scripts/generate-app-icons.py \
        --source "<mac-repo>/Resources/AppIcons"

不传 --source 时使用默认的兄弟仓库路径。脚本不联网、不修改 Mac 仓库。
"""

from __future__ import annotations

import argparse
import pathlib
import sys

from PIL import Image

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_SOURCE = pathlib.Path("D:/SayAll/src/GetSayAll/remote-mic-app/Resources/AppIcons")
ICON_DIRECTORY = REPOSITORY_ROOT / "src-tauri" / "icons" / "app-icons"
PUBLIC_DIRECTORY = REPOSITORY_ROOT / "public"
TRAY_SIZES = (16, 20, 24, 32)
WINDOW_SIZE = 256
PREVIEW_SIZE = 256


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=pathlib.Path, default=DEFAULT_SOURCE)
    arguments = parser.parse_args()

    source_path: pathlib.Path = arguments.source / "faceted-duck.png"
    if not source_path.is_file():
        print(f"缺少 Mac 应用图标源文件：{source_path}", file=sys.stderr)
        return 1

    with Image.open(source_path) as image:
        source = image.convert("RGBA")

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
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
