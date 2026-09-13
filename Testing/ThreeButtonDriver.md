# 三键驱动验证记录

## 2026-09-13：开发测试记录

此记录只保留产品分类、测试结论和公开源码信息；不包含本机设备地址、HID 路径、证书、OEM 编号或原始日志。

### 实现范围

- 来源为 QL-4/RemoteMapper 的 MIT KMDF 过滤实现，来源提交与改编范围见 ATTRIBUTION.md。
- 仅将键盘报文 ID 01 的 Usage 80/81/F1 映射到 F13/F14/F15。
- 应用在已归因的目标 Raw Input 设备上产生 DriverEdge，使用现有手势识别和 SendInput。
- 三键配置不再在保存、加载或引擎入口被删除；普通键盘 F13–F15 不进入全局遥控器按键映射。

### 已执行

| 检查 | 结果 | 实际观察 |
|---|---|---|
| C 报文边界测试 | passed | 7,995,392 组：全部 ID/usage 与长度 0–121，验证非目标字节不变 |
| C 释放/重复/语音测试 | passed | 零释放原样传递、转换幂等、F5 不变 |
| MSVC x64 / KMDF 1.15 构建 | passed | 便携 SDK/WDK 10.0.26100.6584；SYS 生成 |
| InfVerif / Inf2Cat | passed | 无错误、无警告；目录生成成功 |
| Rust 平台层 | passed | 97 passed / 3 ignored；包含短按、重复、释放、全局键盘排除和音量首按不被对冲的用例 |
| 前端测试 / 生产构建 | passed | 65 passed，三键编辑入口及驱动提示可用 |
| 宿主单元测试 | passed | 16 passed / 1 ignored |
| 本地测试签名信任 | passed | SYS 签名 Valid；签名者使用本机开发证书，证书不入库 |
| 当前内核测试签名 | passed | 完整 Windows 重启后 CodeIntegrity 测试签名标志为真 |
| RC001 安装与加载 | passed | 恰好匹配一台设备；PnPUtil exit 0，服务 Running，设备 OK，无需安装后再次重启 |

### 发现并修复的问题

1. **BCD 设置存在但未完成内核重启**：第一次只看到配置项 Yes，运行中内核标志仍为假。
   安装在预检阶段停止。完整重启后通过。公开 Install/Status 改用内核查询，不依赖本地化的 BCDEdit 文本。
2. **独立 EXE 请求 localhost:2430**：直接 Cargo release 构建没有启用 Tauri custom-protocol。
   已补 Cargo feature 与 `scripts/build-local-app.ps1`；重新构建通过，最终可见窗口验收仍未完成。
3. **启动到旧版**：启动工具按相同应用名打开了旧安装路径，界面三键仍灰色。
   验证应检查实际进程路径/source_revision，不能只认窗口标题；先托盘退出旧版再运行新 EXE。

### 未完成的验收（deferred）

- 修复内嵌前端后的窗口启动、配置保存再加载。
- RC001 返回、音量加减各组实际目标动作、首次短按、连续短按、连发停止与冷/闲置首次。
- 上方向短按和语音按下/释放回归，普通键盘 F13–F15，睡眠、断连和进程重启。
- 卸载驱动并恢复原始输入、证书/启动设置的完整回滚。
- RC003 单独的安装与上述全部实机测试。
- VS/WDK 集成项目路线、微软生产签名、公开安装器生命周期。

以上 deferred 项不阻止作为实验源码公开和代码审阅，但不满足正式产品验收。

## 可重复测试命令

```powershell
pnpm test
pnpm build
cargo test -p sayall-windows --lib -- --test-threads=1
cargo test -p sayall-windows-app --lib --release -- --test-threads=1
cargo fmt --all -- --check
.\scripts\build-local-app.ps1
```

驱动命令与安装顺序见[完整操作指南](../docs/three-button-driver-guide.md)。
提交新的实机结果时记录型号、Windows 版本、源码 SHA、配置、成功次数/总次数、闲置时长和边界。
不要把示例/模拟器/CI 的按键事件标注为实体遥控器结果。

开源整理阶段：scripts/ci-preflight.ps1 的 6 项快速门禁全部通过；管理脚本测试通过合法包、篡改脚本、缺失校验项、路径越界、不受信任签名和签名者不符用例，全部 PowerShell 脚本语法解析通过。Common.ps1 对当前内核测试签名和唯一目标集合的只读检查通过。新签名打包脚本的端到端签名/安装复验仍为 deferred，不能以模拟签名器测试代替。
