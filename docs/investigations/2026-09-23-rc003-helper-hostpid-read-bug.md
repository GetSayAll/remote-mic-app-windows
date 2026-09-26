# RC003 助手：真机首跑失败 —— `HostPid` 读取宽度写错，且错误信息断言了未经验证的原因

- 状态：**根因已定位、已修复、已用独立通道验证**（非提权下复现一致）；真机注入仍待重跑
- 日期：2026-09-23
- 前置：产品化 spike（提交 `76e3855` / `6c6c479` / `a0f4d9b`）与只读实验
  [`2026-09-23-rc003-hid-host-readonly-tap-result.md`](2026-09-23-rc003-hid-host-readonly-tap-result.md)

> 本文记录一次**真实花费了用户一轮提权运行**的失败，重点不在"修好了"，
> 而在两件事：**它为什么能躲过已有的自检与探针**，以及**错误信息为什么会指向错的结论**。

## 1. 现象

Andy 以管理员身份运行 `run-helper.ps1 -Mode dryrun`，提权成功、注册表可读，
但助手在第一步就退出（退出码 11）：

```
[ENV] elevated=true pid=38772 port=47831 ... 
[REG] instances_with_hostpid=0 rc003_instances=0
[STOP] 注册表里没有承载 RC003 的 WUDF 实例。设备未连接/未配对时该节点可能不存在——
       请先连接遥控器后重跑。
```

该提示要求用户去**连接遥控器**。但遥控器当时是连着的。

## 2. 先做阳性对照，再谈结论

同一时刻、同一台机器，跑既有的只读探针（`probes/wudf_host_probe.py`）：

```
带 WUDFDiagnosticInfo 的实例总数: 6
其中命中 RC003 硬件 token 的实例: 1
  instance=9&3aacf7b9&0&0055  HostPid=32684 (0x7fac)  exe=WUDFHost.exe
判定: HostPid=32684: exclusive_rc003_host
```

**6 vs 0。** 设备在、宿主在、PID 与只读实验时的 `32684` 一致。
所以"设备未连接"是错的，问题在助手里。这一步很关键：**若跳过对照，就会让用户去拔插遥控器**，
而故障根本不在那里。

## 3. 根因：把 `REG_QWORD` 当 4 字节读

助手**沿用探针的同款遍历逻辑**，逐层用带返回码的最小复现程序（`target/tmp/reg_debug2.rs`）定位：

```
打开 Device Parameters:        ok=208 fail=24
打开 WUDFDiagnosticInfo:       ok=6   fail=202   ← 与探针的 6 完全一致，下钻是对的
读 HostPid 返回码分布:          rc=234 (ERROR_MORE_DATA) × 6
```

`ERROR_MORE_DATA` 的含义是"给的缓冲区比这个值小"。再用 `null` 缓冲区问真实类型与长度：

```
en=BTHLEDevice  ty=11 need=8 bytes=[ac 7f 00 00 00 00 00 00] → QWORD=32684
```

**`HostPid` 是 `REG_QWORD`（类型 11，8 字节），不是 `REG_DWORD`。**
原实现按 `size_of::<u32>() = 4` 字节读，`RegQueryValueExW` 一律返回 `234`，
而 `reg_read_u32` 把**任何非 `ERROR_SUCCESS` 都当 `None`** 静默丢弃 ⇒
6 个节点全丢 ⇒ 计数 0 ⇒ 归因到"设备未连接"。

## 4. 为什么它躲过了已有的两道防线

这是本文最有价值的部分——两层原因都是**结构性**的，不是疏忽：

1. **Python 探针用的是类型无关 API。** `winreg.QueryValueEx` 会按真实类型自动转换后返回
   `int`，调用方根本无从感知宽度。所以**探针能跑通 ≠ 4 字节读法可用**。
   参考实现里从来没有"宽度"这个约束被编码过，抄逻辑时也就抄不到。
   → 这是"**参考实现能跑通 ≠ 我的原语正确**"的一个新实例。
2. **错误信息断言了一个它没有验证过的原因。** 计数为 0 时直接写"设备未连接/未配对"。
   它没有区分"没有诊断节点"／"有节点但读不出值"／"有宿主但不是 RC003"——
   而这三者里只有第三种才是设备问题。**错误提示是给人看的结论，不是给人猜的提示。**

## 5. 修复（提交见 §7）

| # | 改动 | 要点 |
| --- | --- | --- |
| 1 | `reg_read_u32` → `reg_read_host_pid` | 两步读：先用 `null` 缓冲区问**类型 + 所需字节数**，再按实际宽度读 |
| 2 | 新增纯函数 `decode_host_pid(ty, raw)` | 支持 `REG_DWORD`/`REG_QWORD`/`REG_BINARY`；类型或宽度不符一律**拒绝**，不猜 |
| 3 | `enum_hosts` 返回 `HostScan` | 带上 `diag_keys` / `no_host` / `failures` 三个诊断计数，让"0 个实例"可归因 |
| 4 | 归因三分支 | "无诊断节点"／"有节点但读不出（**本程序的问题**）"／"有宿主但不是 RC003（设备问题）" |
| 5 | 失败不再静默 | 每个读取失败都打 `[REG-WARN]` 行，含脱敏设备名与具体原因（类型/长度/返回码） |
| 6 | `--dry-run` 不再要求提权 | 它全是只读检查；正是这条无谓的 UAC 门把本次 bug 藏在了 UAC 后面（见 §6.2） |
| 7 | 日志改为**追加** + 带本地时间戳的分隔头 | 原先每轮启动即截断；本次我的 dry-run 就把 Andy 那轮失败的原始日志抹掉了 |

新增自检项（都是"自实现原语必须自检"的落地）：

- `decode_host_pid` 五例：QWORD 8 字节（本机实际形态）／DWORD 4 字节／BINARY 8 字节／
  BINARY 2 字节**必须拒绝**／`REG_SZ` **必须拒绝**（不得把字符串当数字）
- 实机 `Enum` 扫描（只读、免提权）：**凡存在的诊断节点，`HostPid` 必须读得出**。
  判据刻意不依赖 RC003 是否在场，这样自检在任何机器上都能过
- 本机时间戳形态与取值范围（自实现的格式化同样会"错得很像对的"）

## 6. 验证

### 6.1 三条互相独立的证据

| 证据 | 结果 |
| --- | --- |
| 助手自检 | **21/21 PASS**，退出码 0 → `evidence/helper_selftest.out` |
| 免提权 `--dry-run` 全链 | 退出码 0 → `evidence/helper_dryrun.out` |
| 既有只读探针（**独立实现、独立通道**） | `diag_keys=6`、`HostPid=32684`、`exclusive_rc003_host` |

免提权 `--dry-run` 的实测输出：

```
[REG] diag_keys=6 instances_with_hostpid=6 no_host=0 read_failures=0 rc003_instances=1
[HOST] pid=32684 exe=WUDFHost.exe image=?
[EXCLUSIVE] members=1 rc003_members=1 verdict=exclusive_rc003_host
[GADGET] src=...\helper\vendor\frida-gadget.dll
[VERIFY] size=23254016 sha256=350beb0e...687 expected=350beb0e...687
[DRY-RUN] 宿主定位、独占性核对与 Gadget 校验均通过 ... elevated=false
```

助手与探针**在互相不认识的两套实现里给出了同一个 PID**，这比自检自证强得多。

### 6.2 新增项对旧代码是有效回归

`decode_host_pid` 的五例与「实机 `Enum` 扫描」两项，跑在修复前的代码上**必然 FAIL**：
前者没有 QWORD 分支，后者会把 6 个节点全部记入 `failures`。
这正是"自检必须覆盖自实现原语的契约，而不是只覆盖它的存在"。

## 7. 残余与未做

- **真机注入仍未做**：`--observe` / 正式运行需要提权，必须 Andy 本人执行。
  在此之前，"三键端到端可用"依然不成立（口径见 `hardware/RC003/README.md` §5.5）。
- 本机 `image=?`：非提权时 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` 对
  session 0 的宿主取不到映像路径，会退化为 `?`；这是预期降级，不影响定位（PID 来自注册表）。
- `HostPid` 的类型/宽度目前由**本机实测**确定（6/6 为 QWORD）。
  若换机器出现新类型，`decode_host_pid` 会**拒绝并报 `[REG-WARN]`**，不会静默错——
  这是刻意选的失败方向。
