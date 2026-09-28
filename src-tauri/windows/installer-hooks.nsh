!include WinVer.nsh

!define SAYALL_MINIMUM_WINDOWS_BUILD 17763
!define SAYALL_DOWNGRADE_ERROR_LEVEL 1638
!define SAYALL_VB_CABLE_SERVICE_KEY "SYSTEM\CurrentControlSet\Services\VBAudioVACMME"
!define SAYALL_VB_CABLE_DOWNLOAD_URL "https://vb-audio.com/Cable/"

; ── 部署前先请应用优雅退出（2026-09-16）───────────────────────────────
;
; 背景：Tauri 默认模板的 `CheckIfAppIsRunning`（utils.nsh）在检测到应用正在
; 运行时不会给应用任何退出机会，而是直接强杀。**本仓库构建产物的实际分支**
; （原文见 `target/release/nsis/x64/utils.nsh`，构建时生成）：
;
;   nsis_tauri_utils::FindProcessCurrentUser → $R0 = 0 表示有实例在跑
;     IfSilent kill_...              ; 静默安装（/S）：不提示，直接杀
;     ${IfThen} $PassiveMode != 1 ${|} MessageBox MB_OKCANCEL ... ${|}
;         kill_...:  KillProcessCurrentUser + Sleep 500   ; 交互点"确定" → 强杀
;         cancel_...: Abort $R1                            ; 交互点"取消" → 安装中止
;
; 弹窗文案取自 SimpChinese.nsh：`{{product_name}} 正在运行！$\n点击确定以终止运行。`
; 也就是说：**交互安装时用户手里本来就有一次"安全选择"（取消 = 什么都不装、
; 不碰应用），只有点"确定"才会强杀**；静默/被动模式则没有这个选择。
;
; 而应用在持有活动 BLE GATT 会话时被强杀，会留下未正常关闭的会话，使系统
; 蓝牙栈进入僵死态：此后所有 WinRT 入口（`GetRadiosAsync`、设备查询、
; `FromBluetoothAddressAsync`）一律返回 `0x80070008`；应用内全部自动恢复手段
; （普通重连 / 无线电 Off/On / 提权 PnP 重启）与睡眠都无效，**只有重启电脑能恢复**
; （2026-09-16 现场逐项实测，见 Bugs/2026-09-16-ble-stack-resource-exhaustion-recovery-ineffective.md）。
; AGENTS.md 已把这条列为「部署不得强杀正在连接的应用」（2026-09-05 实证）。
;
; 注意（升级 CLI 时必须复核）：tauri `dev` 分支已把该宏改成走 Restart Manager
; （`RSTRTMGR::RmShutdown` + `RmForceShutdown`，交互取消同样是 `Abort`）。两种实现
; 都**不会**请求应用自行退出，因此本钩子对两者都成立；但升级 `@tauri-apps/cli` 后
; 必须重新查看生成的 `utils.nsh` 并重跑契约测试与端到端测试。
;
; 本宏在 `NSIS_HOOK_PREINSTALL` / `NSIS_HOOK_PREUNINSTALL` 中执行，而 Tauri 的
; `CheckIfAppIsRunning` 在 `Section Install` 里**紧随其后**才跑。因此应用只要能
; 在这段宽限期内自行退出，后续检测自然落空、连弹窗都不会出现；超时才落回原有行为。
;
; 信号用**会话内**命名事件：非提权进程没有 `SeCreateGlobalPrivilege`，无法创建
; `Global\` 命名对象；而安装器与应用同处一个登录会话，`Local\` 命名空间对两者
; 都可见。事件由应用在启动时创建（只有运行中的实例才持有句柄），因此
; `OpenEventW` 打不开 + 进程确实在跑 = 运行的是没有监听线程的旧版。
; 事件名必须与 crates/sayall-windows/src/graceful_exit.rs 的常量一致。
;
; 三种结局：
;   1. 没有实例在跑 → 零等待直接返回（最常见，也是应用内更新路径的常态）；
;   2. 有实例且能打开事件 → 置位 + 轮询等到它退出，`CheckIfAppIsRunning` 落空，
;      连弹窗都不会出现；
;   3. 有实例但打不开事件（旧版）→ 不杀也不装，中止并引导走应用内更新
;      （见宏内说明）。
!define SAYALL_GRACEFUL_EXIT_EVENT "Local\SayAll-GracefulExit"
!define SAYALL_GRACEFUL_EXIT_SETTLE_MS 1500
!define SAYALL_GRACEFUL_EXIT_POLL_INTERVAL_MS 500
; 轮询总预算必须 ≥ 应用侧 GRACEFUL_EXIT_TIMEOUT（5s）+ 退出开销，否则进程还没
; 退干净就轮到 Tauri 的 CheckIfAppIsRunning，弹窗必然出现（2026-09-16 实测）。
!define SAYALL_GRACEFUL_EXIT_MAX_WAIT_MS 20000
!define SAYALL_EVENT_MODIFY_STATE 0x0002
; 旧版（无监听线程）正在运行时的退出码：不装、不杀，交给应用内更新。
!define SAYALL_LEGACY_RUNNING_ERROR_LEVEL 1639

!macro SayAllRequestGracefulExit _uid
  Push $R8
  Push $R9
  ; 先确认是否真有实例在跑。`FindProcessCurrentUser` 的返回值语义由实测确定
  ; （0 = 在跑，1 = 不在跑；用 makensis 编译的最小探针跑出来的 ground truth，
  ; 见 artifacts/nsis-probe/）。**旧实现把这里写成 `!= 0`，语义正好反了**：
  ; 进程已退出时白等 6.5s，而进程还在跑（BLE 关闭最多要 5s）时反而不等，
  ; 直接落到 Tauri 的强杀弹窗——2026-09-16 用户现场看到的弹窗就是这个原因，
  ; 与"运行的是不是旧版本"无关。
  ; **必须传裸进程名，不能传全路径**：实测传 `"$INSTDIR\xxx.exe"` 时插件永远
  ; 返回 1（当作"没有在跑"），整个等待逻辑会被静默跳过（2026-09-16 探针实测，
  ; 见 artifacts/nsis-probe/sayall-findproc-probe2-result.txt）。Tauri 自己的
  ; `CheckIfAppIsRunning` 也是传裸名（installer.nsi 第 638 行）。
  nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
  Pop $R9
  ${If} $R9 = 0
    ; **寄存器大小写决定写进哪个变量**：`.R8` 写 `$R8`，`.r8` 写 `$8`（2026-09-16
    ; 探针实测，见 artifacts/nsis-probe/sayall-probe3-result.txt）。旧实现用 `.r8`
    ; 却判断 `$R8`，后者永远是空值，而空值 `!= 0` 在 NSIS 里为真——于是"事件存在"
    ; 这个分支恒真，旧版检测从来没生效过。事件不存在时输出的是字面 `0`。
    ; 句柄返回值必须用 p（指针宽度）；用 i 在 x64 上会截断。
    System::Call 'kernel32::OpenEventW(i ${SAYALL_EVENT_MODIFY_STATE}, i 0, w "${SAYALL_GRACEFUL_EXIT_EVENT}") p .R8'
    ${If} $R8 != 0
      System::Call 'kernel32::SetEvent(p R8) i .R9'
      System::Call 'kernel32::CloseHandle(p R8)'
      ; 先固定静默一段时间，让应用关闭 GATT 会话并等 `ble_session_cleanup` 落盘。
      Sleep ${SAYALL_GRACEFUL_EXIT_SETTLE_MS}
      ; 之后轮询到进程真正消失为止（预算耗尽才放弃），不再用"睡固定时长"。
      ; 标签后缀由调用方传入：`${__LINE__}` 在安装器与卸载器两次汇编中会展开成
      ; `752.2.16` 这类复合 token，导致卸载段标签解析失败（2026-09-16 构建实测）。
      StrCpy $R9 ${SAYALL_GRACEFUL_EXIT_MAX_WAIT_MS}
      sayall_wait_${_uid}:
        nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
        Pop $R8
        ${If} $R8 != 0
          Goto sayall_done_${_uid}
        ${EndIf}
        Sleep ${SAYALL_GRACEFUL_EXIT_POLL_INTERVAL_MS}
        IntOp $R9 $R9 - ${SAYALL_GRACEFUL_EXIT_POLL_INTERVAL_MS}
        ${If} $R9 > 0
          Goto sayall_wait_${_uid}
        ${EndIf}
      sayall_done_${_uid}:
    ${Else}
      ; 进程在跑但事件打不开 = 运行的是没有监听线程的旧版（0.2.10 及更早）。
      ; 两条路都不能走：
      ;   强杀 → 残留未关闭的 GATT 会话，蓝牙栈僵死，只有重启 Windows 能恢复
      ;           （2026-09-16 逐项实测，见 Bugs/2026-09-16-ble-stack-*.md）；
      ;   装下去 → 旧进程仍占着蓝牙链路，新版本连不上。
      ; 因此主动中止并引导走应用内更新：旧版的 `on_before_exit` 会先断开 BLE
      ; 再拉起安装器（tauri-plugin-updater 的 install_inner：on_before_exit 在
      ; ShellExecuteW 之前执行），既不重启也不需要用户手动退出应用。
      ${If} ${Silent}
        SetErrorLevel ${SAYALL_LEGACY_RUNNING_ERROR_LEVEL}
      ${Else}
        MessageBox MB_ICONINFORMATION|MB_OK "正在运行的无线麦是较早的版本，直接覆盖安装会中断蓝牙链路。$\r$\n请打开无线麦，在「设置」里点「检查更新」完成升级（应用会自己断开蓝牙并重启）。$\r$\n$\r$\nSayAll is running an older build. Please upgrade from Settings → Check for updates inside the app."
      ${EndIf}
      Abort
    ${EndIf}
  ${EndIf}
  Pop $R9
  Pop $R8
!macroend

!macro NSIS_HOOK_PREINSTALL
  ${IfNot} ${AtLeastBuild} ${SAYALL_MINIMUM_WINDOWS_BUILD}
    MessageBox MB_ICONSTOP|MB_OK "无线麦 SayAll 需要 Windows 10 1809（内部版本 17763）或更高版本。$\r$\nSayAll requires Windows 10 1809 (build 17763) or later."
    SetErrorLevel 1633
    Quit
  ${EndIf}

  Push $R8
  Push $R9
  ReadRegStr $R8 SHCTX "${UNINSTKEY}" "DisplayVersion"
  ${If} $R8 != ""
    nsis_tauri_utils::SemverCompare "${VERSION}" $R8
    Pop $R9
    ${If} $R9 = -1
      ${IfNot} ${Silent}
        MessageBox MB_ICONSTOP|MB_OK "已安装较新版本的无线麦 SayAll，不能用此旧版本覆盖。$\r$\nA newer version of SayAll is already installed. This older installer cannot replace it."
      ${EndIf}
      SetErrorLevel ${SAYALL_DOWNGRADE_ERROR_LEVEL}
      Quit
    ${EndIf}
  ${EndIf}
  Pop $R9
  Pop $R8

  ; 版本校验通过后才请正在运行的实例优雅退出：升级路径的关键一步。
  !insertmacro SayAllRequestGracefulExit install
  ; 升级覆盖写 sayall-helper.exe 前，必须让旧助手退出——否则
  ; 「无法打开要写入的文件」（2026-09-24 真机复现：跑了一天的旧助手
  ; 锁住 exe，主程序的优雅退出对它无效）。
  !insertmacro SayAllStopHelper install 0
  ; ── 授权语义（2026-09-28 Andy 拍板）：升级保留、卸载撤销、卸载后重装回落关闭 ──
  ; 开关的持久化意图在 AppSettings（app_config_dir），跨升级/重装幸存；
  ; 授权本体是提权创建的计划任务（普通权限删不掉，跨安装幸存）。撤销的
  ; 唯一凭证是卸载器写下的重授权标记——因此本钩子对标记的删除必须**有条件**：
  ;
  ;   * 交互升级：旧版卸载器（先于本钩子运行）以 revoke=1 写下标记；
  ;     该标记「刚刚」（秒级）写入，属于本次升级的中间产物——删除，
  ;     幸存的任务继续承载授权，开关保持原状态（2026-09-27 决策延续）。
  ;   * 卸载后重装：标记是用户手动卸载时写下的，年龄不可控——保留，
  ;     应用启动对账据此把开关回落为关闭，重开时强制重装任务（必弹 UAC）。
  ;
  ; 安装器在结构上**无法区分**这两种来源（交互升级与卸载后重装到达本钩子时
  ; 系统状态完全一致），唯一可用判据是标记的新鲜度。判据与实现：
  ;   * 卸载器写标记时把 `GetTickCount`（本次开机的毫秒数）写进内容；
  ;   * SayAllClearFreshReauthMarker 读回内容与当前 tick 相减：
  ;     0 ≤ 年龄 ≤ SAYALL_REAUTH_MARKER_FRESH_MS 视为「本次升级刚写的」→ 删除；
  ;   * 旧格式（字面 `reauth`，无 tick）只能来自旧版卸载器，而旧版卸载器只会
  ;     作为本次升级的旧卸载段出现 → 删除（与旧行为一致）；
  ;   * 跨重启（age 为负）/ 标记过期 → 保留（撤销生效）。
  ; 残留风险（已知并接受，TODO.md 同步）：卸载后 120 秒内完成重装，标记会被
  ; 误判为本次升级产物而删除、授权被保留——需用户手动关一次开关。该路径与
  ; 「升级」在系统状态上不可区分，120s 是升级钩子最坏时距（约 40s）与人为
  ; 快速重装之间的工程折中。
  !insertmacro SayAllClearFreshReauthMarker install
  ; 计划任务由提权进程创建，普通权限安装器删不掉它（删除命令静默失败，
  ; 任务本来就跨升级幸存）——升级恰恰要靠幸存的任务承载授权，helper 与
  ; 主程序同路径覆盖更新，幸存的任务指向的路径依然有效。安装路径不得
  ; 删任务。
!macroend

; ── 停止增强捕获助手并等它真正退出（2026-09-24）──────────────────────
;
; 为什么不能直接杀：助手是提权进程，普通权限安装器的 taskkill 必被拒；
; 唯一通道是让它自愿退出——schtasks /end（调度器有权终止任务实例）+
; 停用信号文件（路径必须与 rc003_task.rs 的 stop_signal_path 逐字符一致，
; 新助手每 50ms 轮询一次）。**跑了一整天以上的旧助手两种都不认识**
; （真机：up=27.8h 的实例锁住 exe，升级写文件必然失败），等待超时后
; 只能中止安装并请用户以管理员运行 helper 目录里的 stop-helper.cmd。
!define SAYALL_HELPER_EXIT_MAX_WAIT_MS 8000
!define SAYALL_HELPER_EXIT_POLL_MS 250

; ── 重授权标记的新鲜度窗口（2026-09-28）────────────────────────────
; 升级钩子（旧卸载器完成 → PREINSTALL）的真实时距是秒级、最坏约 40s
; （优雅退出 20s + 助手退出等待 8s + 文件清理）；窗口取 2 倍以上余量。
; 卸载后超过窗口的重装会把标记保留下来 → 撤销生效。
!define SAYALL_REAUTH_MARKER_FRESH_MS 120000

; 读取重授权标记的年龄。结果放在 $R9（**会覆盖 $R9**，调用方若需保留请自行
; Push/Pop），$R8 由本宏保存恢复。输出取值：
;   "fresh"   —— 标记在且内容是 tick、年龄在 [0, FRESH_MS] 内（只能是本次
;                升级的旧卸载器刚写的）；
;   "stale"   —— 标记在但已过期 / 跨重启（撤销生效，不得删除）；
;   "legacy"  —— 标记在但内容是旧格式字面 `reauth`（旧版卸载器所写，只可能
;                出现在本次升级的旧卸载段）；
;   "absent"  —— 标记不存在。
!macro SayAllReauthMarkerAge _uid
  Push $R8
  ClearErrors
  FileOpen $R8 "$LOCALAPPDATA\SayAll\rc003-reauth-required" r
  ${If} ${Errors}
    StrCpy $R9 "absent"
    Goto sayall_age_done_${_uid}
  ${EndIf}
  FileRead $R8 $R9
  FileClose $R8
  ${If} $R9 == "reauth"
    StrCpy $R9 "legacy"
    Goto sayall_age_done_${_uid}
  ${EndIf}
  ; 新格式：内容 = 卸载器写入时的 GetTickCount（毫秒）。IntOp 是带符号 32 位，
  ; 跨重启 / tick 回绕都会算出负年龄 → stale（撤销生效），方向安全。
  System::Call "kernel32::GetTickCount() i .R8"
  IntOp $R8 $R8 - $R9
  ${If} $R8 >= 0
  ${AndIf} $R8 <= ${SAYALL_REAUTH_MARKER_FRESH_MS}
    StrCpy $R9 "fresh"
  ${Else}
    StrCpy $R9 "stale"
  ${EndIf}
  sayall_age_done_${_uid}:
  Pop $R8
!macroend

; 升级路径专用：只删除「本次升级的旧卸载器刚写下」的重授权标记。
; 旧格式视为 fresh（等价于旧行为的无条件删除）；stale/absent 不动。
!macro SayAllClearFreshReauthMarker _uid
  Push $R9
  !insertmacro SayAllReauthMarkerAge ${_uid}clear
  ${If} $R9 == "fresh"
  ${OrIf} $R9 == "legacy"
    Delete "$LOCALAPPDATA\SayAll\rc003-reauth-required"
  ${EndIf}
  Pop $R9
!macroend

!macro SayAllStopHelper _uid _revoke_auth
  Push $R8
  Push $R9
  Push $0
  nsExec::Exec 'schtasks /end /f /tn "SayAll RC003 Helper"'
  CreateDirectory "$LOCALAPPDATA\SayAll"
  FileOpen $0 "$LOCALAPPDATA\SayAll\rc003-capture-stop" w
  ${If} $0 != 0
    FileWrite $0 "stop"
    FileClose $0
  ${EndIf}
  StrCpy $R9 ${SAYALL_HELPER_EXIT_MAX_WAIT_MS}
  sayall_helper_wait_${_uid}:
    nsis_tauri_utils::FindProcessCurrentUser "sayall-helper.exe"
    Pop $R8
    ${If} $R8 != 0
      Goto sayall_helper_done_${_uid}
    ${EndIf}
    Sleep ${SAYALL_HELPER_EXIT_POLL_MS}
    IntOp $R9 $R9 - ${SAYALL_HELPER_EXIT_POLL_MS}
    ${If} $R9 > 0
      Goto sayall_helper_wait_${_uid}
    ${EndIf}
  sayall_helper_done_${_uid}:
  ; 超时仍在跑（旧版助手不认识停用信号）：中止并给出可操作的出路。
  nsis_tauri_utils::FindProcessCurrentUser "sayall-helper.exe"
  Pop $R8
  ${If} $R8 = 0
    ${IfNot} ${Silent}
      MessageBox MB_ICONSTOP|MB_OK "增强捕获助手仍在运行且无法自动停止（可能是较早版本）。请以管理员身份运行安装目录旁 helper 目录中的 stop-helper.cmd，然后重新执行安装/卸载。$\r$\n$\r$\nThe RC003 helper is still running and cannot be stopped automatically. Run stop-helper.cmd as administrator, then retry."
    ${EndIf}
    Abort
  ${EndIf}
  ; 写「需重新授权」标记（**仅卸载路径**，_revoke_auth=1）：提权任务普通
  ; 权限删不掉（真机实测），卸载时这是「授权已应撤销」的唯一可靠凭证；
  ; 应用启动据此回落开关，下次开启强制重装任务（必弹 UAC）。升级路径
  ; （_revoke_auth=0）**不得**写标记——授权跨升级保留（2026-09-27 Andy
  ; 拍板），写了标记应用启动就会把用户已开启的开关打回关闭。路径与
  ; rc003_task.rs reauth_marker_path 逐字符一致（有测试钉住）。
  ; 内容写入**卸载时刻的 GetTickCount**：升级路径的 PREINSTALL 钩子据此
  ; 区分「本次升级的旧卸载器刚写的标记」（删除，授权保留）与「手动卸载
  ; 留下的标记」（保留，撤销生效）——见 PREINSTALL 的授权语义注释。
  !if ${_revoke_auth} == 1
    CreateDirectory "$LOCALAPPDATA\SayAll"
    System::Call "kernel32::GetTickCount() i .R8"
    FileOpen $0 "$LOCALAPPDATA\SayAll\rc003-reauth-required" w
    ${If} $0 != 0
      FileWrite $0 "$R8"
      FileClose $0
    ${EndIf}
  !endif
  Pop $0
  Pop $R9
  Pop $R8
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; 卸载同样不得强杀正在连接的应用（AGENTS.md 同一条规则）。
  !insertmacro SayAllRequestGracefulExit uninstall
  ; 卸载也要先停助手，再删它的文件与授权。
  !insertmacro SayAllStopHelper uninstall 1

  ; ── 授权不跨卸载保留（2026-09-24 产品决策）────────────────────────
  ; 卸载即撤销增强捕获的授权：删除计划任务，重装/升级后打开开关需要
  ; 重新走一次 UAC。任务由同用户的提权进程创建（owner=该用户），
  ; 非提权删除自己的任务通常被允许；即便个别环境拒绝，应用启动侧
  ; 还有"设置说开着但任务不存在 → 回落关闭"的对账兜底。
  nsExec::Exec 'schtasks /delete /f /tn "SayAll RC003 Helper"'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Push $R8
  ReadRegStr $R8 HKLM "${SAYALL_VB_CABLE_SERVICE_KEY}" "DisplayName"
  ${If} $R8 == ""
    ${IfNot} ${Silent}
      MessageBox MB_ICONINFORMATION|MB_YESNO "无线麦需要 VB-CABLE 把遥控器语音传给输入法和语音软件。VB-CABLE 由 VB-Audio 提供，属于 Donationware，安装需要管理员权限，完成后必须重启 Windows。$\r$\n$\r$\n是否现在打开 VB-CABLE 官方下载页面？$\r$\n$\r$\nSayAll requires VB-CABLE for speech input. Installation requires administrator permission and a Windows restart. Open the official download page now?" IDYES sayall_vb_cable_open IDNO sayall_vb_cable_done
sayall_vb_cable_open:
      ExecShell "open" "${SAYALL_VB_CABLE_DOWNLOAD_URL}"
sayall_vb_cable_done:
    ${EndIf}
  ${EndIf}
  Pop $R8
!macroend
