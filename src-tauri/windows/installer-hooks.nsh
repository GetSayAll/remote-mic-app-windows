!include WinVer.nsh

!define SAYALL_MINIMUM_WINDOWS_BUILD 17763
!define SAYALL_DOWNGRADE_ERROR_LEVEL 1638
!define SAYALL_VB_CABLE_SERVICE_KEY "SYSTEM\CurrentControlSet\Services\VBAudioVACMME"
!define SAYALL_VB_CABLE_DOWNLOAD_URL "https://vb-audio.com/Cable/"

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
