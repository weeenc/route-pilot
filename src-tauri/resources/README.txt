RoutePilot runtime resources

Optional bundled OpenVPN executables belong under:

  binaries/macos/openvpn
  binaries/windows/openvpn.exe
  binaries/linux/openvpn

macOS release builds populate the macOS directory. Windows release builds take
a checksum-pinned official MSI through ROUTEPILOT_OPENVPN_MSI and
ROUTEPILOT_OPENVPN_MSI_SHA256; the generated NSIS installer provisions the
OpenVPN core and network drivers automatically. RoutePilot also supports an
administrator-configured executable and standard system installation paths.
