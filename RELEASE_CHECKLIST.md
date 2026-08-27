# RoutePilot v0.1.2 release checklist

## Repeatable verification

Run from the project root:

```sh
pnpm verify
pnpm audit --prod
cargo audit --file src-tauri/Cargo.lock
pnpm tauri build
```

The macOS build produces:

- `src-tauri/target/release/bundle/macos/RoutePilot.app`
- `src-tauri/target/release/bundle/dmg/RoutePilot_0.1.2_aarch64.dmg`

Verify the generated artifacts:

```sh
codesign --verify --deep --strict --verbose=2 \
  src-tauri/target/release/bundle/macos/RoutePilot.app
hdiutil verify \
  src-tauri/target/release/bundle/dmg/RoutePilot_0.1.2_aarch64.dmg
```

## OpenVPN runtime and macOS helper

The macOS bundle preparation script copies the selected OpenVPN executable and
its required dynamic libraries into `Contents/Resources/binaries/macos`,
rewrites their load paths, and signs the sealed runtime. Confirm and ship all
licenses and corresponding-source obligations before distributing that bundle.
It discovers dependencies through `brew --prefix`, so both Apple Silicon and
Intel Homebrew layouts are supported. Reproducible builds can override inputs
with `ROUTEPILOT_OPENVPN_SOURCE`, `ROUTEPILOT_LZO_LIB_DIR`,
`ROUTEPILOT_LZ4_LIB_DIR`, `ROUTEPILOT_PKCS11_LIB_DIR`,
`ROUTEPILOT_OPENSSL_LIB_DIR`, `ROUTEPILOT_CODESIGN_IDENTITY`, and
`ROUTEPILOT_TARGET_ARCH`.

On first use, Settings offers **Enable once**. macOS asks for administrator
authorization while RoutePilot installs these root-owned files:

- `/Library/PrivilegedHelperTools/com.routepilot.client.helper`
- `/Library/PrivilegedHelperTools/com.routepilot.client.runtime`
- `/Library/LaunchDaemons/com.routepilot.client.helper.plist`

Later connections use the installed helper and do not prompt again. Updating or
repairing the helper requires the one-time authorization again.

The helper accepts no executable or arbitrary configuration path over IPC. It
derives the requesting user's profile directory from peer credentials, rejects
script/plugin/management/process-control directives, copies the approved config
and referenced assets into a root-owned runtime snapshot, and stops OpenVPN if
the owning control connection closes.

## Signing and notarization

Local macOS builds use an ad-hoc signature so the application bundle has a
consistent sealed-resource signature. Public distribution requires replacing
the ad-hoc identity with an Apple Developer ID Application certificate and
completing Apple notarization.

For a public macOS 13+ release, migrate the locally installable LaunchDaemon to
Apple's `SMAppService` registration flow. Apple requires apps containing
LaunchDaemons registered this way to be code signed and notarized.

## Windows verification

The Windows Rust target alone is not sufficient on macOS. Cross-checking the
Tauri application also requires a Windows resource compiler such as `llvm-rc`.
For release candidates, build and test the NSIS installer on a Windows CI
runner with the MSVC and WebView2 prerequisites installed.

Download a signed OpenVPN Community MSI from the official release channel and
pin that exact artifact for the RoutePilot build:

```powershell
$env:ROUTEPILOT_OPENVPN_MSI = (Resolve-Path .\OpenVPN-runtime.msi).Path
$env:ROUTEPILOT_OPENVPN_MSI_SHA256 = (Get-FileHash $env:ROUTEPILOT_OPENVPN_MSI -Algorithm SHA256).Hash
pnpm tauri build
```

The bundle preparation step rejects a missing MSI, checksum mismatch, or invalid
Windows Authenticode signature. The NSIS installer reuses an OpenVPN executable
in a standard Program Files location when a TAP or DCO driver is present. If a
compatible installation is not found, it installs the `OpenVPN`, `Drivers`,
`Drivers.TAPWindows6`, and `Drivers.OvpnDco` MSI features without the
separate OpenVPN GUI. RoutePilot uses the resulting
`Program Files\OpenVPN\bin\openvpn.exe` through its existing runtime locator.

On a clean Windows VM with no prior OpenVPN installation, verify that:

- only the RoutePilot installer needs to be launched by the user;
- `Program Files\OpenVPN\bin\openvpn.exe` and a usable network driver are
  present afterward;
- the installer creates the persistent `RoutePilot TAP 1` and
  `RoutePilot TAP 2` adapter pool, and uninstall removes RoutePilot pool
  adapters without removing the shared OpenVPN runtime;
- two profiles can connect concurrently;
- TAP connections do not incur OpenVPN's legacy fixed five-second DHCP route
  delay and still wait for actual adapter readiness before adding routes;
- DNS and routes are removed after normal disconnect, forced process exit, and
  RoutePilot uninstall;
- installer exit codes `0`, `3010`, and `1638` complete successfully, while an
  actual OpenVPN MSI failure stops installation.

On a Windows VM with OpenVPN and a usable TAP or DCO driver already installed,
verify that the RoutePilot installer completes without launching the bundled
OpenVPN MSI or changing the existing OpenVPN installation.

RoutePilot does not automatically remove the shared OpenVPN runtime when it is
uninstalled, because another VPN client may be using it. Document this in the
release notes. Ship the required OpenVPN and bundled-library license notices and
meet all corresponding-source obligations.
