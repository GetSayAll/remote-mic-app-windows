# 返回与音量加减：完整开源操作指南

本文对应源码中的 **SayAllThreeButtonFilter 实验功能**。它是可选驱动，不是基础语音功能的依赖。
旧版本 Release 不一定含此功能；必须使用包含本目录与 `three_button_driver.rs` 的源码构建应用。

## 1. 先了解当前完成程度

| 项目 | 当前证据 |
|---|---|
| 报文转换、释放和边界检查 | 7,995,392 组组合通过 |
| Windows x64 驱动编译、INF 检查、CAT 生成 | 通过 |
| RC001 主机的测试签名安装 | 已安装，服务 Running，匹配设备状态 OK |
| 映射平台层 / 前端 / 宿主层自动化 | 分别 97 / 65 / 16 项通过，另有依赖外部环境的跳过项 |
| 独立应用内嵌页面构建 | 已修复遗漏 `custom-protocol` 导致的 localhost 连接失败，构建通过 |
| 修复后的实际窗口与三个键的最终动作 | 待实机确认，不能以构建或服务 Running 代替 |
| RC001 闲置首按、语音回归、断连、卸载回滚 | 待独立记录 |
| RC003 | 尚未进行本驱动的独立实机验收 |

这是实验代码公开，不是微软生产签名驱动发布。不要将参考项目的 RC003 结果当成本项目 RC001 的验证。

## 2. 工作方式和限制

部分遥控器的三个键在 Windows 普通键盘事件层不可见。驱动位于该设备的 HID 键盘转换器下方，
在读取完成时将特定键盘 Usage 转成 Windows 可以识别的 F13–F15。

| 实体键 | Report ID | 原始 Usage（十六进制） | 转换后 Usage | Windows 虚拟键 | 应用映射来源 |
|---|---|---|---|---|---|
| 音量加 | 01 | 80 | 68 / F13 | 7C | volume_up |
| 音量减 | 01 | 81 | 69 / F14 | 7D | volume_down |
| 返回 | 01 | F1 | 6A / F15 | 7E | back |

只修改第 4 字节，释放报文、F5 语音、方向键和其余报文原样传递。没有增加驱动长按阈值。
SayAll 在 Raw Input 精确匹配选定遥控器后才解码运输键，并将它们送入现有手势引擎。
音量事件不标记为“原生音量已执行”，因此不会误跳过同键映射的第一次注入。

- 必须运行支持此功能的 SayAll 并启用对应映射；关闭应用/映射或留空动作不会自动恢复原生三键。
- 驱动输出 F13–F15；普通键盘上的这些键不被全局接管。其他软件若注册了 F13–F15 全局快捷键，可能同时响应。
- 只支持 INF 中的精确硬件 ID；不会按相似外观或名称绑定其他设备，也不会修改全局键盘类过滤器。
- 不可与 QL-4/RemoteMapper 的 MiRemoteHidFilter 叠装，其额外语音/Home 等转换不属于本方案。
- 只有单击动作时无需双击判定等待；配置双击或长按后采用现有手势时序。语音键始终按下开始、释放结束。

## 3. 基础环境与配对

1. 使用 x64 Windows；应用基础要求 Windows 10 1809，但**本驱动要求 Windows 10 1903 / build 18362 或更高版本**。
2. 安装 WebView2 Runtime。基础配对、VB-CABLE、输入法配置见[基础安装指南](installation-and-configuration.md)。
3. 在 Windows 蓝牙设置中配对遥控器，再在 SayAll 的“连接与语音”选择设备。只连接一台目标遥控器进行测试。
4. 核对型号 RC001/RC003，记录 Windows 版本。硬件名称相似不代表同一协议。
5. 在“按键”页导出现有配置作备份。从系统托盘菜单选择“退出”；关闭窗口只会隐藏应用。

不要为了本功能删除原有蓝牙配对、改动输入法私有文件或禁用系统键盘驱动。

## 4. 从源码构建应用

开发环境：Git、Rust stable MSVC 工具链、Node.js 22+、项目指定的 pnpm、VS 2022 C++ 桌面工具与 Windows SDK。
驱动开发还需下面第 5 节的 WDK。建议在 VS 的 **x64 Native Tools Command Prompt** 中打开 PowerShell，确保 `cl`、`link` 可用。

```powershell
git clone --branch feature/three-button-driver https://github.com/superuct/remote-mic-app-windows.git
cd remote-mic-app-windows
# 以上为本功能开发分支；合入原仓库后也可从原仓库 main 构建。
corepack enable
corepack pnpm install --frozen-lockfile
pnpm test
cargo test --workspace
cargo fmt --all -- --check
powershell -NoProfile -File .\scripts\build-local-app.ps1
```

脚本先生成 `dist`，再执行：

```powershell
cargo build --locked --release -p sayall-windows-app --features custom-protocol
```

输出为 `target\release\sayall-windows-app.exe`。它内置页面，运行时不需要 Vite 或 localhost 端口。
不要把缺少 `custom-protocol` 的直接 Cargo 构建当作可独立分发的应用。
日常开发则运行 `pnpm tauri dev`，此时启动开发服务器是正常行为。

运行前从托盘退出旧版，然后在文件资源管理器中直接打开这份 EXE。
用进程路径或日志内 `source_revision` 确认版本，不能只看相同的窗口标题。
不要强杀旧进程，不要为了双开修改单实例互斥体。

## 5. 编译三键驱动

目录：`driver\SayAllThreeButtonFilter`。两种构建方式共用同一份 C 源码。

### A. 已安装 VS/WDK

安装 VS 2022、对应的 WDK 10.0.26100、WDK 的 VS 集成和所需 C++ 组件。
在配置好的开发者终端执行：

```powershell
msbuild .\driver\SayAllThreeButtonFilter\SayAllThreeButtonFilter.vcxproj /p:Configuration=Release /p:Platform=x64
```

该项目使用 KMDF 1.15。实际产物路径以 MSBuild 输出为准。此路线保留供标准开发环境使用；
本次可复现构建证据来自下面的便携 SDK/WDK 路线。

### B. 使用 Microsoft SDK/WDK NuGet

准备微软包 `Microsoft.Windows.WDK.x64`、`Microsoft.Windows.SDK.cpp`、
`Microsoft.Windows.SDK.cpp.x64`，本次验证版本均为 `10.0.26100.6584`。
解压后的 `c` 目录作为参数，外部 MSVC x64 环境仍需正确设置 PATH、INCLUDE、LIB。

```powershell
.\driver\SayAllThreeButtonFilter\Build-Portable.ps1 `
  -WdkRoot 'D:\toolchains\wdk\c' `
  -SdkRoot 'D:\toolchains\sdk\c'
```

路径仅为示例，请替换为自己的工具目录。脚本会先编译并运行 `remap_test.c`，
随后生成 SYS、执行 InfVerif、生成 CAT，输出到该驱动目录的 `build`。
构建失败时停止，不要沿用旧 SYS。SYS/CAT 仍需签名。

官方说明：[通过 NuGet 安装 WDK](https://learn.microsoft.com/en-us/windows-hardware/drivers/install-the-wdk-using-nuget)。

## 6. 创建自己的测试签名包

**每位开发者生成自己的测试证书，不使用公开共享私钥。** 本仓库不包含私钥、已信任证书、测试机器地址或采集日志。
以下操作只创建当前用户的签名身份，不会把它加入受信任根。
在含 `signtool.exe` 的 Windows 开发环境运行：

```powershell
$testCert = New-SelfSignedCertificate -Type CodeSigningCert `
  -Subject 'CN=SayAll Three Button Development' `
  -CertStoreLocation Cert:\CurrentUser\My `
  -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
  -KeyExportPolicy NonExportable -NotAfter (Get-Date).AddDays(30)

.\driver\SayAllThreeButtonFilter\Package-TestDriver.ps1 `
  -BuildDirectory .\driver\SayAllThreeButtonFilter\build `
  -OutputDirectory .\local-driver-package `
  -CertificateThumbprint $testCert.Thumbprint `
  -Inf2Cat 'D:\toolchains\wdk\c\bin\10.0.26100.0\x86\Inf2Cat.exe'
```

输出目录必须为空或不存在。脚本按**签 SYS → 重新生成 CAT → 签 CAT → 生成 SHA256.json** 的顺序操作。
私钥不导出；包内仅含公开证书。修改 SYS/INF 后必须重新生成和签署 CAT，不能复制之前的目录文件。
不要提交 `local-driver-package`；根 `.gitignore` 和目录规则排除了构建、证书和二进制产物。

## 7. 信任证书并开启测试签名

此步骤改变系统驱动信任/启动配置，只用于明确选择的开发测试机器。
普通用户不应把实验包当作生产签名安装器。确认系统恢复方式和数据备份后，使用管理员 PowerShell。

1. 对照构建者提供的校验值检查 SHA256.json 与包来源。哈希一致只说明字节一致，不证明来源可信。
2. 用 `Get-PfxCertificate .\local-driver-package\SayAllThreeButtonFilter.cer` 查看颁发者和有效期。
3. 记录原本是否已经启用测试签名，避免回滚时影响其他开发驱动：

   ```powershell
   bcdedit /enum
   ```

4. 确认这是自己构建或审阅过的证书后，导入本地计算机信任：

   ```powershell
   $certFile = Resolve-Path .\local-driver-package\SayAllThreeButtonFilter.cer
   Import-Certificate -FilePath $certFile -CertStoreLocation Cert:\LocalMachine\Root
   Import-Certificate -FilePath $certFile -CertStoreLocation Cert:\LocalMachine\TrustedPublisher
   bcdedit /set testsigning on
   ```

5. 保存工作，使用 **开始菜单 → 电源 → 重启**。仅重开应用或快速启动后的关机开机不能作为生效证据。

如果 Secure Boot 阻止设置，先停止并评估是否适合该机器，不要自动关闭固件保护、BitLocker 或内存完整性。
测试签名也要求 SYS 本身有签名，不使用 `nointegritychecks` 代替。
微软规则见[加载测试签名驱动](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/the-testsigning-boot-configuration-option)。

## 8. 检查、安装和核对加载

在管理员 PowerShell 中进入 `local-driver-package`：

```powershell
.\Status.ps1
.\Install.ps1 -CheckOnly
.\Install.ps1
.\Status.ps1
```

`Status.ps1` 通过 `SystemCodeIntegrityInformation` 查询**运行中的内核**，不把 BCD 中 `testsigning Yes` 等同于本次启动已生效。
`-CheckOnly` 执行签名、校验、设备和进程预检，不安装驱动。

安装会拒绝以下情况：文件校验不一致、证书不受信任或签名不符、内核测试签名未生效、
未找到唯一精确匹配的遥控器集合、旧 SayAll 尚未退出、存在 RemoteMapper 冲突驱动。
安装只调用目标 INF 的 `pnputil /add-driver ... /install`，不会强杀应用、改变配对或自动重启。

成功标准是状态中 `installedPackageCount=1`、`driverServiceState=Running`、设备状态 OK。
记录自动分配的 `publishedInf`（例如 `oemNN.inf`），不要照抄他人机器的编号。
如果尚未 Running，再保存工作并按提示重启后复查。本次 RC001 安装已观察到直接 Running，无需第二次重启。
驱动安装命令说明见[微软 PnPUtil 文档](https://learn.microsoft.com/en-us/windows-hardware/drivers/devtest/pnputil-command-syntax)。

## 9. 配置映射与实机验收

1. 普通用户身份打开第 4 节的测试版。旧 Release 中三个格子仍灰色，不能用于验证。
2. 进入“连接与语音”，确认目标遥控器已就绪；回到“按键”，启用自定义按键。
3. 为方便首轮验收，先只设单击：返回→Esc、音量加→音量加、音量减→音量减。保留其他键配置。
4. 点击预设会保存；检查卡片显示的动作。需要备份时使用页面导出功能，不手工修改 JSON 绕过界面。
5. 测试音量前将系统音量放在中间范围；在可由 Esc 关闭的普通弹窗中测试返回。
6. 每个键短按 3 次，每次松开，检查一次短按只有一次目标动作；再测试按住连发及松开后停止。
7. 空闲一段时间后再测第一次短按，上方向键也要测短按，不用长按成功代替短按通过。
8. 测试语音按下/释放、普通键盘 F13–F15、应用退出、重新连接后的状态清理。
9. 最后按需求设置双击/长按并分别验证。加入双击/长按后单击时序可能变化，这不等于驱动丢首按。

请记录：型号、系统版本、源码 SHA、驱动状态、配置动作、每组成功/总次数、闲置时间、是否出现重复或释放后连发。
不要在公开 Issue/PR 粘贴蓝牙 MAC、HID 路径、完整个人路径、语音内容或未经检查的原始日志。

## 10. 排错表

| 现象 | 检查与处理 |
|---|---|
| localhost 拒绝连接 | 用 `build-local-app.ps1` 或显式 `--features custom-protocol` 重新构建；先退出旧进程。不要开临时服务器冒充独立包修复。 |
| 三键仍灰色 | 检查进程实际路径和 `source_revision`，确认没有被单实例保护拦下后继续操作旧版。 |
| BCD 是 Yes，但脚本报测试签名未生效 | 看 Status 的 `testSigningActive`；完整重启 Windows。 |
| 匹配数量为 0 或大于 1 | 确认目标已配对、在线、只有一台；硬件 ID 不匹配时停止，不扩大 INF 到全部键盘。 |
| 签名不可信或证书过期 | 核对自己生成的证书、SYS/CAT 同一签名者、有效期及信任库。必要时重新构建签名包，不关闭校验。 |
| 服务未 Running 或设备错误 | 记录错误码；按安装提示重启仍不正常则回滚。服务存在不等于已接入目标设备。 |
| 三键有高亮但目标动作未发生 | 检查映射启用、动作内容以及日志 `map_fire`/注入失败；Windows 权限等级不同可能阻止向管理员程序注入。 |
| 同时触发其他快捷键 | 检查其他程序是否注册 F13–F15；它们是此驱动的运输键。 |
| 按键要长按才响应 | 先测仅单击配置，记录短按/释放/闲置首按；不直接调低时序常量，也不推断所有遥控器都相同。 |

日志位置遵循 [LOGGING.md](../LOGGING.md)。优先使用“权限 → 诊断摘要”。
开发日志应按 `three_button_driver ... phase=decoded` → `map_edges` → `map_fire` 追踪；
`decoded` 只代表事件已解码，目标应用响应才是成功。可用 [独立诊断工具](../Testing/rc003-diagnostic/README.md) 调查输入缺失，
但它的原始候选集不是 F13–F15 的最终映射验收工具。

## 11. 卸载和回滚

1. 从托盘正常退出 SayAll。
2. 在管理员 PowerShell 中运行包内 `Uninstall.ps1`。它只查找名为 SayAllThreeButtonFilter.inf 的包，
   恰好一个时卸载；多个版本会停止，要求人工逐项核对。不会使用 `/force` 批量删除。
3. 保存工作并重启，验证设备原始输入和基础语音恢复。卸载驱动不会删除 SayAll 的映射配置。
4. 只有测试签名是为本次实验开启、且没有其他测试驱动依赖时，执行 `bcdedit /set testsigning off` 并重启。
5. 在证书管理器中按本次 CER 的指纹核对，只删除本次新导入的 LocalMachine Root/TrustedPublisher 证书。
   如果证书或测试模式在实验前已存在，保留它们。私钥证书留在签名者的 CurrentUser My；是否删除由签名者决定。

若 Windows 无法正常启动，在 Windows 恢复环境中先确认系统盘符和**本次记录的 OEM INF**。
可由管理员用 DISM 对该离线 Windows 删除精确的本驱动包，再尝试启动；不要照抄盘符或他人的 OEM 编号，
更不要删除系统 HID、kbdhid、键盘类或蓝牙驱动。准备不充分时先由熟悉 Windows 恢复的人员处理。

## 12. 开源贡献与发布边界

- 应用使用仓库 GPL-3.0-only；驱动改编来源保留 MIT 许可全文，见 [ATTRIBUTION.md](../ATTRIBUTION.md)。品牌素材另有许可。
- fork 后在功能分支提交，向原仓库 main 开 PR。保留来源、测试命令与 `passed / failed / deferred` 矩阵。
- 可以公开源码和复现步骤；不提交签名私钥、测试机器证书、设备采集、个人配置或未经授权的第三方二进制。
- 测试签名不能替代微软生产驱动签名。公开生产安装包需要独立签名、安装生命周期和两型号硬件验收。
