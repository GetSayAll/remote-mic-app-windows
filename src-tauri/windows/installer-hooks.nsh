!include WinVer.nsh

!define SAYALL_MINIMUM_WINDOWS_BUILD 17763
!define SAYALL_DOWNGRADE_ERROR_LEVEL 1638
!define SAYALL_VB_CABLE_SERVICE_KEY "SYSTEM\CurrentControlSet\Services\VBAudioVACMME"
!define SAYALL_VB_CABLE_DOWNLOAD_URL "https://vb-audio.com/Cable/"

; ── 部署前先请应用优雅退出（2026-09-16）───────────────────────────────
;
; 背景：Tauri 默认模板的 `CheckIfAppIsRunning`（utils.nsh）在检测到应用正在
; 运行时**直接 `TerminateProcess`**（`nsis_tauri_utils::KillProcessCurrentUser`），
; 没有优雅退出请求、静默安装（`IfSilent`）连提示都没有，杀完只 `Sleep 500`。
;
; 而应用在持有活动 BLE GATT 会话时被强杀，会留下未正常关闭的会话，使系统
; 蓝牙栈进入僵死态：此后所有 WinRT 入口（`GetRadiosAsync`、设备查询、
; `FromBluetoothAddressAsync`）一律返回 `0x80070008`；应用内全部自动恢复手段
; （普通重连 / 无线电 Off/On / 提权 PnP 重启）与睡眠都无效，**只有重启电脑能恢复**
; （2026-09-16 现场逐项实测，见 Bugs/2026-09-16-ble-stack-resource-exhaustion-recovery-ineffective.md）。
; AGENTS.md 已把这条列为「部署不得强杀正在连接的应用」（2026-09-05 实证）。
;
; 本宏在 `NSIS_HOOK_PREINSTALL` / `NSIS_HOOK_PREUNINSTALL` 中执行，而 Tauri 的
; `CheckIfAppIsRunning` 在 `Section Install` 里**紧随其后**才跑。因此应用只要能
; 在这段宽限期内自行退出，后续检测自然落空、不会强杀；超时才落回原有行为。
;
; 信号用**会话内**命名事件：非提权进程没有 `SeCreateGlobalPrivilege`，无法创建
; `Global\` 命名对象；而安装器与应用同处一个登录会话，`Local\` 命名空间对两者
; 都可见。事件由应用在启动时创建（只有运行中的实例才持有句柄），所以
; `OpenEventW` 失败即表示"没有实例在运行"，直接跳过、零开销。
; 事件名必须与 crates/sayall-windows/src/graceful_exit.rs 的常量一致。
!define SAYALL_GRACEFUL_EXIT_EVENT "Local\SayAll-GracefulExit"
!define SAYALL_GRACEFUL_EXIT_SETTLE_MS 1500
!define SAYALL_GRACEFUL_EXIT_TAIL_MS 6500
!define SAYALL_EVENT_MODIFY_STATE 0x0002

!macro SayAllRequestGracefulExit
  Push $R8
  Push $R9
  ; 句柄返回值必须用 p（指针宽度）；用 i 在 x64 上会截断。
  System::Call 'kernel32::OpenEventW(i ${SAYALL_EVENT_MODIFY_STATE}, i 0, w "${SAYALL_GRACEFUL_EXIT_EVENT}") p .r8'
  ${If} $R8 != 0
    System::Call 'kernel32::SetEvent(p r8) i .r9'
    System::Call 'kernel32::CloseHandle(p r8)'
    ; 先固定静默一段时间，让应用关闭 GATT 会话并等 `ble_session_cleanup` 落盘。
    Sleep ${SAYALL_GRACEFUL_EXIT_SETTLE_MS}
    ; 若此时仍未退出，再补足剩余宽限（总计 SETTLE + TAIL）。
    ; 这里**刻意不用 Goto/标签轮询**：`${__LINE__}` 在安装器与卸载器两次汇编中
    ; 会展开成 `752.2.16` 这类复合 token，导致卸载段标签解析失败
    ; （2026-09-16 构建实测：could not resolve label "…_752.2.16" in uninstall section）。
    nsis_tauri_utils::FindProcessCurrentUser "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $R8
    ${If} $R8 != 0
      Sleep ${SAYALL_GRACEFUL_EXIT_TAIL_MS}
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
  !insertmacro SayAllRequestGracefulExit
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; 卸载同样不得强杀正在连接的应用（AGENTS.md 同一条规则）。
  !insertmacro SayAllRequestGracefulExit
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
