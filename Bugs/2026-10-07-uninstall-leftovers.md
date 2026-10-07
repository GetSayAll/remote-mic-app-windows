# 卸载残留：运行时目录（234 MB）与登录启动项未清理

- 发现日期：2026-10-07（用户要求"确保卸载干净"时做的审计）
- 状态：已修复；提权清理那一支的**真机复测 `deferred`**
- 影响范围：所有安装过并开启过「全按键支持」的用户（与架构无关）；卸载后残留
- 功能点：安装器卸载流程（`src-tauri/windows/installer-hooks.nsh`）+ 助手的运行时目录
- 现象：卸载后 `%PROGRAMDATA%\SayAll\rc003-helper\` 与 `HKCU\...\Run` 的 `SayAll` 值仍在
- 复现条件：装一次、开过全按键支持（助手跑过以后运行时目录就有东西），再卸载
- 正常预期：卸载后安装目录、快捷方式、注册表卸载项、计划任务、运行时目录、登录启动项都不留

## 证据（审计结果，2026-10-07）

卸载流程此前已有：请求应用优雅退出 → 停助手（撤销授权 + 写 reauth 标记）→
`schtasks /delete` 删计划任务；CI 断言覆盖安装目录、开始菜单、卸载注册表项、用户数据保留。

残留两处：

| 残留 | 实测 | 为什么没清 |
| --- | --- | --- |
| `%PROGRAMDATA%\SayAll\rc003-helper\` | 本机 **234.1 MB / 11 个文件**（`frida-gadget.dll`、两个 `gen-*` 世代目录、`session.token`、`helper-task.log`、`rc003_agent.js`…） | 目录由**提权**进程创建：ACL 只给 `BUILTIN\Users` `(RX)` + 「目录内可新建」，**不给删已有文件**（`icacls` 实测）。非提权的卸载器删不掉 |
| `HKCU\...\CurrentVersion\Run` 的 `SayAll` | 指向已删除的 exe | 卸载器没碰它（应用只在用户关掉开关时才删） |

附带发现：`installer-hooks.nsh` 里我最初写的清理路径用了 `$PROGRAMDATA` —— **NSIS 没有这个变量**，
makensis 只警告然后把字面量留下，实际会去删相对路径。探针编译（`makensis -INPUTCHARSET UTF8`
+ 最小 `.nsi` 头 + `!insertmacro NSIS_HOOK_PREUNINSTALL`）当场抓到，改用 `ReadEnvStr $R7 "ProgramData"`。

## 修复

1. **助手新增 `--uninstall-cleanup`**（`hardware/RC003/helper/src/main.rs`）：递归删除运行时目录；
   删不掉的文件（典型：宿主仍映射着那一代 Gadget）用 `MoveFileEx(DELAY_UNTIL_REBOOT)` 安排到
   下次重启删除；目录空了连父目录 `%PROGRAMDATA%\SayAll` 一起收掉。非提权调用直接拒绝（退出码 2）。
2. **助手自清通道**：卸载器在停助手**之前**写标记 `%LOCALAPPDATA%\SayAll\rc003-uninstall-cleanup`；
   仍在运行的助手见到停用信号时顺手清理（它本来就是提权的）——这条路径**不弹 UAC**。
   路径两侧（NSIS 与 Rust）各钉一条断言。
3. **卸载器钩子**（`NSIS_HOOK_PREUNINSTALL`）：写标记 → 停助手 → 删 `Run` 值 →
   清理运行时目录（先非提权 `RMDir /r`；仍留下且非静默时用 `ExecShell "runas"` 拉起已装好的
   助手做提权清理，按目录是否消失等待 20s；静默安装在无人应答 UAC 的环境里跳过提权）→
   收空父目录 → 仍剩下时如实提示（授权被取消或文件被占用，已安排到重启后删除）→
   删掉两个信号文件（残留的停用信号会让重装后的助手一启动就退出）。
4. **护栏**：`--follow-app` 的默认日志会把运行时目录**建回来**，因此该分支加了
   `&& !args.uninstall_cleanup`——否则"清完立刻又建回来"。
5. **CI 断言**（`scripts/test-windows-silent-install.ps1`）：卸载前人为造出 `Run` 值与运行时目录
   夹具，卸载后断言两者都已消失（否则这两条断言在 CI 里是空的）。用户数据（`%APPDATA%` 下的
   设置/映射/统计）按既有策略**保留**，原有断言不动。

## 验证

- 助手：`cargo test --manifest-path hardware/RC003/helper/Cargo.toml` **34 passed**；
  `--selftest` **全部通过**（新增两项：清理标记路径与安装器侧一致、递归删除整棵目录树）；
  非提权执行 `--uninstall-cleanup` → 退出码 2 且给出明确拒绝文案（`passed`）。
- 钩子：`makensis -INPUTCHARSET UTF8` 探针编译 `passed`（0 警告）；
  `cargo test -p sayall-windows-app --lib installer_` 10 passed（含既有契约断言）。
- 脚本：`test-windows-silent-install.ps1` PowerShell 解析通过。
- CI（run 37633120610，sha `17756c0`）：`verify` 16m27s **success**；
  `Verify Windows installer lifecycle` 9m32s **success**，其中静默安装/卸载步骤输出
  `Leftovers removed: run-at-login entry, RC003 runtime directory`
  —— 该行只在两条断言都通过后才打印（`Run` 值仍在即 `throw`，运行时目录仍在即 `throw`），
  夹具在卸载前由脚本人为造出（`Run` 值指向已装 exe + 运行时目录里的 `session.token`
  与 `gen-ci-fixture/frida-gadget.dll`），因此这是**真实装/卸上的断言通过**，不是空断言。
- **`deferred`**：提权清理那一支（`ExecShell "runas"` + 被占用文件的"重启后删除"）需要真实**非提权**
  管理员账户 + 已跑过助手的机器才能复现与验证。CI runner 用提权账户、夹具也由同一账户创建，
  所以它覆盖的是"非提权删除能成功"与 `Run` 值两条，**覆盖不到**原始场景里
  「提权助手建的 ACL 拒绝普通用户删除已有文件」。请在本机跑一次"装 → 开全按键支持 → 卸载"，
  确认目录消失、必要时出现一次 Windows 授权窗口。

## 隐私检查

本文不含个人路径（写 `%PROGRAMDATA%` / `%LOCALAPPDATA%` / `%APPDATA%`）、不含设备身份、
蓝牙地址、语音内容或凭据；助手侧清理日志只记文件名，不记路径。
