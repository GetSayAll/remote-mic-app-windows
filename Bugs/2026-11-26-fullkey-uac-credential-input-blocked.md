# 开启「全按键支持」时标准用户被要求输入管理员凭据，且用户名框无法输入（客户反馈）

- 发现日期：2026-11-26（客户反馈；本机为管理员账户，UAC 不要求凭据，未复现）
- 状态：**已修复（自动化 passed；真机 deferred）**——入口按账号类型门禁：
  标准用户不再弹出凭据式授权窗口；管理员路径不变
- 影响范围：Windows 10/11；客户机为 1.9.7、域内**标准用户**（截图
  `域: ROBAMBIO`）、RC003；「全按键支持」开启/重开流程
- 功能点：「全按键支持」→ `rc003_task::enable_capture` → `ShellExecuteExW(runas)`
  提权授权；报告层语音键合成（`0x003E → 0x00E6`）
- 现象：点开「全按键支持」后出现「用户账户控制」窗口，要求输入**管理员用户名和
  密码**；客户补充：**用户名框无法输入，密码框可以输入**
- 正常预期：标准用户不应被引导到一条注定失败的授权路径；界面应说明需要管理员账户

## 证据

### 1. 截图（客户，1.9.7）

- 窗口标题「用户账户控制」，正文「你要允许来自未知发布者的此应用对你的设备进行
  更改吗？」，程序 `sayall-helper.exe`，发布者「未知」；
- **要求凭据**：「若要继续，请输入管理员用户名和密码。」`域: ROBAMBIO`；
- 与代码路径一致：`task_install_elevated` 以 `runas` 直接拉起助手本体
  `--install-task --hide-window`（`src-tauri/src/rc003_task.rs:233-296`）。

### 2. 客户补充（2026-11-26）：只有用户名框不行，密码框正常

- 这条**推翻了"键盘被按住"的假设**：修饰键粘住会让**所有**输入框失效，
  不可能只坏用户名框。键盘、安全桌面输入链路都正常。
- 因此本次现象落在 **Windows 凭据窗口的用户名控件本身**（空框、无法键入），
  不是本应用的输入层。该窗口在安全桌面上，我们的钩子与 SendInput 都到不了，
  也不应该向它注入凭据。
- 能控制的只有一件事：**不要让用户走到这个窗口**。

### 3. 账号形态决定窗口形态（已核对）

- 管理员（在本地 Administrators 组）：默认 `ConsentPromptBehaviorAdmin=5` →
  “是/否”同意框，不需要密码；`0` → 不弹；域策略改成 `1/3` → 才要求凭据。
- 标准用户：`ConsentPromptBehaviorUser=3`（默认）→ 凭据框，必须输入**另一个**
  管理员账号；本机实测注册表值见 `docs/investigations/2026-09-22-…:233` 同口径。

### 4. 即使输对凭据，功能也用不了（结构性问题）

- 提权助手以**被提权的管理员账户**身份运行并注册任务；任务 XML 无 `<UserId>`、
  `LogonType=InteractiveToken`（`hardware/RC003/helper/src/main.rs:5840`），
  `schtasks /create` 也不带 `/ru`（`:5877`）——任务只在**该管理员账户交互登录**
  时运行。
- 当前登录的是标准用户：`schtasks /run`（`src-tauri/src/rc003_task.rs:209`）
  没有可用的交互令牌，助手起不来；`enable_capture` 在触发成功后才落盘设置，
  开关回落关闭，界面上报失败。
- 任务的作者是那个管理员账户，标准用户既不能运行也不能删除它 → 残留一个
  永远跑不起来的任务，下次再点开关又弹一次凭据。

### 5. 已确认的代码事实（补充）

- LL 钩子/门控不能吞字母键：普通键直接透传（`key_gate.rs:797-803`），
  宿主任何失败都 fail-open（`key_host.rs:144-150`）。
- 合成键（报告层右 Alt）状态泄漏是**独立缺陷**（不是本次现象的原因，见 §2）：
  释放报告丢失时右 Alt 会一直按下（2026-10-23 实测 66 秒；
  `Bugs/2026-10-21-voice-session-stuck-right-alt-held.md:9-10` 现场“字母键变组合键”）。

## 已实施的修复

### A. 标准用户入口门禁（本次现象的直接处置）

- 新增 `crates/sayall-windows/src/user_account.rs`：只读当前进程令牌判形态
  （`TokenElevationType`：`Limited`=管理员过滤令牌、`Full`=已提权；
  `Default` 再读 `TokenElevation` 区分标准用户 / UAC 关闭的管理员）。
  **探测失败 fail-open**（`Unknown` 放行），不因一次 API 失败锁死功能。
- `capture_support_decision(arch, helper_found, authorization_allowed)` 新增
  第三种不可用原因 `admin_required`（`src-tauri/src/rc003_task.rs`）：
  架构 → 载荷 → 账号类型，顺序固定（先让用户换机器/重装，再谈账号）。
- 按键页 / 连接页 / 向导：开关置灰 + 说明「需要管理员账户登录后开启」，
  不再出现“这台电脑暂不支持”的误导；`enable_capture` 后端同样拒绝
  （防御过期 UI），错误文案可操作。
- 日志：`rc003 … action=enable phase=started elevation=standard_user`，
  一次日志即可归因。

### B. 合成键状态泄漏加固（独立缺陷，顺带修）

- 松手保险改为工具无关；右/左 Alt 分别判定并读回。
- `abort_voice_session`（静默看门狗/音频失败/入队拒绝/开始收尾失败）与
  `invalidate_connection`（断连/重连/睡眠）补发合成键抬起；`stream_replaced`
  除外（后面紧接新会话）。
- 授权前 / 关闭开关前 / 应用退出前各清一次（`release_stuck_synth_*`）；
  补发后仍按着且确属合成键时中止提权并给出可操作提示。

## 验证

- `cargo fmt --all -- --check`：passed。
- `cargo test -p sayall-windows --lib`：**643 passed / 0 failed / 22 ignored**
  （分支基线；含新增 `user_account` 三条、合成键判据）。
- `cargo test -p sayall-windows-app`：233 passed / 2 ignored；含
  `capture_support_reasons_are_stable_strings` 的 `admin_required` 断言。
- `cargo test --workspace`：**1012 passed / 0 failed**（分支基线）。
- `cargo check -p sayall-windows-app --features runtime-simulation`：passed。
- `pnpm test`：51 files / **761 passed / 10 skipped**（含新增的
  ButtonsPage / ConnectionPage `admin_required` 用例）；`pnpm build`：passed。
- **deferred（真机）**：标准用户机器上开关置灰、说明正确；管理员机器行为与
  1.9.7 完全一致（同意框 / 每次开启重新授权）。需 RC003 真机各一次。
- **deferred（Windows 凭据窗口）**：用户名框不可输入属操作系统窗口行为，
  本应用无法修复；若管理员被域策略强制要求凭据且用户名未预填，仍需 IT
  调整策略（不在本应用范围）。

## 边界

- 本轮只把标准用户从入口挡下，**没有**把助手改造成支持标准用户机器的形态
  （SYSTEM 任务/服务 + 跨会话注入）；那是另一条产品路线，需要单独设计、
  安全评审与真机验收。
- 门禁只影响「全按键支持」开关；基础语音、按键映射、回眸等路径不变。
- 管理员被策略要求凭据的机器仍允许开启（用户名通常预填）；本应用不向安全
  桌面注入任何内容。

## 隐私检查

只引用截图中的界面文案、版本号、域名字段与代码行；未包含用户名、蓝牙地址、
HID 路径、语音内容或凭据。
