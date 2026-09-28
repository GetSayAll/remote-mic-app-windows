# 卸载→重装后授权未撤销（开关未回落、重开未弹 UAC）

- 发现日期：2026-09-28（Andy 现场报告）
- 状态：已修复（本地自动化 passed；真机复验 deferred，**须用修复后构建重装复验**）
- 影响范围：Windows 安装器钩子（`src-tauri/windows/installer-hooks.nsh`）授权撤销语义；不影响基础语音与按键路径
- 功能点：卸载/重装后的「全按键支持」授权生命周期（重授权标记 + 计划任务）
- 现象：卸载应用 → 重新安装 → 启动后「全按键支持」开关**没有回落为关闭**，重新开启也**没有触发 UAC**（授权复活，103ms 内无弹窗完成 enable）
- 复现条件：卸载已授权的安装包后重新安装任意携带旧授权语义（或修复前语义）构建的包
- 正常预期：卸载撤销授权 → 重装启动时开关回落为关闭 → 用户重新开启时走一次 UAC 重新授权（2026-09-28 ab83163 定稿语义）
- 证据：
  - 诊断日志：12:29:48 `app_exit reason=installer_requested_exit`（pid=23784，`source_revision=877a3c59` 被卸载器优雅退出）→ 12:51:13 起新进程 pid=7048 **`source_revision=0a876485`** → 12:51:21 `action=enable phase=started` → 103ms 后 `terminal_result=passed`（无 UAC）、`helper_authenticated version=2`
  - 已安装 exe（`%LOCALAPPDATA%` 安装布局）内嵌 revision 为 `0a876485`，不含 `877a3c5`；`0a876485` 为另一 agent 分支 12:27 的构建，**不含 ab83163**
  - main（d5bd0ca）版钩子实况：PREINSTALL 存在**无条件** `Delete "$LOCALAPPDATA\SayAll\rc003-reauth-required"`；卸载器写裸字面 `reauth`
  - 现场标记文件不存在（被 PREINSTALL 清掉）；`C:\Windows\System32\Tasks` 递归查无 SayAll 任务（目录权限受限；且已知普通权限卸载器删不掉提权任务）
- 根因（两层）：
  1. **直接原因**：本次重装安装的包来自另一分支构建 `0a876485`，其 PREINSTALL 沿用 main 的无条件删标记语义；计划任务在卸载中幸存（提权创建、普通权限卸载器删不掉），标记被清后 `authorization_required` 判 false → enable 直通。
  2. **设计缺口**（即使安装 ab83163 构建也存在）：ab83163 依赖标记**新鲜度**（≤120s 判为升级产物）区分「升级」与「卸载后快速重装」——两条路径在系统状态上不可区分，120s 内重叠。本次为彻底修复：利用 NSIS 真卸载会把卸载器拷进 `%TEMP%\~nsu*.tmp` 执行的机制（`$EXEDIR != $INSTDIR`），**写入时机**即已区分两类路径，不再依赖时间窗口。
- 修复（`installer-hooks.nsh`）：
  - `SayAllReauthMarkerAge` 新增 `revoked` 分类：内容以 `uninstalled=` 为前缀（`StrCpy $R8 $R9 12` 判断，不依赖 `IntOp` 对非数字的隐式归零）；
  - 卸载器只在**真卸载**（`$EXEDIR != $INSTDIR`）时写标记，内容 = `uninstalled=<卸载时刻 GetTickCount>`；升级路径的原位卸载（`_?=$INSTDIR`）**根本不写**；
  - `SayAllClearFreshReauthMarker` 只清理旧版卸载器产物（`fresh`/`legacy`），`revoked` 与 `stale` 一律保留——带 `uninstalled=` 前缀的标记是「真卸载撤销过授权」的凭证，**任何安装都不得删除**；
  - 契约测试 `installer_preserves_capture_authorization_on_upgrade`（`src-tauri/src/lib.rs`）同步覆盖五态语义与守卫位置。
- 修复后语义：升级 → 授权与开关状态保持不变；卸载 → 标记 `uninstalled=` 落盘 → 重装启动开关回落关闭 → 重开走一次 UAC。**不再有时限**。
- 残留风险（过渡期，仅一次）：旧版卸载器（写裸 tick / 字面 `reauth` 的构建）真卸载后 ≤120s 内重装，其标记会被新版 PREINSTALL 当作过渡产物清理 → 授权复活，需手动关一次开关；从修复后版本起的真卸载写 `uninstalled=` 新格式，无此窗口。
- 验证：
  - `cargo test -p sayall-windows-app`：38 passed（含新契约断言）—— passed
  - `cargo fmt --all -- --check`、`cargo check --workspace` —— passed
  - **真机复验 deferred**：须以修复后构建出包 → 安装并授权 → 卸载 → 重装 → 核对开关回落关闭 → 重开触发 UAC。注意：修复只在本分支构建生效，用旧分支构建（如 `0a876485` 语义）重装不构成对本修复的检验。
- 隐私检查：未包含个人路径、设备身份、语音内容或凭据
