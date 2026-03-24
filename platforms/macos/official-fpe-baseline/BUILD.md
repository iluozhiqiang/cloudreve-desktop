# Official FPE Baseline（对照工程）

与同级目录 `macos-file-provider-extension/` 中的 Cloudreve 生产宿主 **完全独立**，用于：

- 验证 **最小** `NSFileProviderReplicatedExtension` + 固定 smoke domain 能否在本机通过 `NSFileProviderManager.add`；
- 与业务代码（XPC、Rust、`drives.json`）隔离，便于判断故障在 **系统/签名/App ID** 还是在 Cloudreve 实现。

## 标识符（需在 Apple Developer 中创建）

| 用途 | 值 |
|------|-----|
| 宿主 Bundle ID | `com.cloudreve.desktop.fperef.host` |
| 扩展 Bundle ID | `com.cloudreve.desktop.fperef.host.fileprovider` |
| App Group | `group.com.cloudreve.desktop.fperef` (与 `OfficialFPHostDomain.swift` / `fpe/Info.plist` 一致) |

在 [Identifiers](https://developer.apple.com/account/resources/identifiers/list) 中为上述两个 App ID 勾选 **App Groups**，并创建与上面一致的 Group。

## 构建

```bash
cd /path/to/cloudreve-desktop
./scripts/build_official_fpe_baseline.sh
```

一键「清 DerivedData → 构建 → 安装 /Applications → 打印 codesign」（需 `DEVELOPMENT_TEAM` 或 `~/.cloudreve/xcode_development_team` 为 **10 位 Team ID**，勿填证书指纹）：

```bash
DEVELOPMENT_TEAM=你的TeamID ./scripts/verify_official_fpe_baseline.sh
```

或手动：

```bash
cd platforms/macos/official-fpe-baseline
DEVELOPMENT_TEAM=你的TeamID xcodebuild -allowProvisioningUpdates \
  -project OfficialFPEBaseline.xcodeproj -scheme OfficialFPHost -configuration Debug \
  -derivedDataPath ./build/DerivedData build
```

产物：`platforms/macos/official-fpe-baseline/build/DerivedData/Build/Products/Debug/OfficialFPHost.app`

## 安装与运行

```bash
ditto ./build/DerivedData/Build/Products/Debug/OfficialFPHost.app /Applications/OfficialFPHost.app
xattr -cr /Applications/OfficialFPHost.app
open /Applications/OfficialFPHost.app
```

在窗口里点 **「创建 App Group 容器 + 注册 smoke domain」**。成功时应出现 **「✓ 已注册 domain=fperef-smoke-domain」**。

若此处仍报 **-2001 / -2014**，则与 Cloudreve 主工程同源，应优先排查 **Apple 账号、描述文件、App Group 勾选、系统版本**。

## 自动化验证记录（可重复）

在仓库根目录执行（需本机已登录 Xcode Apple ID、`DEVELOPMENT_TEAM` 与开发者后台一致）：

```bash
DEVELOPMENT_TEAM=你的TeamID ./scripts/build_official_fpe_baseline.sh
```

已通过项（示例环境）：**BUILD SUCCEEDED**；宿主与 `OfficialFPE.appex` 均为 **TeamIdentifier** 非 adhoc；宿主与 appex 均含 **embedded.provisionprofile**；`ValidateEmbeddedBinary` / `RegisterWithLaunchServices` 在 xcodebuild 日志中为成功。

安装到 `/Applications` 并启动：

```bash
ditto platforms/macos/official-fpe-baseline/build/DerivedData/Build/Products/Debug/OfficialFPHost.app /Applications/OfficialFPHost.app
xattr -cr /Applications/OfficialFPHost.app
open /Applications/OfficialFPHost.app
```

**需人工确认**：窗口内点击「创建 App Group 容器 + 注册 smoke domain」后，是否出现 **「✓ 已注册 domain=fperef-smoke-domain」**。

### 若仍失败（-2001 / 底层 -2014），与 Cloudreve 主工程相同

说明 **最小 replicated 扩展** 在本机也无法被系统接受——**不是** Cloudreve 业务代码问题。Swift 在 macOS 上 **不能使用** `NSFileProviderExtension` 子类（SDK 标为 `unavailable`），只能使用 **`NSFileProviderReplicatedExtension`**，无法像旧资料那样改用 `com.apple.fileprovider-nonui` 做 Swift 侧对照。

**建议下一步**：① 在稳定版 macOS（非预览版）上重复构建/注册；② 通过 **Feedback Assistant** 向 Apple 反馈（附 `sysdiagnose`）；③ 在开发者账号侧确认 File Provider / App Groups 能力。

### 可选：`fileprovider.testing-mode`（勿直接抄进 entitlements 除非后台已开）

在 [Identifiers](https://developer.apple.com/account/resources/identifiers/list) 中为 **扩展** App ID 勾选 **File Provider Testing Mode**（名称以 Apple 后台为准），保存后让 Xcode **Download Manual Profiles** 或命令行加 `-allowProvisioningUpdates` 重新生成描述文件。然后再在 **`OfficialFPE.entitlements`** 中加入：

```xml
<key>com.apple.developer.fileprovider.testing-mode</key>
<true/>
```

若描述文件尚未包含该 entitlement，构建会报错：`Provisioning profile ... doesn't include the FileProvider Testing Mode capability`。此时应 **删掉** 上述两行，恢复可构建状态（本仓库默认不包含此项）。

## 在此基础上改回 Cloudreve

1. 保留本工程 target 结构（宿主 + Embed & Sign 扩展 + `FileProvider.framework`）。
2. 将 `extension/` 中文件替换为 `platforms/macos/macos-file-provider-extension/extension/` 的实现，并恢复 `XPCClient`、路径编解码等。
3. 将 `host-app/` 中注册逻辑替换为 `FileProviderDomainRegistration` + `CloudreveDrivesConfig`。
4. 把 Bundle ID / App Group 改回生产用（或继续用独立 ID 做 staging）。
