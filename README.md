<p align="center">
  <img src="./public/routepilot-icon.png" width="112" alt="RoutePilot icon">
</p>

<h1 align="center">RoutePilot</h1>

<p align="center">
  A focused OpenVPN desktop client for macOS and Windows, built with Tauri, Vue, and Rust.
</p>

<p align="center">
  <strong>English</strong> · <a href="./README.zh-CN.md">简体中文</a>
</p>

> [!IMPORTANT]
> RoutePilot is currently at version 0.1.1. macOS builds are suitable for local
> development and testing; review the [release checklist](./RELEASE_CHECKLIST.md)
> before distributing an application bundle publicly.

## Features

- Import `.ovpn` profiles and their referenced local certificates, keys, and
  other assets into private application storage.
- Run and manage multiple VPN connections independently.
- See live connection state, duration, traffic totals, tunnel address, server
  address, and active routes.
- Detect overlapping routes across active VPN connections.
- Rename profiles, copy server addresses, and optionally ignore a server-pushed
  default route.
- Connect and disconnect from the system tray; closing the main window keeps
  RoutePilot available in the tray.
- Locate OpenVPN from the app bundle, a custom path, `PATH`, or common install
  locations.
- Switch between English and Simplified Chinese interfaces.
- Use a restricted macOS privileged helper so administrator approval is needed
  once instead of for every connection.

## Platform status

| Platform | Status | Notes |
| --- | --- | --- |
| macOS | Supported | Uses the bundled, root-owned helper and OpenVPN runtime. |
| Windows | Supported | Uses an installed OpenVPN 2.x executable with administrator privileges. |
| Linux | Not supported | The current runtime explicitly disables unsupported platform paths. |

CI runs the full verification suite on both macOS and Windows.

## Technology stack

- [Tauri 2](https://v2.tauri.app/) and Rust for the desktop runtime
- [Vue 3](https://vuejs.org/), TypeScript, Pinia, Vue Router, and Vue I18n for
  the interface
- Vite for frontend development and Vitest for frontend tests
- OpenVPN 2.x through its Management Interface

## Prerequisites

Install the following before working on the project:

- Node.js 24 (the CI baseline)
- pnpm 10.28.2, as declared by `packageManager` in `package.json`
- Rust 1.77.2 or newer
- The [Tauri system prerequisites](https://v2.tauri.app/start/prerequisites/)
  for your operating system
- OpenVPN 2.x

On macOS, install the native dependencies used by the development runtime and
bundle preparation script:

```sh
xcode-select --install
brew install openvpn lzo lz4 pkcs11-helper openssl@3
```

On Windows, install the Microsoft C++ Build Tools, WebView2 when required, and
the [OpenVPN Community Edition](https://openvpn.net/community/). Use the MSVC
Rust toolchain when building locally. RoutePilot requests administrator
privileges at startup so OpenVPN can configure its virtual adapter, DNS, and
system routes; approve the Windows UAC prompt.

## Getting started

Install JavaScript dependencies from the project root:

```sh
corepack enable
pnpm install --frozen-lockfile
```

### macOS

Prepare the local OpenVPN runtime and privileged helper, then start the desktop
application:

```sh
./scripts/prepare-macos-bundle.sh
pnpm tauri dev
```

In **Settings → VPN system helper**, select **Enable once** and approve the
administrator prompt. Future connections use the installed restricted helper
without prompting again.

The preparation script supports custom build inputs through the
`ROUTEPILOT_*` environment variables documented in the
[release checklist](./RELEASE_CHECKLIST.md).

### Windows

Make sure `openvpn.exe` is installed in a standard OpenVPN location or available
on `PATH`, then run:

```powershell
pnpm tauri dev
```

Start the terminal as an administrator before running the development build.
Windows does not allow an unelevated development process to directly launch the
RoutePilot executable after it has been marked as requiring administrator access.

If automatic detection does not find it, set the absolute executable path under
**Settings → OpenVPN executable**.

### Frontend-only preview

To work on the interface in a browser without launching Tauri:

```sh
pnpm dev
```

Profile import, OpenVPN discovery, and connection controls are disabled in this
mode because they require the desktop runtime.

## Build

Build only the frontend:

```sh
pnpm build
```

Build the native application and platform installers:

```sh
pnpm tauri build
```

Native installers should be built on their target operating system. The macOS
bundle step embeds and signs the selected OpenVPN runtime; public distribution
also requires an Apple Developer ID signature, notarization, and the helper
registration changes described in the [release checklist](./RELEASE_CHECKLIST.md).

## Verification

Run the same checks used by CI:

```sh
pnpm verify
```

This checks version consistency, TypeScript types, frontend tests, the frontend
production build, Rust formatting, Clippy, and Rust tests.

Run dependency audits separately after installing `cargo-audit`:

```sh
cargo install cargo-audit --locked
pnpm run audit
```

Useful individual commands:

| Command | Purpose |
| --- | --- |
| `pnpm typecheck` | Check Vue and TypeScript types. |
| `pnpm test:frontend` | Run Vitest once. |
| `pnpm format:check` | Check Rust formatting. |
| `pnpm lint:rust` | Run Clippy with warnings treated as errors. |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Run Rust tests. |

## Project structure

```text
route-pilot/
├── src/                    # Vue application, stores, API bindings, and i18n
├── src-tauri/              # Rust runtime, OpenVPN manager, helper, and packaging
├── public/                 # Static application assets
├── scripts/                # Version checks and platform bundle preparation
├── .github/workflows/      # macOS/Windows verification and dependency audit CI
└── RELEASE_CHECKLIST.md    # Release, signing, and platform validation notes
```

## Profile data and security

Imported profiles are copied into RoutePilot's per-user application data
directory. On Unix-like systems, app-owned directories and files are created
with restrictive permissions. Referenced local assets are copied alongside the
normalized profile so the original files do not need to remain in place.

On macOS, the privileged helper accepts only a constrained request format. It
uses a root-owned runtime snapshot and rejects unsafe OpenVPN directives such as
arbitrary scripts, plugins, management endpoints, and process-control options.

Treat `.ovpn` files and their referenced credentials as sensitive material. Do
not commit real VPN profiles, private keys, or signing credentials.

## License

RoutePilot is available under the [MIT License](./LICENSE).
