# Windows BLE 资源耗尽恢复验证

日期：2026-09-14

## 验证目标

确认 Windows BLE 栈返回 `0x80070008` 或 `0x80004004`、普通 Radio API 无法取得
对象时，SayAll 能以公开 Windows 接口恢复蓝牙设备节点，而不是要求用户进入设置
手工关开蓝牙；同时确认连接会话的 WinRT service/device Close 失败可被再次重试。

## 现场结果

| 项目 | 结果 | 证据边界 |
| --- | --- | --- |
| 参考实现 paired selector → `FromIdAsync` | failed | 僵死现场返回 `0x80004004` |
| 直接 WinRT GATT service selector | failed | 僵死现场返回 `0x80070008` |
| SetupAPI + Win32 GATT `CreateFile` | failed | 接口可枚举，打开返回 `0x80070079` |
| 普通用户 PnP 重启 | failed | Windows 拒绝访问；退出码不能单独代表成功 |
| UAC 明示授权的唯一 BTHUSB 节点重启 | passed | ignored live test 1.95s 完成；WinRT Radio 读回成功 |
| 恢复后的 `BluetoothLEDevice` 创建 | passed | SayAll 日志前进到 `conn_params`，不再即时资源耗尽 |
| 提交 `341721f` 本地 NSIS 安装与启动 | passed | 静默安装退出码 0；日志 source revision 与提交一致 |
| 当前所选遥控器完整 BLE 建链 | passed | 2.806s 内完成设备、服务、三项特征、两路订阅和能力响应 |
| RC001 完整连接与首次语音 | deferred | 本轮未取得完整能力协商和语音证据 |
| RC003 完整连接与首次语音 | deferred | 本轮未取得完整能力协商和语音证据 |

## 安全与产品边界

- PnP 兜底只接受两个已验证的系统栈错误码，常规 Radio Off/On 仍是首选路径。
- SetupAPI 必须恰好找到一个当前存在、服务为 `BTHUSB` 的蓝牙类设备；零个或多个都
  终止恢复，不猜测目标。
- 使用 `%SystemRoot%\System32\pnputil.exe` 固定系统工具；设备实例 ID 不进入命令
  shell、不写日志，并拒绝引号、换行、NUL 与异常长度。
- PnP 重启必须由 Windows UAC 明示授权。主程序始终保持普通用户权限；用户拒绝授权
  时继续普通自动重连，不把失败伪装成成功。
- 工具执行后必须重新枚举 WinRT Bluetooth Radio；只有读回成功才报告恢复完成。

## 自动化与本地包

- `cargo check --workspace --all-targets --all-features`：passed。
- `cargo test --workspace`：176 passed，0 failed，5 ignored（硬件/联网显式测试）。
- `pnpm test -- --run`：83 passed，0 failed。
- `pnpm build`：passed。
- 本地 NSIS：passed；`SayAll-Windows-0.2.6-ble-pnp-recovery-341721f-local-x64-setup.exe`
  的 SHA-256 为
  `701e7870c813de89bfb46eb852486b3717a449a9278d148e0e84995e5d1ac6a5`。
- 安装版进程启动后，诊断日志记录 `source_revision=341721f...`、
  `radio_recovery_prepare ... cache=ready access=allowed`，随后当前所选遥控器完成
  capability response；真实双型号和首次语音结果只按上表标记，不由单型号连接推导。

---

# 2026-09-16 追加：无线电 Off/On 的 A/B 结论与复算操作

## 一句话结论

**在蓝牙栈僵死态（`windows_resource_exhausted` / `winrt_operation_aborted`），
关开无线电无效。** 实验组恢复率 0.61%（3/488），对照组 0.62%（15/2406），
相差 **-0.01 个百分点**（判定阈值 ±10）。恢复是普通重连自己等到的，不是开关换来的。

证据：`artifacts/ev_stream_raw.txt`（0.2.6，僵死现场，2983 条 `ble_connect` 记录、
657 条 `ble_radio_recovery` 记录）。

> **计数口径更正（2026-09-16）**：现场日志里**没有** `ble_radio_recovery phase=requested`
> 这一行——分流之前的实现只在 Off/On 结束处落 `phase=completed`。所以 Off/On 总量 489
> 请按 `phase=completed` 计数（`terminal_result=passed` 143 + `failed` 346）。
> `phase=requested` 只出现在 `pnp_radio_recovery` 上。按不存在的标记统计会得到 0。

| 组 | 样本 | 恢复 | 恢复率 |
| --- | --- | --- | --- |
| 实验组：Off/On 之后**首次**重连 | 488 | 3 | **0.61%** |
| 对照组：同事件内普通重试（`attempt>=1`） | 2406 | 15 | **0.62%** |
| （参考）进程冷启动 `attempt=0` | 89 | 26 | 29.21% |
| （参考）Off/On 自身报 `failed` | 345 | 0 | 0.00% |
| （参考）Off/On 自身报 `passed` | 143 | 3 | 2.10% |

**统计检验（2026-09-16 补充，防止过度解读）**：

- 双比例 z 检验：**z = -0.022，双尾 p = 0.982** → 两组无统计显著差异。
- 95% Wilson 置信区间：实验组 **0.21% – 1.79%**，对照组 **0.38% – 1.03%**，区间大幅重叠。
- 检出"开关组高 10 个百分点"（0.62% → 10.62%）所需每组样本量约 **80**，当前 488 条**足够**。

**引用时的准确措辞**：可以说「僵死态下 Off/On 没有可观测的收益，不值得保留一条会
打断链路的路径」；**不要**说成「Off/On 的效果完全为零」——实验组 CI 上限 1.79%，
严格讲只能说"收益不超过约 2%"。对当前决策（删掉这条路径）这个上限已经足够低，
但它不是"零效果"的普适证明。

**关键旁证**：Off/On 自身报告成功 143 次，其后也只恢复 3 次。说明 Off/On 命中的是
启动时预热缓存的 Radio 对象——**"WinRT 说开关成功"不等于"碰到蓝牙栈"**。这正是
`ATTRIBUTION.md` 里"系统栈健康时开关有效"在僵死态不成立的原因。

## 代码现状（已改）

| 位置 | 行为 |
| --- | --- |
| `bluetooth_radio::is_stack_exhausted()` | 按错误码判定僵死态，只认 `windows_resource_exhausted` / `winrt_operation_aborted` |
| `ble.rs` 恢复分支 | 命中僵死码 → `action=skip_recovery reason=stack_exhausted_proven_ineffective`，**跳过 Off/On 和 PnP 重启**，只留普通重连 |
| `ble.rs` 未命中 | 仍走原 Off/On，保留非僵死故障的兜底 |
| `ble.rs::ble_error_code` | 新增 `0x80004004`（已中止操作）→ `winrt_operation_aborted` |
| 用户可见文案 | 僵死态不再提示"请重启电脑以恢复蓝牙"，改为"蓝牙链路暂时不可用，正在持续重试…" |

> 注：上面 `ble_recovery_decision ...` 是本次引入的决策日志标记。分流之前的版本只有
> `ble_radio_recovery phase=requested|completed`，那个标记仍在非僵死路径上保留
> （`phase=requested` 只在实际要执行 Off/On 时写），脚本两套都能解析。

单测：`ble::tests::aborted_operation_is_classified_as_the_same_wedged_state`、
`bluetooth_radio::tests::exhausted_stack_is_recognised_and_everything_else_is_not`、
`bluetooth_radio::tests::window_advances_only_when_a_cooldown_reopens_the_budget`
—— `cargo test -p sayall-windows --lib` 116 passed / 0 failed（2026-09-16 在本分支实测）。

## 下次怎么查（三条命令出结论）

1. **取日志**：应用「设置 → 关于 → 打开日志目录」，或直接在
   `%LOCALAPPDATA%` 下 SayAll 的 GATT 日志目录，拷出 `SAYALL_GATT_LOG` 产物
   （`.log` 或 `.txt` 都行）。
2. **跑脚本**（给文件或整个目录都可以，目录会递归扫 `.log`/`.txt`）：

   ```bash
   python scripts/analyze-radio-recovery-ab.py <日志文件或目录>
   # 例：python scripts/analyze-radio-recovery-ab.py artifacts/ev_stream_raw.txt
   ```

3. **只看最后一行**：脚本只会输出四种结论之一，不要自己算。

## 判读标准（脚本内置）

| 条件 | 输出 |
| --- | --- |
| 实验组样本 < 30 | **样本不足**——这段日志几乎没有执行过 Off/On |
| 两组样本都 < 30 | **样本不足**——并说明可能原因（见下） |
| 对照组 ≥50% 是僵死码样本 | **不可判定**——两组不同质，见下节（脚本会打 ⚠️ 并直接返回） |
| \|实验组 − 对照组\| < 10 个百分点 | **无效**（无可观测边际价值） |
| 实验组高 ≥ 10 个百分点 | **有效**，保留 Off/On |
| 实验组低 ≥ 10 个百分点 | **无效且有害**（开关打断链路），删除 |

脚本同时打印两组的 **95% Wilson 置信区间**。若上限 > 10 个百分点，会额外警告
"样本不足以排除 10% 左右的真实收益"——此时结论仍是"无效"，但不得引用成"零效果"。

**对照组有两个口径，脚本自动选**：

| 口径 | 何时用 | 为什么 |
| --- | --- | --- |
| `skip_recovery`（优先） | 日志里有 `ble_recovery_decision` 标记（2026-09-16 之后的版本） | 与实验组处在**同一个决策时刻**，可比性最好 |
| `no_cycle` / `attempt>=1`（兜底） | 旧版日志，没有决策标记 | 用同一次僵死事件里没有紧接 Off/On 的重连尝试 |

脚本会打印「本次对照组口径：…」，看到哪一行就知道用的是哪个口径。

**两套标记都能吃**：新版是 `ble_recovery_decision action=radio_cycle|skip_recovery`，
旧版只有 `ble_radio_recovery phase=completed terminal_result=passed|failed`
（脚本会按 Off/On 自身是否执行成功再拆一层）。

**对照组为什么排除 `attempt=0`**：进程冷启动的首次连接成功率 29.21%，那是"健康态
第一次连"，不是"僵死中继续挣扎"。混进对照组会人为抬高基线，让 Off/On 显得有用。
脚本把这一档单列出来标注"不参与对照"，就是为了防止下次误读。

**为什么按 pid 配对**：多个进程会往同一个日志文件里写，不按 pid 配对会把 A 进程的
开关错配给 B 进程的重连结果。

## 脚本自检（2026-09-16 已跑过）

用合成日志验过四个分支，结论分别是：两组同为 50% → **无效**；
实验组 80% vs 对照 10% → **有效**；只有实验组 → **样本不足**并附说明；
对照组全为僵死码 → **不可判定**并打 ⚠️。
改脚本正则后请把这四个分支重跑一遍再信结论。

## ⚠️ 分流上线后，这个 A/B 就不再自动成立（重要）

分流生效后，僵死码 → `skip_recovery`、非僵死码 → `radio_cycle`。此时脚本给出的
「实验组 vs 对照组」**不再是干净的 A/B**：

- 两组样本天然不同质——分到哪一组由错误码决定，而错误码本身决定可恢复性；
- 僵死态只有重启能恢复，所以 `skip_recovery` 组会**永远**显得更差；
- 拿这两组比出来的"Off/On 有效"是**假阳性**，不能作为恢复 Off/On 的依据。

因此：

- **问「僵死态 Off/On 有没有用」**→ 直接引用本节表格（0.61% vs 0.62%）。
  这个结论来自 2026-09-16 之前的日志，对照组是同一僵死事件内的普通重试，口径干净，
  且分流上线后僵死态不再产生新的 `radio_cycle` 样本，不会有新数据推翻它。
- **问「非僵死码场景 Off/On 有没有用」**→ 需要**同一错误码内部**的对照，
  现有代码给不出（所有非僵死码都走同一条路）。要做就必须在
  `bluetooth_radio` 里加一个诊断开关，让同一错误码下随机/交替决定是否执行
  Off/On，跑一段时间再算。这是改代码的事，不是改脚本能解决的。

## 结论什么情况下会翻转

| 情况 | 处理 |
| --- | --- |
| 换了蓝牙适配器/驱动版本，想重新评估 | 用**诊断开关**做同错误码内对照，两组各 ≥30 条且实验组高出 ≥10 个百分点才算有效 |
| 只想复用现有分流数据 | 不算数——分组由错误码决定，见上节 |
| 判定为有效 | 改 `is_stack_exhausted` 的错误码白名单即可恢复开关路径，脚本不用改 |
