# SayAll 三键可选驱动

本目录包含完整 C 源码、构建、测试、签名打包、安装、状态检查及卸载工具。
完整教程见 [返回/音量加减操作指南](../../docs/three-button-driver-guide.md)，验证范围见 [测试记录](../../Testing/ThreeButtonDriver.md)。

这是实验功能：RC001 主机已观察到安装成功、服务 Running、设备 OK；最终动作、闲置首按、语音回归和回滚仍待实机验证。RC003 未独立验收。

## 文件索引

| 文件 | 用途 |
|---|---|
| driver.c / driver.h | KMDF 1.15 读取完成过滤 |
| remap.c / remap.h | 只改三键的纯 C 报文转换 |
| remap_test.c | 全报文 ID/Usage/长度边界、释放、语音不变检查 |
| SayAllThreeButtonFilter.inf | 精确设备 ID、独立服务及 ExtensionId |
| SayAllThreeButtonFilter.vcxproj | VS/WDK x64 工程 |
| Build-Portable.ps1 | 使用配置好的 MSVC 与 SDK/WDK NuGet 编译并检查 |
| Package-TestDriver.ps1 | 使用开发者自己的签名证书制作测试包，不导出私钥 |
| Common.ps1 | 运行内核测试签名、包校验、设备及单实例检查 |
| Install.ps1 | 管理员安装；-CheckOnly 仅预检 |
| Status.ps1 | 管理员读取脱敏运行状态，不作硬件通过判定 |
| Uninstall.ps1 | 只卸载唯一匹配的本驱动，不自动清除信任/启动设置 |
| Test-ManagementScripts.ps1 | PowerShell 7 无系统变更的脚本测试 |
| LICENSE.RemoteMapper | 改编来源 MIT 许可 |

## 快速参考

驱动最低 Windows 10 1903 x64（应用基础最低版本不同）。只转换 ID=01 的第 4 字节：

| 实体键 | 输入 Usage | 输出 |
|---|---|---|
| 音量加 | 80 | 68 / F13 |
| 音量减 | 81 | 69 / F14 |
| 返回 | F1 | 6A / F15 |

释放、F5 语音、方向键和其他报文原样传递。应用必须运行并启用相应映射；退出应用不会恢复驱动转换前的三键行为。其他软件的 F13–F15 全局快捷键可能同时响应。普通键盘 F13–F15 不被 SayAll 全局截获。

不要与 MiRemoteHidFilter 叠装，不要将 INF 扩大到全部键盘。测试签名不等于微软生产签名。
签名顺序为 SYS → 生成 CAT → 签 CAT → 校验清单；改动 SYS/INF 后必须重新制作目录。
包内 SYS/CAT/CER/清单由构建者生成，Git 仓库不携带测试机证书、私钥、采集或第三方二进制。

安装前按完整教程明确选择测试信任及启动设置，保存工作后执行 Windows“重启”。
Status 的 testSigningActive 检查运行内核，不仅查看 BCD。安装脚本不会更改启动设置或自动重启。
从源码直接构建应用时必须使用 scripts/build-local-app.ps1 或 --features custom-protocol，避免 localhost 连接失败。
