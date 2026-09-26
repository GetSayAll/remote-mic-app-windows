# 第二次运行被上一代 Gadget 挡住：根因、修法与一次性迁移（2026-09-23）

**状态**：根因已定位（两条独立证据 + 阳性对照），修复已实现并通过自检（34/34）。
**未验证**：修复后的真机行为（接管路径 / `[STALE-TAP]` 分流 / `--await-hello`）需要一次提权运行。
**一句话**：注入成功的代价是宿主**长期映射**运行时目录那份 `frida-gadget.dll`；
而助手的运行时目录准备是**无条件覆盖复制**，于是"第二次运行"必然在注入之前失败。

---

## 1. 现象

用户 2026-09-23 15:32 / 15:33 / 15:35 连跑三次，每次都在注入之前退出，报同样的错：

```
[STOP] 复制 Gadget 失败 D:\...\helper\vendor\frida-gadget.dll
       -> C:\ProgramData\SayAll\rc003-helper\frida-gadget.dll:
       另一个程序正在使用此文件，进程无法访问。 (os error 32)
```

原始日志：`hardware/RC003/evidence/helper-rerun-blocked.log`
（从 `helper-run.log` 第 695–726 行归档，保留三条完整运行记录）。

`os error 32` = `ERROR_SHARING_VIOLATION`，即"文件被别的进程占用着，写不进去"。

## 2. 为什么是必然的，而不是偶发

同一天 15:10:33 那次 `observe` 首跑**成功注入**：

```
[LISTEN] addr=127.0.0.1:47831
[INJECT] pid=32684 dll=C:\ProgramData\SayAll\rc003-helper\frida-gadget.dll wait=0x00000000 hmodule=0x645B0000 loaded=true
```

Windows 的语义是：**DLL 一旦被 `LoadLibrary` 进某个进程，那份 image section 就会一直占着文件**，
直到那个进程退出或卸载模块（"另一个程序正在使用此文件"就是这条）。于是：

| 事实 | 证据 |
| --- | --- |
| 宿主 `WUDFHost.exe`（pid=32684）启动于 `2026-09-23 01:23:26`，采样时已运行 **14.47 h** | `evidence/windows-restart-manager-probe.log`（Restart Manager 返回的进程启动时刻） |
| 运行时目录那份 DLL 正被 **pid=32684 `WUDFHost.exe`** 占用 | 同上（占用者列表） |
| 三次重跑都在同一行失败 | `evidence/helper-rerun-blocked.log` |
| 而 `prepare_runtime` 当时是**无条件** `fs::copy` | 修复前的 `helper/src/main.rs`；`[PREP]` 行只在复制之后才打印，失败时连它都没有 |

也就是说：**"宿主还映射着上一代 DLL" 是一个正常状态，却被当成了致命错误。**
宿主在这里是长寿进程（HID 宿主随设备节点存活，实测已 14 小时），
所以"跑第二次"本来就是最常见的使用方式。

## 3. 取证过程中的一个错误判据（必须记下来）

第一版探针想用"试开文件"判断是否被占用，写成了 `READ + share=NONE`：占用的 DLL 应该打不开。
实测**打开成功**，于是结论会变成"没被占用"——**与事实相反**。

救回来的是**阳性对照**：拿同一个探针去问 `kernel32.dll`（本进程自己就映射着它），
它同样返回 OK。于是判据被证伪，而不是"事实被确认"。

第二个坑：换成 `WRITE + share=NONE` 后拿到 `err=5`（`ERROR_ACCESS_DENIED`）——
这是 `C:\ProgramData\SayAll\...` 的 ACL（提权创建，普通用户无写权限）造成的，
**会掩盖真正的 `err=32`**。所以非提权状态下这条路根本得不出结论。

最终用 **Restart Manager API**（`RmStartSession`/`RmRegisterResources`/`RmGetList`）直接问系统要占用者名单：
不需要写权限，绕开了 ACL 与"读共享"两个坑，并且**自带阳性对照**
（默认先拿自己的 exe 问一遍，必须认出本进程 pid）。

两个探针都留在 `probes/` 里，第一个**故意保留错误判据**：
`windows-file-lock-probe.py`（含失败判据与对照组）、`windows-restart-manager-probe.py`（可用的那条）。

## 4. 修复：把状态显式化，而不是绕过

### 4.1 启动时枚举宿主模块，判断里面有没有我们的 Gadget

`enum_modules(pid)` 走 Toolhelp32（`TH32CS_SNAPMODULE`），`resident_taps()` 按基名
`frida-gadget*.dll` 过滤，日志打 `[TAP] resident=N` 与每条模块的路径（并核对路径是否落在
我们的运行时目录下——名字相同但路径在别处就是**别人**注入的）。

> **顺带修掉一个未经核实的断言**：旧代码用 `loaded = (LoadLibraryW 返回值 != 0)` 表示"注入成功"。
> 但模块已存在时 `LoadLibraryW` 返回的是**已加载模块的基址**（非 0），
> 所以"注入成功"和"什么都没发生"给出同一个值。现在改为注入后**再枚举一次模块**，
> 用"这个路径的模块是否真的在列表里"作为判据（`[VERIFY-MODULE] module_present=…`）。

### 4.2 令牌跨运行稳定 —— 让"接管"这件事成立

agent 脚本里本来就写了重连：

```js
/* 断线后重连：也覆盖"助手重启但注入未重做"的情形（DLL 已加载，
   LoadLibrary 不会重跑构造，agent 实例仍在，靠这条重连恢复）。 */
```

但**助手每次运行都重新生成令牌**，常驻 agent 手里的旧令牌必然被新助手当冒充者拒掉
（`[REJECT] reason=token_mismatch`），于是这条重连路径**在实践中永远走不通**。

修法：令牌落盘到运行时目录 `session.token`，默认复用（`--new-token` 才换）。
判据 `token_reusable = 磁盘上原本就有令牌文件`：只有本来就有，才可能和常驻 agent 手里那个相同。

### 4.3 接管路径 与 分流

```
宿主里已有我们的 Gadget?
├─ 有 + 令牌可接管 → [ATTACH] 不复制、不注入；鉴权后补发 arm/mode/restore
├─ 有 + 令牌接不上（本机制之前的旧世代）→ [STALE-TAP] + 清理步骤，退出码 13
│                                          （实验性退路 --new-generation）
└─ 无            → 复制（已存在且 SHA-256 一致则复用）+ 注入
```

三处细节，每一处漏掉都会变成"看起来在工作、实际没有"：

1. **必须补发 `arm`**。上一轮收尾时助手发过 `disarm`，而 agent 的 `disarmed` 只能被 `arm` 清除
   （`leaseOk()` 里 `if (disarmed ...) return false`）。漏发的话心跳、续约、握手全部正常，
   `[EDGE]` 却永远不出现——最容易被误读成"注入失败"。
2. **复制前先算摘要**。运行时目录是可被替换的，只看"文件存在"会把被换掉的 DLL 当成校验过的那一份。
   摘要一致 → `[PREP] action=reuse`，**一个字节都不写**。
   实测（免提权 `--dry-run` 的 `[PREP-SIM]`）：`dll_exists=true sha256_matches=true would_do=reuse_existing`。
3. **`mode` / `restore` 改为由助手下发**，而不是沿用 Gadget 加载时那份旧配置——
   否则接管之后想从 `observe` 切到 `clear` 会静默不生效。

### 4.4 `--await-hello`：不许把"失败"伪装成"正在工作"

新增 `--await-hello <SEC>`（默认 30，0=不限）：注入/接管后若一直没拿到**已鉴权**的 hello，
打 `[NO-HELLO]` 并按可能性列出排查方向，退出码 12。
判据用"这辈子有没有鉴权成功过"（`saw_authenticated`），不能用 `session.authenticated`
——后者会随连接重建被重置。

### 4.5 agent 侧：看门狗 + 鉴权失败退避

- **下行静默看门狗**：frida 17 实测"对端关闭后继续 write 不抛错"，所以断线**在 socket 层看不见**。
  但助手每 500 ms 发一次续约，于是"下行静默 > 3000 ms"就是可靠的断线判据
  （`RX_TIMEOUT_MS`）。**接管路径能不能真的连回来，靠的就是这一条。**
- **鉴权失败退避**：连上了却一直没有续约 ⇒ 对面不认我们 ⇒ 关掉连接，退避 15 s 再试
  （而不是每秒重连刷爆日志、并跟应该被接管的那一个实例抢连接）。
  助手侧也把 `[HELLO] auth=false` 折叠成计数器（前 3 次 + 之后每 30 次一条）。

## 5. 自检 23 → 34 项，其中一项当场抓到真 bug

新增：模块枚举（**阳性对照=本进程自身** / 阴性对照=无 Gadget）、Gadget 模块名判据、
放置策略决策表（5 例）、DLL 复用判定（3 例）、分代目录命名、令牌跨运行稳定、分代目录回收。

**第一次跑就 FAIL 了两项，两项都是真 bug**：

| 失败项 | 根因 | 为什么以前不会暴露 |
| --- | --- | --- |
| 模块枚举返回空列表，`GetLastError=24` | Toolhelp 的 `szModule` 是 **MAX_MODULE_NAME32+1 = 256**，不是 `MAX_PATH`(260)。写错让结构体大 8 字节 → `ERROR_BAD_LENGTH` | 若只有"没找到 Gadget"的阴性断言，空列表**正好符合预期**，会静默通过 |
| 分代目录名出现大写 `gen-9F3A1C7E` | `is_ascii_hexdigit()` 接受大写；`--token` 可以是任意字符串 | 只用 `{:032x}` 生成的令牌去测，永远是大写不出现 |

规则沉淀（已写进 `README` §4.3 与技能）：
**凡是"没找到 / 为 0 / 未发生"的结论，都必须附一条能返回"找到了"的阳性对照。**

## 6. 本机的一次性迁移（尚未执行）

15:10 那次注入的 Gadget 握着一个**未持久化**的随机令牌，属于"本机制引入之前的旧世代"：
接不上，也无法覆盖它的 DLL（正被宿主映射着）。所以修复后的第一次提权运行会打 `[STALE-TAP]`
并以 13 退出——**这是设计好的分流，不是新 bug**。

清理方式（任选其一）：

1. **断开并重新配对 RC003**（最干净：设备节点重建会带走整个 `WUDFHost` 进程）；
2. 设备管理器 → RC003 的 HID 设备 → 禁用再启用；
3. 重启系统。

确认已释放（应输出 `users = (none)`）：

```
python hardware/RC003/probes/windows-restart-manager-probe.py ^
       C:\ProgramData\SayAll\rc003-helper\frida-gadget.dll
```

清干净之后，之后的每次运行都会走"复制/复用 + 注入"，第二次及以后走"接管"。

> 本机 `pnputil /restart-device` 有"返回成功但设备没重启"的先例
> （`Bugs/2026-09-16-ble-stack-resource-exhaustion-recovery-ineffective.md`），
> 所以没有把它做成自动步骤；清理由用户按上面的方式做一次。

## 7. 残留风险与未验证项

| 项 | 状态 |
| --- | --- |
| 接管路径（`[ATTACH]`）真机端到端 | **未验证**。前提是 agent 侧重连真的能连回来；看门狗是为此新加的，但尚未在真机上观察过一次"助手退出→重开→接管成功" |
| `[STALE-TAP]` 分流真机行为 | **未验证**（下一次提权运行就会走到） |
| `--new-generation`（同宿主双实例） | **未验证**，依赖 Frida 是否允许同进程两个 Gadget 实例；失败时会由 `[NO-HELLO]` 明确报出 |
| 注入的 Gadget 常驻宿主直到宿主重启 | 仍成立（不可卸载，参考实现也选择"只重启独占节点"而非原地卸载）。多世代并存时会有多个实例常驻内存，旧实例已 disarm、不会清键 |
| 令牌落盘 | 威胁模型变化：本地进程不再需要猜令牌，改为"读文件即可"。该目录是提权创建的（普通用户不可写），可接受；仅本地回环使用 |

## 8. 复现与验收命令

```
# 免提权：自检（34 项）
hardware/RC003/helper/target/release/rc003-helper.exe --selftest

# 免提权：定位链 + 只读预告运行时目录会怎么处理
hardware/RC003/helper/target/release/rc003-helper.exe --dry-run

# 查出谁占着运行时目录那份 DLL（阳性对照默认开启）
python hardware/RC003/probes/windows-restart-manager-probe.py

# 提权（右键 run-helper-observe.cmd）：接管/注入 + 等待已鉴权 hello
```

退出码新增：`10` 令牌文件、`11` `--attach-only` 但没 tap、`12` `[NO-HELLO]`、`13` `[STALE-TAP]`。
