# 卸载时弹「增强捕获的运行时数据未能立即全部删除」——被宿主占用的文件锁

- 发现日期：2026-10-08
- 状态：已修复；真机验收 **passed**（2026-10-08 本机，机制见文末"追加"）
- 影响范围：0.8.1 / 0.8.2；Windows 10/11；RC003 增强捕获；卸载与升级安装（升级时先跑旧版卸载器）
- 功能点：安装器卸载清理（`src-tauri/windows/installer-hooks.nsh` + 助手 `--uninstall-cleanup`）
- 现象：卸载（或升级安装开头）弹出一个说明窗：运行时数据未能立即全部删除，其余文件可手动删除
- 复现条件：遥控器仍连着——承载它的系统进程 `WUDFHost.exe` 正映射着
  `%ProgramData%\SayAll\rc003-helper\frida-gadget.dll`——时执行卸载
- 正常预期：用户不需要为了卸载先断开遥控器，也不该看到一个需要人工介入的提示
- 证据（2026-10-08 现场，本机）：
  - 安装器把该 dll 登记进了 `PendingFileRenameOperations`（同一条路径重复 8 次，来自当天多次卸载/升级）；
  - 助手只读 `--dry-run` 显示 `[HOST] pid=…` + `[EXCLUSIVE] verdict=exclusive_rc003_host`——宿主仍在，
    且该代 Gadget 仍被映射；
  - 弹窗出现后一切功能正常（弹窗只说明删除被推迟，不是卸载失败）。
- 根因：DLL 被宿主进程映射 = 文件锁（Windows 不允许删除已映射的镜像）；宿主进程只在设备链路断开
  或重启时才退出，因此"卸载时立即删干净"与"遥控器还连着"天然冲突。旧实现把冲突推给用户
  （弹窗建议手动删除），不符合"用户侧零介入"。
- 修复（方案 B，2026-10-08 用户选定："不应该让用户去断开，看看还有什么办法不弹窗"）：
  1. 助手 `--uninstall-cleanup`：先删能删的；**确有删不掉的文件**时对遥控器设备做一次
     **禁用 → 启用**（SetupAPI `DIF_PROPERTYCHANGE`，`DICS_DISABLE` / `DICS_ENABLE`；设备实例 ID
     仅运行期使用、绝不进日志），放掉宿主后重试删除。安全边界：**永远重新启用**（最多 5 次，
     失败会在日志里明确写出）；共享宿主或任一步失败 → 自动回退"安排到重启删除"。
  2. 安装器钩子：卸载路径**移除 MessageBox**；新增契约测试 `uninstall_never_shows_a_cleanup_message_box`
     防止把提示加回来。清理结论只落诊断日志的 `[CLEANUP]` 行（日志目录在 LOCALAPPDATA，卸载不删）。
  3. `schedule_delete_on_reboot` 先查已有清单：同一路径不重复登记（消除 8 条重复那类噪音）。
- 验证：助手单测 **39 passed**（含新增"待删清单包含判断""RC003 设备判据"两例）、助手 `--selftest` 通过；
  应用 crate **107 passed**（含新增卸载契约测试）；`cargo fmt --all -- --check` 干净。
  **真机复测 deferred**：需在遥控器连着时做一次交互式卸载，确认①不再弹窗 ②运行时目录当场删净
  ③设备禁用后已重新启用、遥控器仍正常。
- 隐私检查：全文不含设备身份、个人路径、令牌或语音内容；设备实例 ID 仅用于运行期操作，不进日志。

## 追加（2026-10-08 晚）：5 轮真机复测把"禁用设备"否掉，机制换成"结束宿主"

### 真机逐条否定掉的假设（全部有日志/命令证据，勿回退）

| 假设 | 真机结果 |
| --- | --- |
| 设备环闸门看 `is_clean()` | **缺陷**：`is_clean` 只看 failures，被锁文件记在 `files_pending_reboot` → 设备环被跳过（日志 `files_pending_reboot=2` 紧跟着"删除 0 个目录 / 0 个文件"）。改为 `host_release_still_needed`（只看目录还在不在） |
| 实例 ID 存最后一段、`CreateDeviceInfoList`+`OpenDeviceInfoW` 打开设备 | **缺陷**：设备实例 ID 必须是完整 `<枚举器>\<设备>\<实例>`；改用 devcon 式 `ALLCLASSES\|PRESENT` 枚举 + 完整 ID 匹配 |
| `SP_CLASSINSTALL_HEADER.cbSize` 填整块 20 | **缺陷**：`SetupDiSetClassInstallParams` 报 1784；样例值 8，做双值重试 |
| **"禁用设备"这条路本身** | **走不通**：`pnputil /disable-device` 原话 `Cannot disable critical system device`（`SetupDiCallClassInstaller` 同码 `CR_NO_SUCH_DEVINST 0xE0000201`，与实例 ID/参数无关）；`pnputil /restart-device` 返回 0 但**不卸载**注入模块——因为 gadget 是 Frida **注入**，不是驱动加载，设备重启与它无关 |

### 最终机制（已验收）

1. **结束持有注入且"独占 RC003"的宿主进程** → 两份 `frida-gadget` 映射随进程消亡 → 运行时目录
   **普通删除即成功**（真机：`plain_delete=ok`，不需要 takeown / 改 ACL）。
   安全闸：① 宿主必须是 `members==1 && rc003_members==1`（共享宿主一律不动手，回退到重启删除——
   2026-09-25 起产品允许宿主同时承载别的 HID 设备）；② 必须确实枚举到我们的 gadget 模块
   （防 pid 复用/误伤）；③ 复用既有 `terminate_process`（产品里本来就用它换 Gadget 世代）。
2. **修设备节点**：宿主被终结后其挂载节点可能停在 `Error`（真机：遥控器本体 OK、HID 子节点 Error，
   此时按键不可用）。`pnputil /remove-device <实例 ID>` + `/scan-devices` 后恢复（真机：只 restart
   不生效，Error 仍为 1；remove+scan 后为 0；之后宿主重新拉起且**不再有 gadget 映射**）。
3. **另一独立缺陷**：`pending_reboot_blob()` 用 `from_wide` 读 `REG_MULTI_SZ`，遇第一个 `\0` 截断 →
   判重只看得见第一条 → 每轮各多登记一条（真机同一 dll 攒到 5 条）。改 `multi_sz_bytes_to_string`
   整段保留，并加回归单测钉住"目标不是第一条"。

### 验收证据（2026-10-08 本机，非仿真）

注入真实存在（锁探测 `lock_probe=locked hresult=2147024891`）时调用被测的 `--uninstall-cleanup`：

```
[CLEANUP] dir=… files_deleted=3 files_pending_reboot=1 failures=-
[CLEANUP] 待重启清单已含 frida-gadget.dll（listing_bytes=905），跳过重复登记
[CLEANUP] host_release=terminating gadget_maps=1
[CLEANUP] host_release=released
[CLEANUP] dir=… files_deleted=1 files_pending_reboot=0 dirs_deleted=2 failures=-
[CLEANUP] device_recover=ok remove_ok=true scan_ok=true
[CLEANUP] 已清理运行时目录（删除 2 个目录 / 1 个文件）
```

外部观察：`runtime_dir_exists=False`、`runtime_files_after=0`、`bthle_err=0`（无 Error 节点）。
弹窗：真机交互式卸载连续多轮未再出现（契约测试 `uninstall_never_shows_a_cleanup_message_box` 兜底）。

### 未覆盖边界

- 卸载器 → 助手这段调用关系在前 5 轮真机卸载里已反复验证（助手均以 `elevated=true` 被拉起）；
  本次验收验证的是助手内部的**新机制**，未再跑一次完整交互式卸载。
- **共享宿主**下走的是回退路径（安排到重启删除），该分支只有单测覆盖，真机未构造。
- v0.8.2 draft 的安装包内是**旧助手**：发布前必须用含本修复的 main 重新出包。
