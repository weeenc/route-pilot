!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Installing the RoutePilot OpenVPN runtime..."
  ExecWait '"$SYSDIR\msiexec.exe" /i "$INSTDIR\openvpn-runtime.msi" /qn /norestart ADDLOCAL=OpenVPN,Drivers,Drivers.TAPWindows6,Drivers.OvpnDco' $0
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
  ${AndIfNot} ${FileExists} "$PROGRAMFILES\OpenVPN\bin\openvpn.exe"
    MessageBox MB_ICONSTOP|MB_OK "The OpenVPN runtime was not found after installation. The RoutePilot installation will stop."
    Abort
  ${EndIf}
!macroend
