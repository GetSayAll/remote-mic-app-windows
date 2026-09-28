
## 2026-09-28 第四轮：维护模式卸载不撤销授权（同日二次修正）

- **现象（Andy 现场报告）**：Windows「设置 → 应用」卸载已生效（第三轮修复验证通过）；但双击安装包 →「已安装」页选「卸载 无线麦 SayAll」的**维护模式卸载**不撤销授权。
- **根因（生成的 installer.nsi 实证）**：`PageLeaveReinstall` 的 `reinst_uninstall` 段对**所有**向导内卸载（升级「卸载后安装」、维护模式「卸载」）都用 `ExecWait '"$INSTDIR\uninstall.exe" _?=$INSTDIR'` **原位调用**旧卸载器（`$EXEDIR == $INSTDIR`）——与第三轮的 `$EXEDIR != $INSTDIR` 真卸载守卫正好互斥，标记不写。且卸载成功后向导**不退出**，继续走目录页与安装节**重装**——净效果等同覆盖安装，授权保留。
- **难点**：原位调用此刻无法区分「升级」与「维护卸载」——进程、命令行、版本关系（卸载器 = 已安装版本）全部相同；版本关系（同版本 vs 新版本）只在安装器的重装页可知。
- **修复（纯 NSIS 侧，Rust 不动）**：
  - 卸载器原位调用（`$EXEDIR == $INSTDIR`）改写**待决文件** `$LOCALAPPDATA\SayAll\rc003-uninstall-pending`，内容 `pending-uninstall=${VERSION}`；真卸载分支照旧写 `uninstalled=<tick>` 并清掉残留待决；
  - PREINSTALL 新增 `SayAllResolveUninstallPending`（先于过渡清理执行）：待决内容 == 本版本（维护卸载后向导重装）→ 写 `uninstalled=<tick>` 撤销凭证；其他版本（升级）→ 删待决、授权保留；待决文件两条出路都删（瞬时信号，不承载跨安装语义）。
- **语义矩阵**：真卸载（设置→应用 / 双击 uninstall.exe / /S）→ 直接撤销 ✓；维护模式卸载 → 向导重装完成后启动应用，开关回落关闭 ✓；交互升级（卸载后安装 / 不卸载）→ 授权保留 ✓；静默升级 /S（无向导页，无原位调用）→ 保留 ✓；修复（添加/重新安装组件）→ 保留 ✓。
- **已知边界（fail-safe 方向优先）**：① 维护卸载后在目录页取消向导 → 待决文件残留，由下一次安装裁决（同版本 → 撤销，视为「卸载已发生」；换新版本 → 删待决、保留）；② 卸载器中途取消 → 应用未卸载，待决文件残留，后续**同版本修复**会误判为维护卸载而撤销（多走一次 UAC，方向安全）；③ 旧版卸载器（无待决逻辑）的维护卸载 → 不撤销，过渡期一次，换装本修复后消失。
- 契约测试：`installer_resolves_maintenance_uninstall_via_pending_file`（新增）+ `installer_preserves_capture_authorization_on_upgrade`（扩展待决断言）——38 → 39 passed。
- 隐私检查：未包含个人路径、设备身份、语音内容或凭据
��载**（`$EXEDIR != $INSTDIR`）时写标记，内容 = `uninstalled=<卸载时刻 GetTickCount>`；升级路径的原位卸载（`_?=$INSTDIR`）**根本不写**；
  - `SayAllClearFreshReauthMarker` 只清理旧版卸载器产物（`fresh`/`legacy`），`revoked` 与 `stale` 一律保留——带 `uninstalled=` 前缀的标记是「真卸载撤销过授权」的凭证，**任何安装都不得删除**；
  - 契约测试 `installer_preserves_capture_authorization_on_upgrade`（`src-tauri/src/lib.rs`）同步覆盖五态语义与守卫位置。
- 修复后语义：升级 → 授权与开关状态保持不变；卸载 → 标记 `uninstalled=` 落盘 → 重装启动开关回落关闭 → 重开走一次 UAC。**不再有时限**。
- 残留风险（过渡期，仅一次）：旧版卸载器（写裸 tick / 字面 `reauth` 的构建）真卸载后 ≤120s 内重装，其标记会被新版 PREINSTALL 当作过渡产物清理 → 授权复活，需手动关一次开关；从修复后版本起的真卸载写 `uninstalled=` 新格式，无此窗口。
- 验证：
  - `cargo test -p sayall-windows-app`：38 passed（含新契约断言）—— passed
  - `cargo fmt --all -- --check`、`cargo check --workspace` —— passed
  - **真机复验 deferred**：须以修复后构建出包 → 安装并授权 → 卸载 → 重装 → 核对开关回落关闭 → 重开触发 UAC。注意：修复只在本分支构建生效，用旧分支构建（如 `0a876485` 语义）重装不构成对本修复的检验。
- 隐私检查：未包含个人路径、设备身份、语音内容或凭据

## 2026-09-28 第三轮：现场复验失败诊断 + 回落取证盲区修复

- **现场复验（14:11–14:22 本地）仍报失败**：卸载 → 重装 → 开关未落为关闭、未要求重新授权。
- **受控实验（14:34–14:37，本机实测）证实链路通畅**：`uninstall.exe /S` 静默真卸载 → 进程 3.4s 全退、标记写入 `uninstalled=152468250` → 静默重装 → 标记**原样保留** ✓。修复本身无缺陷。
- **现场失败归因**：用户三轮操作走的是**覆盖安装/升级语义**（直接双击安装包，旧卸载器被 `_?=$INSTDIR` 原位调用、`$EXEDIR == $INSTDIR` 不写标记）→ 授权与开关状态保留是**定稿正确行为**，不是回归。日志旁证：14:13:46 启动无 `auto_trigger started`（settings 已回落，回落机制实际生效过）；14:22 用户开启 86ms 无 UAC（当时标记已不在）。
- **用户假设「卸载时应删除开关状态」不采纳**：开关意图存 AppSettings（`settings.json`），升级/覆盖安装保留语义依赖它；卸载时删数据会破坏「升级保留」。正确机制 = 真卸载标记（`uninstalled=`）+ 启动对账回落，已实测可用。
- **取证盲区修复（本轮代码改动）**：启动对账回落分支原先只 `eprintln!`（stderr，现场不可见）→ 改为 `gatt_note` 落诊断日志：`rc003 feature=enhanced-capture action=reconcile phase=completed terminal_result=revoked reason=reauth_marker_present|task_missing reauth_required=<bool> task_installed=<bool>`。reason 只陈述探针可支撑的结论。`cargo test -p sayall-windows-app --lib` 38 passed。
- **复验要点（给 Andy）**：① 必须走**真卸载**——Windows「设置 → 应用」卸载，或运行安装目录里的 `uninstall.exe`；直接双击新安装包属升级语义，授权保留是设计使然；② 当前现场已是「真卸载后重装 + 标记在」状态，启动应用即可看到开关回落为关；③ 回落现在会落诊断日志（`action=reconcile … terminal_result=revoked`），可事后取证。
- 隐私检查：未包含个人路径、设备身份、语音内容或凭据
