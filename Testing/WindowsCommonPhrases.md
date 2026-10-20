# 常用语真机验收（RC001 / RC003）

> 状态：**自动化 passed；真机验收 deferred**（2026-10-15）。
> 适用：含常用语能力的构建（`feat/common-phrases` 本地测试包；会员门禁见下）。
> 功能口径、分批评审与实施记录见 `docs/plan/2026-10-15-common-phrases-windows.md`。

## 要验什么

常用语是**会员能力**（Plus / Pro 可用）：把常用句绑定到遥控器五个位置（上 / 左 / OK / 右 / 下），
按键映射一个动作「打开常用语」；面板打开期间按方向键 / OK **按下即插入**（剪贴板 + Ctrl+V），
返回键关闭。面板**不抢焦点**（插入落在你打开面板前的输入框）。

## 前置条件

- RC001 或 RC003 已配对并连接；构建含常用语能力（私有组件已注入）。
- **启动方式（重要）**：走查构建的会员走查开关只在**带开关启动**时生效——
  直接双击图标启动 = 无会员，面板打不开（日志
  `map_fire ... action=open_common_phrases result=unavailable access=Unavailable`）。
  用交付目录里的「启动走查.cmd」（或桌面「无线麦 SayAll（常用语走查）」快捷方式）启动；
  应用已在运行时先退出再启动（单实例守卫不会把开关带给已运行的实例）。
- 某个非语音键已在按键页「常用语」分组映射「打开常用语」。
- 一个可粘贴的目标（记事本 / 微信输入框等），光标在其中。
- 可读取诊断日志（`COMMON_PHRASES` 标记；默认日志目录或 `SAYALL_GATT_LOG`）。

## 用例

**P1 · 打开与不抢焦点**
步骤：光标放进输入框 → 按映射键 → 面板出现（居中、不抢前台焦点）。
判据：日志 `PANEL phase=opened result=visible`；`PANEL phase=shown …` 行 `fg_before` / `fg_after`
一致且非本应用。副屏：面板落在光标所在屏。

**P2 · 逐键插入与剪贴板恢复**
步骤：先在别处复制一段文字（作为原剪贴板）→ 打开面板 → 上 / 左 / OK / 右 / 下 各插入一次。
判据：每次该位置的常用语立即进入输入框；五次之后**剪贴板恢复为原内容**。
日志：`INSERT … phase=completed result=submitted reason=paste_submitted`、`phase=clipboard result=restored`。

**P3 · 返回键关闭且不透传**
按返回 → 面板关闭；返回键不落进前台应用（浏览器不会后退、输入框不会删除字符）。

**P4 · 失败提示（面板重新显示 + 自动收尾）**
步骤：光标**不在**输入框（如点在桌面）→ 打开面板 → 选一条。
判据：面板重新出现并显示「光标不在输入框中，请把光标放进输入框后再试」，约 5 秒后自动关闭；
提示期内把光标放回输入框可直接重试成功。
日志：`FEEDBACK phase=requested reason=input_unavailable` →（重开）`PANEL phase=opened` →
`PANEL phase=closed reason=notice_timeout`。

**P5 · 断连 / 睡眠收尾（不粘键）**
步骤：面板打开期间断遥控器电源（或系统睡眠）→ 恢复。
判据：面板关闭、六个键立即恢复原映射；无幽灵单击 / 长按（按一次只响应一次）。

**P6 · 退出剪贴板恢复**
步骤：插入一条后**立刻**从托盘退出应用。
判据：重开应用检查剪贴板为原内容（不得停在常用语文本上）；
日志 `SHUTDOWN phase=completed result=services_stopped`。

**P7 · 物理键盘同吞（已接受口径）**
步骤：面板打开期间按物理键盘 Enter / 方向键 / Backspace。
判据：这些键被面板吞下（不透传给前台应用）——**已拍板接受**（2026-10-13 决策）；
鼠标点面板五个位置与遥控器按下等价（点选也插入）。

**P8 · 公开构建兼容（无私有组件）**
步骤：公开构建（未注入私有组件）打开按键页。
判据：无「常用语」动作分组；若配置里有「打开常用语」绑定，摘要显示
「打开常用语（当前不可用）」，页面不崩（不静默替换成别的动作）。

**P9 · 连续与快速**
连续插入、快速连按、面板开关后原映射逐键一致；插入后约 250 ms 恢复窗内用户自行复制，
剪贴板**不回滚**用户的新内容。

## 自动化覆盖（本机 passed，2026-10-15）

- 公开：`pnpm test` 372 passed（动作入口显隐、摘要「当前不可用」、认领配对、来源失效收尾等
  5 + 4 条新增断言）、`pnpm build`、`cargo test --workspace`、`scripts/ci-preflight.ps1` 7 步。
- 私有（注入公开宿主后）：`cargo test -p sayall-windows-common-phrases` 30 passed
  （失败反馈链、退出等在途事务、在不阻塞界面线程、队列中毒降级、门禁开关收紧等）。
- **真机 / 现场：deferred**——RC001 与 RC003 分别执行 P1–P9；结果回填本节。

## 日志速查（不落正文）

| 阶段 | 行 |
| --- | --- |
| 打开 | `COMMON_PHRASES PANEL phase=opened result=visible`；`phase=shown … fg_before/after` |
| 选中入队 | `INSERT operation_id=N phase=requested` |
| 终态 | `INSERT operation_id=N phase=completed result=submitted\|failed reason=… elapsed_ms=…` |
| 剪贴板 | `phase=clipboard result=restored\|user_copy_preserved\|failed` |
| 失败反馈 | `FEEDBACK phase=requested reason=<key>` |
| 关闭 | `PANEL phase=closed reason=selection\|back\|notice_timeout\|device_removed\|…` |
| 退出 | `SHUTDOWN phase=completed result=services_stopped` |
| 门禁 | `ACCESS phase=initial source=env result=enabled_dev_build\|ignored_release_build` |

隐私：常用语正文不进日志（只记操作 id、结果、原因与耗时）。
