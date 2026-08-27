<p align="center">
  <img src="./public/routepilot-icon.png" width="112" alt="RoutePilot 图标">
</p>

<h1 align="center">RoutePilot</h1>

<p align="center">
  一款专注于 macOS 与 Windows 的 OpenVPN 桌面客户端，基于 Tauri、Vue 和 Rust 构建。
</p>

<p align="center">
  <a href="./README.md">English</a> · <strong>简体中文</strong>
</p>

> [!IMPORTANT]
> RoutePilot 当前版本为 0.1.2。macOS 构建目前适合本地开发和测试；公开分发应用包前，
> 请先检查[发布清单](./RELEASE_CHECKLIST.md)。

## 功能特性

- 导入 `.ovpn` 配置，并将其引用的本地证书、密钥及其他资源复制到应用私有存储中。
- 独立运行和管理多个 VPN 连接。
- 实时查看连接状态、连接时长、上下行流量、隧道地址、服务器地址和活动路由。
- 检测不同活动 VPN 连接之间的路由重叠。
- 重命名配置、复制服务器地址，并可选择忽略服务器推送的默认路由。
- 通过系统托盘连接或断开；关闭主窗口后 RoutePilot 仍会驻留在托盘中。
- 随应用安装包自动配置 OpenVPN；自定义路径仅用于开发和排障。
- 支持英文与简体中文界面切换。
- 通过受限的 macOS 特权助手运行连接，仅首次启用时需要管理员授权。

## 平台状态

| 平台 | 状态 | 说明 |
| --- | --- | --- |
| macOS | 支持 | 使用随应用提供、由 root 管理的助手和 OpenVPN 运行时。 |
| Windows | 支持 | RoutePilot 安装包会自动配置 OpenVPN 2.x 及其网络驱动。 |
| Linux | 暂不支持 | 当前运行时代码会明确禁用未支持的平台路径。 |

CI 会在 macOS 和 Windows 上运行完整验证流程，并使用校验和固定的 OpenVPN 运行时构建
Windows NSIS 安装包。

## 技术栈

- [Tauri 2](https://v2.tauri.app/) 与 Rust：桌面运行时
- [Vue 3](https://vuejs.org/)、TypeScript、Pinia、Vue Router 与 Vue I18n：用户界面
- Vite：前端开发与构建；Vitest：前端测试
- 通过 OpenVPN Management Interface 管理 OpenVPN 2.x

## 环境要求

参与开发前请安装：

- Node.js 24（CI 使用的基准版本）
- pnpm 10.28.2（与 `package.json` 中的 `packageManager` 声明一致）
- Rust 1.77.2 或更高版本
- 当前操作系统对应的 [Tauri 系统依赖](https://v2.tauri.app/start/prerequisites/)
- 本地 VPN 开发需要 OpenVPN 2.x；正式安装包会配置自身固定版本的运行时

在 macOS 上，请安装开发运行时和打包脚本需要的原生依赖：

```sh
xcode-select --install
brew install openvpn lzo lz4 pkcs11-helper openssl@3
```

在 Windows 上，请安装 Microsoft C++ Build Tools，必要时安装 WebView2。只有运行开发版
时才需要安装 [OpenVPN Community Edition](https://openvpn.net/community/)。本地构建请使用
MSVC Rust 工具链。RoutePilot 启动时会请求管理员权限，以便 OpenVPN 配置虚拟网卡、DNS
和系统路由；请批准 Windows 的 UAC 提示。

## 快速开始

在项目根目录安装 JavaScript 依赖：

```sh
corepack enable
pnpm install --frozen-lockfile
```

### macOS

先准备本地 OpenVPN 运行时和特权助手，再启动桌面应用：

```sh
./scripts/prepare-macos-bundle.sh
pnpm tauri dev
```

进入**设置 → VPN 系统助手**，点击**启用一次**并批准管理员授权。此后连接将使用已安装的
受限助手，不再重复请求管理员密码。

准备脚本支持通过 `ROUTEPILOT_*` 环境变量自定义构建输入，具体说明见
[发布清单](./RELEASE_CHECKLIST.md)。

### Windows

开发版不会执行安装包中的运行时配置，因此请确保 `openvpn.exe` 已安装到常见目录或已加入
`PATH`，然后运行：

```powershell
pnpm tauri dev
```

运行开发版本前，请先以管理员身份启动终端。Windows 不允许未提权的开发进程直接启动
带有管理员清单的 RoutePilot 可执行文件。

如果自动检测未找到 OpenVPN，请在**设置 → OpenVPN 可执行文件**中填写其绝对路径。

### 仅预览前端

如需在不启动 Tauri 的情况下通过浏览器开发界面，可运行：

```sh
pnpm dev
```

由于缺少桌面运行时，此模式下配置导入、OpenVPN 检测和连接控制会被禁用。

## 构建

仅构建前端：

```sh
pnpm build
```

构建原生应用和平台安装包：

```sh
pnpm tauri build
```

建议在对应的目标操作系统上构建原生安装包。macOS 打包步骤会嵌入并签名选定的 OpenVPN
运行时。Windows 构建需要一个官方 OpenVPN Community MSI 及其预期 SHA-256：

```powershell
$env:ROUTEPILOT_OPENVPN_MSI = (Resolve-Path .\OpenVPN-runtime.msi).Path
$env:ROUTEPILOT_OPENVPN_MSI_SHA256 = (Get-FileHash $env:ROUTEPILOT_OPENVPN_MSI -Algorithm SHA256).Hash
pnpm tauri build
```

Windows 只生成 NSIS 安装包；如果标准 Program Files 目录中已有 OpenVPN，并且 TAP 或 DCO
驱动可用，安装程序会直接复用。否则，安装过程会静默配置 OpenVPN 核心和网络驱动，最终
用户无需再单独安装 OpenVPN。签名、第三方声明以及各平台发布要求见
[发布清单](./RELEASE_CHECKLIST.md)。

## 验证

运行与 CI 相同的检查：

```sh
pnpm verify
```

该命令会检查版本一致性、TypeScript 类型、前端测试、前端生产构建、Rust 格式、Clippy
以及 Rust 测试。

安装 `cargo-audit` 后可单独运行依赖安全审计：

```sh
cargo install cargo-audit --locked
pnpm run audit
```

常用的独立命令：

| 命令 | 用途 |
| --- | --- |
| `pnpm typecheck` | 检查 Vue 与 TypeScript 类型。 |
| `pnpm test:frontend` | 运行一次 Vitest。 |
| `pnpm format:check` | 检查 Rust 代码格式。 |
| `pnpm lint:rust` | 运行 Clippy，并将警告视为错误。 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 运行 Rust 测试。 |

## 项目结构

```text
route-pilot/
├── src/                    # Vue 应用、状态管理、API 绑定和国际化
├── src-tauri/              # Rust 运行时、OpenVPN 管理器、助手与打包配置
├── public/                 # 静态应用资源
├── scripts/                # 版本检查与平台打包准备脚本
├── .github/workflows/      # macOS/Windows 验证与依赖审计 CI
└── RELEASE_CHECKLIST.md    # 发布、签名与平台验证说明
```

## 配置数据与安全

导入的配置会被复制到 RoutePilot 的当前用户应用数据目录。在类 Unix 系统上，应用创建的
目录和文件采用受限权限。配置引用的本地资源会随标准化后的配置一同复制，因此原文件无需
一直保留在原位置。

在 macOS 上，特权助手只接受受约束的请求格式。它使用由 root 管理的运行时快照，并拒绝
任意脚本、插件、管理端点和进程控制选项等不安全的 OpenVPN 指令。

请将 `.ovpn` 文件及其引用的凭据视为敏感数据，不要提交真实 VPN 配置、私钥或签名凭据。

## 许可证

RoutePilot 基于 [MIT 许可证](./LICENSE)开放源代码。
