# 应用图标在任务栏与托盘比邻居小一圈

- 发现日期：2026-10-03
- 状态：已修复（资产级验证 + 自动化解 + 真机量测 passed；安装包内目视确认 deferred）
- 影响范围：设置页改版后的本地验证包（0.5.0 起）的「几何鸭」（`faceted-duck`）图标；用户现场 Windows 11、150% 缩放；与 RC001/RC003、第三方工具无关
- 功能点：应用图标资产（`src-tauri/icons/app-icons/faceted-duck-*.png`、`public/app-icon-faceted-duck.png`）→ 托盘 + 任务栏 / Alt-Tab
- 现象：切到「几何鸭」后，任务栏按钮与通知区域托盘的图标都比同排其它应用"小一圈"（深色瓷贴外还有一圈留白）
- 复现条件：
  1. 安装本地验证包并启动，设置页把「应用图标」切到「几何鸭」；
  2. 与任务栏同排应用（微信、Chrome 等）比较按钮图标大小；托盘区同理。
- 正常预期：Windows 应用图标贴满画布，瓷贴边长与邻居一致（任务栏 150% 缩放下为 36px 槽位、托盘 24px）
- 证据：
  - 用户截图量测（150% 缩放）：任务栏我方瓷贴 ≈34px vs 邻居满格 36px（0.944）；托盘我方 22px vs 邻居 24px（0.917）。
  - 资产量测（深色瓷贴外接框 / 画布）：旧资产 20px→18px(0.900)、24px→22px(0.917)、32px→30px(0.938)、256px→240px(0.938)；新资产 全部 1.000（16px 一档旧资产因柔和阴影被判为满格，该档不具区分度，其余各档与用户截图比例吻合）。
  - 阳性对照：用旧的 256px 资产当源跑生成器 → `源图内容只占画布 0.934（要求 ≥ 0.95）` 退出码 1；新回归用例在旧资产下 **FAILED**、新资产下 passed。
- 根因（已确认）：源图是 macOS 图标网格的导出（图案四周带留白与柔和投影，实心内容只占画布 ~87–94%）。Windows 图标惯例是贴满画布，同一份图缩到 16/20/24/32/256 后瓷贴自然比邻居小一圈——与窗口图标注入路径（`ICON_BIG`）无关，那条链路 2026-10-02 已修好并保持有效。
- 「默认」（standard）图标：同一批 macOS 导出（16/20/24/32 档实心 0.812–0.832），2026-10-03 追加修复——新增 `scripts/generate-standard-icon.py`（探测瓷贴边界 → 裁掉外部阴影 → 拉伸铺满 → 派生全部 PNG 与 `icon.ico`，逐输出卡 0.95 填充比），覆盖窗口/托盘（`bundle.icon` 列表）、安装器图标与快捷方式图标（`shortcut_icons.rs` 内嵌 `icon.ico`，字节不一致才重写）。
- 修复（最小改动）：
  - 新源图落库：`src-tauri/icons/app-icons/faceted-duck-source.png`（用户 2026-10-03 提供的满画布导出，实心占比 0.995；设计不变，只换导出）。
  - `scripts/generate-app-icons.py`：默认源改为仓库内源图（`--source` 仍可指向文件或目录），新增 `FILL_RATIO_MINIMUM = 0.95` 守卫，源图留白过多直接拒绝并给出原因；重新生成 16/20/24/32/256 与设置页预览。
  - `src-tauri/src/app_icon.rs`：新增回归用例 `faceted_duck_artwork_fills_the_canvas`（alpha ≥ 128 的可见内容外接框 ≥ 98%，覆盖 4 档托盘 + 256 窗口图）。
- 验证：
  - `uv run --with pillow python scripts/generate-app-icons.py` → 写出 6 份资产，`fill_ratio=0.995`：`passed`
  - 阳性对照（旧源）→ 守卫生效、退出码 1：`passed`
  - `cargo fmt --all -- --check`：`passed`
  - `cargo test -p sayall-windows-app app_icon` → 7 passed（含新旧资产对照的回归用例）：`passed`
  - `cargo test --workspace`：`passed`
  - 真机渲染目视（任务栏 + 托盘与邻居并排）：`deferred` —— 本机装的是正式版包且单实例互斥，无法在真机跑未打包的新资产；下次出包后按 `Testing/WindowsRC003Preview.md` 用例十四一并确认。
- 隐私检查：用户截图只在本地做像素量测、未入库；记录仅保留比例与像素数，无个人路径、设备身份、语音内容或凭据。
