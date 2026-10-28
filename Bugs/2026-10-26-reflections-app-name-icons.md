# 回眸：微信显示 Weixin、微信/WorkBuddy 图标不对

- 发现日期：2026-10-26
- 状态：已修复（本机安装版验证通过；新录音 manifest 的真机复核 deferred）
- 影响范围：Windows 回眸「仅录音」条目；微信 4.x（`Weixin.exe`）与 WorkBuddy 5.8.3
- 功能点：`sayall-windows::transcript_capture_windows`、`src-tauri::transcripts::attach_icons`、`ReflectionsPage.vue`

## 现象

- 微信录音在回眸页显示为 `Weixin`，不是「微信」。
- 微信和 WorkBuddy 的「仅录音」行显示通用文档图标，不是各自应用图标。

## 复现条件

1. 开启「记录回眸」与「保存原始录音」。
2. 在微信或 WorkBuddy 中完成一次语音，文字捕获失败但录音成功。
3. 回到回眸页查看「仅录音」条目。

## 证据

- 新录音 manifest：`applicationKey=Weixin-7e04e5c6931ccef3`、`applicationName=Weixin`；WorkBuddy 的 `applicationName=WorkBuddy`。
- 记录开始时日志显示文字基线 `unsupported_control_type`，但 `transcript_recording action=finish ... result=passed`。
- 图标缓存目录在修复前只有 DimAgent、Notepad；微信/WorkBuddy 没有缓存，前端只能显示通用图标。
- 微信 exe 的版本资源 `ProductName`/`FileDescription` 都是 `Weixin`，不是中文；WorkBuddy exe 的名称正确。

## 根因

1. `application_identity_parts` 直接使用 exe 的版本资源显示名；`foreground::application_display_name` 虽然已有 `Weixin.exe → 微信` 映射，但没有用于回眸身份。
2. 图标缓存只在文字捕获成功并保存记录时由 `write_icon` 写入。微信/WorkBuddy 的文字捕获失败，因此没有缓存；`attach_icons` 只读缓存，找不到就回退到通用图标。

## 修复

- `transcript_capture_windows.rs`：新增 `localized_application_name`，把 `Weixin` / `WeChat` 显示为「微信」；`application_key` 继续使用原始显示名，避免已有 `Weixin-*` / `WeChat-*` 记录被拆成新应用。
- `ReflectionsPage.vue` / `reflections-view.ts`：旧 manifest 的 `Weixin` / `WeChat` 名称在界面统一显示为「微信」；英文界面显示 `WeChat`。
- `attach_icons`：缓存缺失时，按应用显示名只读扫描当前进程（`application_icon_for_display_name`），从公开 Shell 图标提取 64×64 PNG，写入现有图标缓存并返回 data URL。微信/WorkBuddy 的图标来自各自进程可执行文件，不读取私有数据。
- 新增前端/后端回归：微信中英文名称、旧名称别名、图标提取函数（notepad）与名称本地化。

## 验证

- 自动化：前端 557 passed / 10 skipped；`sayall-windows` 470 passed / 16 ignored；`sayall-windows-app` 177 passed / 1 ignored；`vue-tsc --noEmit`、`pnpm build`、`cargo fmt --all -- --check` passed。
- 安装版：测试包 SHA-256 `041ec05f9df555f72e72d7a8c242c9eb48b1814bd7713df3b36069470dd296ee`，内嵌修订号 `0fad4d99`；`verify-local-test-install.ps1 -ExpectRevision` 全部 PASS。注意该修订号是外部切分支时自动生成的 checkpoint 提交，包含本次修复。
- 页面：修复后图标缓存出现 `Weixin-7e04e5c6931ccef3.png` 与 `WorkBuddy-8e9eab770187d406.png`；筛选「微信」显示中文名与绿色微信图标，筛选 `WorkBuddy` 显示绿色 WorkBuddy 图标。
- 截图：`artifacts/ui-selftest/reflections-wechat-filtered.png`、`artifacts/ui-selftest/reflections-workbuddy-filtered.png`。
- **deferred**：用 RC001/RC003 在微信/WorkBuddy 各完成一次新语音，确认新 manifest 的 `applicationName` 直接写「微信」；旧 manifest 的 `Weixin` 名称不会改写，只做界面别名。

## 隐私检查

日志与文档未写入真实蓝牙地址、HID 路径、语音内容或个人完整路径；图标缓存只保存公开的应用图标 PNG。
