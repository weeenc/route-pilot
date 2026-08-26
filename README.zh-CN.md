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
> RoutePilot 当前版本为 0.1.1。macOS 构建目前适合本地开发和测试；公开分发应用包前，
> 请先检查[发布清单](./RELEASE_CHECKLIST.md)。

## 功能特性

- 导入 `.ovpn` 配置，并将其引用的本地证书、密钥及其他资源复制到应用私有存储中。
- 独立运行和管理多个 VPN 连接。
- 实时查看连接状态、连接时长、上下行流量、隧道地址、服务器地址和活动路由。
- 检测不同活动 VPN 连接之间的路由重叠。
- 重命名配置、复制服务器地址，并可选择忽略服务器推送的默认路由。
- 通过系统托盘连接或断开；关闭主窗口后 RoutePilot 仍会驻留在托盘中。
- 从应用内置资源、自定义路径、`PATH` 或常见安装目录中查找 OpenVPN。
- 支持英文与简体中文界面切换。
- 通过受限的 macOS 特权助手运行连接，仅首次启用时需要管理员授权。

## 平台状态

| 平台 | 状态 | 说明 |
| --- | --- | --- |
| macOS | 支持 | 使用随应用提供、由 root 管理的助手和 OpenVPN 运行时。 |
| Windows | 支持 | 使用已安装的 OpenVPN 2.x 可执行文件和平台标准权限。 |
| Linux | 暂不支持 | 当前运行时代码会明确禁用未支持的平台路径。 |

CI 会在 macOS 和 Windows 上运行完整验证流程。

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
- OpenVPN 2.x

在 macOS 上，请安装开发运行时和打包脚本需要的原生依赖：

```sh
xcode-select --install
brew install openvpn lzo lz4 pkcs11-helper openssl@3
```

在 Windows 上，请安装 Microsoft C++ Build Tools、必要时安装 WebView2，并安装
[OpenVPN Community Edition](https://openvpn.net/community/)。本地构建时请使用 MSVC
Rust 工具链。

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

确保 `openvpn.exe` 已安装到 OpenVPN 的常见目录，或已加入 `PATH`，然后运行：

```powershell
pnpm tauri dev
```

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
运行时；公开分发还需要 Apple Developer ID 签名、公证，以及
[发布清单](./RELEASE_CHECKLIST.md)中说明的助手注册方式调整。

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
