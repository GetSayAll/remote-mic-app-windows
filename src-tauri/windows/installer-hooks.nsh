!include WinVer.nsh

!define SAYALL_MINIMUM_WINDOWS_BUILD 17763

!macro NSIS_HOOK_PREINSTALL
  ${IfNot} ${AtLeastBuild} ${SAYALL_MINIMUM_WINDOWS_BUILD}
    MessageBox MB_ICONSTOP|MB_OK "无线麦 SayAll 需要 Windows 10 1809（内部版本 17763）或更高版本。$\r$\nSayAll requires Windows 10 1809 (build 17763) or later."
    SetErrorLevel 1633
    Quit
  ${EndIf}
!macroend
