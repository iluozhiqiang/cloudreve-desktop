# Cloudreve File Provider（Xcode 工程）

本目录已包含可直接打开的 Xcode 工程：

- `CloudreveFileProvider.xcodeproj`
- 宿主：`CloudreveFPHost`（macOS App）
- 扩展：`CloudreveFPE`（File Provider Extension，已嵌入宿主 `PlugIns`）

## 1. 首次安装 Xcode 后（只需一次）

若命令行出现 `CoreSimulator.framework` / `runFirstLaunch` 相关错误，请在本机执行其一：

```bash
sudo xcodebuild -runFirstLaunch
```

或直接**打开一次 Xcode 图形界面**，让它跑完组件安装/许可/额外组件下载。

## 2. 命令行编译（不打开 Xcode 也行）

在 `macos-file-provider-extension/` 目录：

```bash
xcodebuild \
  -project CloudreveFileProvider.xcodeproj \
  -scheme CloudreveFPHost \
  -configuration Debug \
  -derivedDataPath ./build/DerivedData \
  build
```

或使用 target（不依赖 shared scheme）：

```bash
xcodebuild \
  -project CloudreveFileProvider.xcodeproj \
  -target CloudreveFPHost \
  -configuration Debug \
  -derivedDataPath ./build/DerivedData \
  build
```

产物路径通常在：

`build/DerivedData/Build/Products/Debug/CloudreveFPHost.app`

### Bundle Identifier（已按 Apple 规则对齐）

- 宿主：`com.cloudreve.desktop.fpehost`
- 扩展：**必须是宿主前缀** → `com.cloudreve.desktop.fpehost.fileprovider`  
  （否则会报：`Embedded binary's bundle identifier is not prefixed with the parent app's bundle identifier`）

## 3. 代码签名 / Team（File Provider **必须**，否则常见 -2001 / 底层 -2014）

工程里 `DEVELOPMENT_TEAM` 默认为空（便于提交到仓库）。**仅 ad-hoc 构建时，`NSFileProviderManager` 往往报 `ProviderNotFound`（-2001），底层为 `ApplicationExtensionNotFound`（-2014）**——系统无法把 `PlugIns/CloudreveFPE.appex` 当作合法扩展。

你需要在本机：

1. **Xcode** → **Settings** → **Accounts** → 登录 **Apple ID**（免费账号也有 Personal Team）。
2. 打开本工程 → **CloudreveFPHost** 与 **CloudreveFPE** → **Signing & Capabilities** → 为两者选择 **同一 Team**。
3. 命令行构建时传入 Team（任选其一）：
   - 环境变量：`DEVELOPMENT_TEAM=你的10位TeamID ./scripts/build_macos_fpe.sh`
   - 或创建 **`~/.cloudreve/xcode_development_team`**，文件内**仅一行** Team ID（无空格）；`build_macos_fpe.sh` 会自动读取。

Team ID 可在 [developer.apple.com](https://developer.apple.com/account) Membership 页查看，或在 Xcode 选中 Team 后在 **Build Settings** 里搜 `DEVELOPMENT_TEAM`。

### 3.0 变通：无签名构建 + 手动 `codesign`（不推荐 File Provider 使用）

**注意：** 该方式产物通常 **没有** `Contents/embedded.provisionprofile`，带 **App Groups** 时 **`NSFileProviderManager` 仍可能报 -2001 / -2014**。主流程请用 **`./scripts/build_macos_fpe.sh`**（`-allowProvisioningUpdates` + Xcode 自动签名）。

若命令行曾报 **`No signing certificate "Mac Development"`** 且仅作临时退路，可在 **终端.app** 执行：

```bash
cd /path/to/cloudreve-desktop
./scripts/build_macos_fpe_manual_sign.sh
```

**必须在系统终端运行**，以便批准钥匙串访问。

### 3.1 命令行构建失败：`No Accounts` / `No signing certificate "Mac Development"`

- **在 Cursor / 部分 IDE 内置终端里**，`xcodebuild` 可能**读不到**你在 Xcode 里登录的 Apple 账户，会报 `No Accounts` 或找不到 `Mac Development` 证书。  
  **请在 macOS「终端.app」中**执行 `./scripts/build_macos_fpe.sh`，或直接用 **Xcode 打开本工程 → Product → Build（⌘B）**。
- 若仍提示 **Mac Development**：打开 **Xcode → Settings → Accounts** → 选中你的 Apple ID → **Manage Certificates…** → 左下角 **+** → 若可选 **Mac Development** 则添加（新版 Xcode 往往统一使用 **Apple Development**，以本机 Xcode 显示为准）。
- 工程内 **Signing** 的 Team 需与钥匙串里 **`security find-identity -v -p codesigning`** 显示的 **Team ID** 一致；命令行可把 Team ID 写入 `~/.cloudreve/xcode_development_team` 供 `build_macos_fpe.sh` 读取。

## 4. 安装与注册 domain

### 方式 A（推荐）：宿主应用自动注册

1. 先保证 **Cloudreve Desktop 已运行并成功挂载过网盘**，这样会生成 `~/.cloudreve/drives.json`（内含 `mount_id` 与 `sync_path`）。
2. 构建并打开宿主应用：

```bash
cd /path/to/cloudreve-desktop
./scripts/build_macos_fpe.sh
open ./build/DerivedData/Build/Products/Debug/CloudreveFPHost.app
```

窗口里会显示从 `drives.json` 读取并调用 `NSFileProviderManager.add` 的结果；也可点「重新读取…」重试。

### 方式 B：命令行 / 独立脚本

仍可使用 `host/DomainRegistrar.swift`（环境变量 `CLOUDREVE_MOUNT_ID`、`CLOUDREVE_SYNC_PATH`），逻辑与宿主内注册一致。

### 验证

```bash
fileproviderctl diagnose 2>&1 | head -n 40
```

应能看到与你的扩展 / domain 相关的条目（具体格式因系统版本略有差异）。

**若仍只有 iCloud/OneDrive**：请看同目录下的 **`TROUBLESHOOTING.md`**（系统设置里启用「文件提供程序」扩展等步骤）。宿主窗口也会列出 `getDomains` 的结果，便于对照。

## 5. 说明：沙箱与 `~/.cloudreve/.../xpc.sock`

当前调试用的 entitlements 将 **App Sandbox 关闭**，便于扩展连接宿主 Unix socket。若以后要上架 Mac App Store，需要改为 App Group + 共享路径或 XPC 到宿主应用。
