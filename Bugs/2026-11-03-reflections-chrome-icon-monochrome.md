# 回眸：Chrome 图标变成黑白块（应用图标一律单色）

- 发现日期：2026-11-03
- 状态：已修复（提取函数、缓存迁移、应用名均改；本地测试包安装版页面复核 passed）
- 影响范围：Windows 回眸「仅录音」与文字记录的应用图标；所有以经典位图（非 PNG 帧）
  存放 64px 图标的 exe，实测 Chrome 156.0.8078.74、系统记事本、cmd
- 功能点：`sayall-windows::transcript_capture_windows`（图标提取）、
  `src-tauri::transcripts`（图标缓存）、`src/pages/reflections-view.ts`（应用名显示）

## 现象

回眸页 Chrome 的应用行显示一张几乎全白的方块，不是 Chrome 彩色圆环；
其他应用（微信、WorkBuddy、DimAgent）图标正常。

## 复现条件

1. 开启「记录回眸」。
2. 在 Chrome 中完成一次语音（文字捕获失败、只有录音也算）。
3. 回眸页按应用筛选，看 Chrome 行。

## 正常预期

显示 64×64 的 Chrome 彩色圆环，透明背景，与资源管理器里的图标一致。

## 证据

- 图标缓存 `%LOCALAPPDATA%\SayAll\transcripts\v1\icons\chrome-da76bff1b19fb376.png`：
  64×64，整图只有 2 种颜色（纯黑 2437 像素、纯白 1659 像素），alpha 恒为 255。
- 同一份代码对同一路径的提取结果与缓存一致（650 字节），说明不是陈旧缓存，
  而是提取本身产出单色图。
- 用 `PrivateExtractIconsW` 按尺寸扫描 `chrome.exe`（64px 请求）：

  | flags | 结果 |
  | --- | --- |
  | 1（当前代码） | 2 种颜色，alpha 全 0 |
  | 0 | 1511 种颜色，alpha 0..254，画面即正常 Chrome logo |

  同一组参数下 `notepad.exe` 从 2 色变成 2325 色，`cmd.exe` 从 2 色变成 325 色；
  微信、WorkBuddy 因只用 PNG 压缩帧，两种取值都正常，所以此前只暴露出 Chrome。
- `windows` 0.62 crate 的绑定：

  ```rust
  pub unsafe fn PrivateExtractIconsW(szfilename, niconindex, cxicon, cyicon,
      phicon: Option<&mut [HICON]>, piconid, flags: u32) -> u32
  ```

  `nicons` 由 `phicon` 切片长度推出，第 7 个参数是 `flags`。

## 根因

1. **主因**：调用处按原生 Win32 签名传 `(…, phicon, piconid, 1)`，本意是 `nIcons = 1`，
   但在 crate 绑定里这个位置是 `flags`，`1` 等于 `LR_MONOCHROME`。Windows 于是把
   BMP 帧的图标重新渲染成单色位图：内容只剩黑/白，alpha 通道全 0。（2026-11-03 实测。）
2. **放大器**：`icon_to_png` 在「整张图标没有 alpha」时把所有像素 alpha 补成 255，
   于是一张 alpha 全 0 的单色图变成不透明黑白块，被写进图标缓存。
3. **修复不可达**：`write_icon` 命中同名文件就跳过、`attach_icons` 优先读缓存，
   图标永不复检。只改提取逻辑，已安装用户仍会一直看到旧图。

## 修复

- `transcript_capture_windows.rs`：
  - `PrivateExtractIconsW` 第 7 个参数改为 `LR_DEFAULTCOLOR`（0），并注明 `nicons`
    由切片长度决定；尺寸提取为 `APPLICATION_ICON_PIXELS`。
  - 无 alpha 时改用 AND 掩码合成透明度（`mask_alpha_bits` + `compose_mask_alpha`），
    取不到掩码才退回整块不透明，避免「无 alpha 图标一律变不透明色块」。
  - 应用显示名改走 `foreground::application_display_name`（产品名），稳定键仍用原始
    文件名去扩展名，已有记录不会被拆成新应用。
  - 新增 `icon_png_is_usable`：判断缓存 PNG 还值不值得用（2 色且全不透明 = 坏图）。
- `transcripts.rs`：图标缓存文件名带 `ICON_CACHE_VERSION`（当前 `v2`）。读缓存时按
  `v2 → 旧 {key}.png（仅当可用）→ 现场提取` 回退：坏图会被重新提取；应用没在运行时
  仍沿用旧图（否则升级后所有没开着的应用会整列退回通用图标，实测 14 条记事本记录
  就是这种情况）。
- `reflections-view.ts`：`chrome` / `notepad` 等历史原始名统一显示为产品名
  （`Chrome 浏览器` / `记事本`），英文界面用 `Chrome` / `Notepad`。

## 验证

- `cargo fmt --all -- --check` passed。
- `cargo test --workspace` passed（sayall-windows 497 passed / 16 ignored、
  sayall-windows-app 185 passed / 1 ignored）；新增：
  - `extracted_icons_keep_their_color_and_alpha`：解码 64px 图标，要求颜色数 > 16
    且存在透明像素。正向对照：把 `flags` 改回 `1` 时该断言失败（实测「3 种颜色」）。
  - `composes_alpha_from_a_one_bit_mask`、`mask_alpha_tolerates_short_rows`。
  - `cached_icons_are_reused_only_when_they_can_still_be_real_icons`。
  - `icon_cache_ignores_files_written_by_older_versions`：坏旧缓存被忽略。
    正向对照：把可用性判定强制成 `true` 时该用例失败。
  - `usable_legacy_icon_is_reused_and_migrated`：可用旧缓存被沿用并写成 `v2`。
- `npx vitest run`（564 passed / 10 skipped）、`vue-tsc --noEmit` passed；
  新增历史应用名映射用例。
- 提取结果复核：Chrome 由 650 字节 / 2 色变成 6846 字节 / 1511 色，画面为正常彩色
  圆环；微信 2770 字节、WorkBuddy 5320 字节与修复前逐字节一致（无回归）；
  无 alpha 的 `imageres.dll` 掩码路径可正常出图且不崩溃。
- 本地测试包（2026-11-04）：`target/release/bundle/nsis/无线麦 SayAll_1.8.3_x64-setup.exe`，
  SHA-256 `af175313b799ef80e19191bced8fc41359912dbe862ba28128a1a30d67b84f21`，
  内嵌修订号 `ef79e05d`；`.sig` 用本机测试密钥签名，**不可发布**。
  - `verify-frontend-embedded.ps1`：18/18 前端资源全部嵌入（阳性对照：换旧 dist 目录
    后报「缺失 9/15」）。
  - `verify-local-test-install.ps1 -ExpectRevision`：helper / gadget 哈希对齐、
    app exe 差 3 字节（UNK→NSS）、修订号命中，**PASS**；旧实例走
    `key_host action=exit reason=shutdown_requested` 正常退出，未强杀。
  - 安装版**页面 passed**：启动后停在回眸页，应用栏 chrome 芯片显示彩色 Chrome 圆环，
    截图 `artifacts/ui-selftest/reflections-apps-wide-20261104.png`。
  - 图标缓存 passed：`chrome-da76bff1b19fb376.v2.png` 6846 B（旧 650 B / 2 色）；
    `Notepad-c73816817bc021f7.v2.png` 4043 B 由旧缓存迁移 —— 应用没在运行时图标不会丢。
- 验证环境坑（供下次复用）：从沙箱 shell 启动安装版时，WebView2 浏览器进程会随任务树
  结束而退出，应用把标题改成「界面未能正常显示，请从托盘退出后重开」并显示空白页；
  这**不是包的问题**，用 `explorer.exe <exe>` 启动即可正常渲染。另外 `PrintWindow`
  抓不到 WebView2 内容（返回空白），要判定界面必须用 `CopyFromScreen`。

## 隐私检查

日志与文档未写入真实蓝牙地址、HID 路径、语音内容或个人完整路径；图标来自公开 Shell
提取，只缓存应用图标 PNG。
