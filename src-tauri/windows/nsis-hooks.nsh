!macro NSIS_HOOK_POSTINSTALL
  StrCpy $0 ""
  ${If} ${FileExists} "$PROGRAMFILES64\OpenVPN\bin\openvpn.exe"
    StrCpy $0 "$PROGRAMFILES64\OpenVPN\bin\openvpn.exe"
  ${ElseIf} ${FileExists} "$PROGRAMFILES32\OpenVPN\bin\openvpn.exe"
    StrCpy $0 "$PROGRAMFILES32\OpenVPN\bin\openvpn.exe"
  ${EndIf}

  ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\ovpn-dco" "ImagePath"
  ${If} $1 == ""
    ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\tap0901" "ImagePath"
  ${EndIf}

  ${If} $0 != ""
  ${AndIf} $1 != ""
    DetailPrint "Using the existing OpenVPN runtime at $0."
    Delete "$INSTDIR\openvpn-runtime.msi"
    Goto routepilot_openvpn_ready
  ${EndIf}

  DetailPrint "Installing the RoutePilot OpenVPN runtime..."
  nsExec::ExecToLog '"$SYSDIR\msiexec.exe" /i "$INSTDIR\openvpn-runtime.msi" /qn /norestart ADDLOCAL=OpenVPN,Drivers,Drivers.TAPWindows6,Drivers.OvpnDco'
  Pop $0
  Delete "$INSTDIR\openvpn-runtime.msi"

  ${If} $0 != 0
  ${AndIf} $0 != 3010
  ${AndIf} $0 != 1638
    MessageBox MB_ICONSTOP|MB_OK "RoutePilot could not install its OpenVPN runtime (Windows Installer error $0). The RoutePilot installation will stop."
    Abort
  ${EndIf}

  ${If} $0 = 3010
    SetRebootFlag true
  ${EndIf}

  ${IfNot} ${FileExists} "$PROGRAMFILES64\OpenVPN\bin\openvpn.exe"
  ${AndIfNot} ${FileExists} "$PROGRAMFILES32\OpenVPN\bin\openvpn.exe"
    MessageBox MB_ICONSTOP|MB_OK "The OpenVPN runtime was not found after installation. The RoutePilot installation will stop."
    Abort
  ${EndIf}

  ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\ovpn-dco" "ImagePath"
  ${If} $1 == ""
    ReadRegStr $1 HKLM "SYSTEM\CurrentControlSet\Services\tap0901" "ImagePath"
  ${EndIf}
  ${If} $1 == ""
    MessageBox MB_ICONSTOP|MB_OK "An OpenVPN network driver was not found after installation. The RoutePilot installation will stop."
    Abort
  ${EndIf}

  routepilot_openvpn_ready:
  StrCpy $2 ""
  ${If} ${FileExists} "$PROGRAMFILES64\OpenVPN\bin\tapctl.exe"
    StrCpy $2 "$PROGRAMFILES64\OpenVPN\bin\tapctl.exe"
  ${ElseIf} ${FileExists} "$PROGRAMFILES32\OpenVPN\bin\tapctl.exe"
    StrCpy $2 "$PROGRAMFILES32\OpenVPN\bin\tapctl.exe"
  ${EndIf}
  ${If} $2 != ""
    DetailPrint "Preparing the RoutePilot TAP adapter pool..."
    StrCpy $3 1
    routepilot_create_tap_loop:
      IntCmp $3 3 routepilot_create_tap_done
      nsExec::ExecToLog '"$2" create --name "RoutePilot TAP $3" --hwid tap0901'
      Pop $4
      IntOp $3 $3 + 1
      Goto routepilot_create_tap_loop
    routepilot_create_tap_done:
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $0 ""
  ${If} ${FileExists} "$PROGRAMFILES64\OpenVPN\bin\tapctl.exe"
    StrCpy $0 "$PROGRAMFILES64\OpenVPN\bin\tapctl.exe"
  ${ElseIf} ${FileExists} "$PROGRAMFILES32\OpenVPN\bin\tapctl.exe"
    StrCpy $0 "$PROGRAMFILES32\OpenVPN\bin\tapctl.exe"
  ${EndIf}
  ${If} $0 != ""
    DetailPrint "Removing the RoutePilot TAP adapter pool..."
    StrCpy $1 1
    routepilot_remove_tap_loop:
      IntCmp $1 17 routepilot_remove_tap_done
      nsExec::ExecToLog '"$0" delete "RoutePilot TAP $1"'
      Pop $2
      IntOp $1 $1 + 1
      Goto routepilot_remove_tap_loop
    routepilot_remove_tap_done:
  ${EndIf}
!macroend
