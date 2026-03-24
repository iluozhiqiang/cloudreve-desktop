# macOS File Provider（Finder 角标）排障

## 必查：`codesign` 为 ad-hoc、且未设置 Development Team

在终端执行：

```bash
codesign -dv /Applications/CloudreveFPHost.app 2>&1 | head -5
codesign -dv /Applications/CloudreveFPHost.app/Contents/PlugIns/CloudreveFPE.appex 2>&1 | head -5
```

若出现 **`Signature=adhoc`** 且 **`TeamIdentifier=not set`**，则 **File Provider 的 domain 注册常会失败**，界面只显示类似 **「应用程序目前无法使用」**。

若 NSError 里出现 **`[NSFileProviderErrorDomain code=-2001]`** 且 **`NSUnderlyingError` … `code=-2014`**，对应头文件中的 **`NSFileProviderErrorApplicationExtensionNotFound`**：系统在**签名/插件信任**层面无法加载嵌入的 `.appex`，**不是**「没拷到 PlugIns 文件夹」。**必须**用带 **Apple Development** 的签名重新构建（两 target 同一 Team）。

**处理：** 用 Xcode 打开 `CloudreveFileProvider.xcodeproj`，**先**在 **Xcode → Settings → Accounts** 登录 Apple ID；对 **CloudreveFPHost** 与 **CloudreveFPE** 两个 target 都在 **Signing & Capabilities** 里选择**同一** **Apple Development Team**（免费 Personal Team 亦可），然后：

```bash
cd /path/to/cloudreve-desktop
./scripts/build_macos_fpe.sh
./platforms/macos/macos-file-provider-extension/scripts/install_cloudreve_fpe_to_applications.sh
```

（也可把 Team ID 写入 `~/.cloudreve/xcode_development_team` 再执行 `build_macos_fpe.sh`，见 `BUILD.md`。）

宿主在出错时会把完整 `NSError` 追加写入 **`~/.cloudreve/fpe-host-debug.log`**，可用 `cat ~/.cloudreve/fpe-host-debug.log` 查看（不依赖窗口是否截断）。

---

## 关键对照：`platforms/macos/official-fpe-baseline` 仍报 -2001 / -2014

仓库中的 **`platforms/macos/official-fpe-baseline/`** 是 **无 XPC、无 drives.json、无 Cloudreve 业务** 的最小 `NSFileProviderReplicatedExtension`。若此处 **App Group 容器已就绪**，但 **`NSFileProviderManager.add` 仍失败**（底层 **-2014**），可认定问题 **不在** Cloudreve 主工程 Swift/Rust 实现，而在 **本机环境**，例如：

- **macOS 预览版 / 小版本** 上 `fileproviderd` 与 replicated 扩展的已知/未知缺陷（建议用 **Feedback Assistant** 附 `sysdiagnose` 反馈 Apple）；
- **Apple Developer / Personal Team** 与 File Provider 在部分系统组合下的限制（可尝试 **付费计划** 或 **另一台已发行版 macOS** 对照）；
- **仅有一条可行 API 路径**：在 Swift 中 **`NSFileProviderExtension` 在 macOS 上被标为不可用**，第三方 File Provider 只能走 **`com.apple.fileprovider-replicated` + `NSFileProviderReplicatedExtension`**，无法像旧文档那样用 `com.apple.fileprovider-nonui` 子类在 Swift 里做 A/B（第三方 App 如 OneDrive 可能仍使用 ObjC 或其它构建配置）。

**可选实验（需在开发者后台为扩展 App ID 启用对应能力）**：在扩展 entitlements 中加入 **`com.apple.developer.fileprovider.testing-mode`**（仅开发用，上架前须移除），见 [Apple 文档](https://developer.apple.com/documentation/BundleResources/Entitlements/com.apple.developer.fileprovider.testing-mode)。

---

## 现象：`fileproviderctl diagnose` 里只有 iCloud / OneDrive，没有 Cloudreve

说明 **系统尚未加载你的 File Provider 扩展**，或 **domain 没注册成功**。角标是系统按扩展画的，这一步没通就不会有任何状态图标。

### 1. 必须先运行宿主 App 并完成注册

- 构建并打开 **`CloudreveFPHost.app`**（见 `BUILD.md`）。
- 窗口里应出现 **「✓ 已注册 domain=…」**；若全是 **「✗ …」**，把完整日志复制下来排查（常见：无 `mount_id`、`drives.json` 路径不对）。

### 2. 在系统设置里启用「文件提供程序」扩展（必查）

不同 macOS 版本菜单位置略有不同，请在本机搜索 **「扩展」** 或 **「File Provider」**：

- 打开 **系统设置（System Settings）**
- 进入 **隐私与安全性 → 扩展 → 文件提供程序**，部分版本在 **通用 → 登录项与扩展 → 扩展**  
- 在列表中搜索 **Cloudreve**、**FPE**、**Host**（宿主显示名为 **Cloudreve FPE Host**，扩展显示名可能为 **Cloudreve**）
- 若**能**看到条目，请确保为 **开启**

#### 若「文件提供程序」列表里完全没有 Cloudreve / FPE（与你的情况一致）

这与「开关被关掉」**不是一回事**：说明 **系统从未把你的扩展登记进可管理的扩展列表**，与 **`NSFileProviderError` -2001 / 底层 -2014** 是同一类问题——扩展在系统层面仍被视为不可用，**不是**去设置里「找一个开关打开」就能解决。

请优先做：

1. **Xcode → CloudreveFPE target → Signing & Capabilities**：对 **App Groups** 执行一次 **移除能力 → 再点 + 重新添加 → 勾选 `group.com.cloudreve.desktop.fpe`**。正常时 App Group 名称旁应有 **可勾选框**；若只有一行文字、无勾选框，属 Xcode 已知配置异常，会导致运行时异常（参见 Stack Overflow 等「App Groups checkbox」讨论）。
2. **Clean Build Folder**（⇧⌘K）后重新 `./scripts/build_macos_fpe.sh`，再执行 `install_cloudreve_fpe_to_applications.sh`。
3. 开发者后台：扩展用 Bundle ID **`com.cloudreve.desktop.fpehost.fileprovider`** 的 App ID 必须启用 **App Groups**，并与 Xcode 中完全一致。

待 **宿主内注册不再报 -2001** 后，扩展才有机会出现在「文件提供程序」列表中；若仍无，再结合 `diagnose_macos_fpe.sh` 与 Console 中 `fileproviderd` 日志排查。

### 3. 尽量把 App 安装到「正规」位置

将 `CloudreveFPHost.app` 拷到 **`/Applications`** 再运行一次注册，然后可尝试：

```bash
killall Finder
```

### 4. 确认看的是「云盘域」而不是普通文件夹

角标针对 **File Provider 提供的条目**。请在 **Finder 侧边栏** 找 Cloudreve/对应网盘入口；仅在普通路径里浏览同步目录时，可能看不到与 FPE 绑定的装饰。

### 5. 主程序与 `mount_id` 一致

- **Cloudreve Desktop 需已挂载**，生成 `~/.cloudreve/drives.json`（含 `mount_id`）。
- 扩展通过 IPC 问 Rust 时使用的 **`mount_id`** 必须与注册 domain 时的一致。

### 6. 命令行辅助

```bash
# 粗略查看与 file provider 相关的插件（名称因系统而异）
pluginkit -m -v 2>/dev/null | awk '/cloudreve|Cloudreve|fpehost|FileProvider/ {print}'
```

**注意：** 使用 **`com.apple.fileprovider-replicated`** 的扩展在部分系统上 **`pluginkit -p com.apple.fileprovider-replicated` 可能始终为空**（而 `com.apple.fileprovider-nonui` 下列出 OneDrive 等）。因此 **不能** 单靠「pluginkit 里有没有 Cloudreve」判断 replicated 扩展是否安装成功，应以 `codesign`、`embedded.provisionprofile`、宿主能否注册 domain 为准。

---

## 现象：宿主窗口提示「应用程序目前无法使用」

这是系统对 `NSFileProviderError` 的本地化短句，**本身不说明原因**。请在 **Cloudreve FPE Host** 里重新点一次注册/查询：日志会附带 **`[NSFileProviderErrorDomain code=…]`** 与 `userInfo`。

| code | 含义 | 常见处理 |
|------|------|----------|
| **-2001** | `ProviderNotFound`：系统在你这个 App 里找不到可用的 File Provider 扩展 | 在 Xcode 为 **宿主 + 扩展** 设置同一 **Signing / Development Team**；确认扩展 target 已 **Embed & Sign**；不要用损坏的 ad-hoc 包 |
| **-2002** | `ProviderTranslocated`：因 **App Translocation**（常从「下载」、压缩包解压路径、DerivedData 直接运行）禁用提供程序 | 把 `CloudreveFPHost.app` 拷到 **`/Applications`**，并清除隔离属性：`xattr -cr /Applications/CloudreveFPHost.app`，再打开注册 |
| **-2003 / -2004** | 扩展版本与已注册不一致等 | 退出宿主、`killall Finder` 后重试；必要时移除旧 domain 再注册 |

仓库脚本（从构建产物安装到 `/Applications` 并 `xattr -cr`）：

```bash
# 在 cloudreve-desktop 根目录，按 BUILD.md 先 xcodebuild 出 .app
./platforms/macos/macos-file-provider-extension/scripts/install_cloudreve_fpe_to_applications.sh
```

### 一键自检（推荐）

安装到 `/Applications` 后，在仓库根目录执行：

```bash
./platforms/macos/macos-file-provider-extension/scripts/diagnose_macos_fpe.sh
# 或指定路径: APP="$HOME/…/CloudreveFPHost.app" ./platforms/macos/macos-file-provider-extension/scripts/diagnose_macos_fpe.sh
```

会依次检查：**PlugIns 内是否有 appex**、`codesign` 的 Team/adhoc、`embedded.provisionprofile`、entitlements 摘要、`spctl`、`pluginkit` 是否出现 Cloudreve。把输出贴到 issue 便于排查。

---

## 现象：已带 Team 签名，仍 `-2001` / 底层 `-2014`（ApplicationExtensionNotFound）

### 必查：宿主内是否有 `embedded.provisionprofile`

在终端执行：

```bash
ls -la /Applications/CloudreveFPHost.app/Contents/embedded.provisionprofile \
       /Applications/CloudreveFPHost.app/Contents/PlugIns/CloudreveFPE.appex/Contents/embedded.provisionprofile
```

若 **不存在**：说明你当前安装的是 **「无签名构建 + 手动 codesign」** 的产物——**不会**带上 Xcode 为 App Groups 生成的描述文件，系统往往仍报 `-2014`。

**正确做法：** 在仓库根目录使用 **`./scripts/build_macos_fpe.sh`**（已含 `-allowProvisioningUpdates`），或直接在 **Xcode 里 ⌘B**，再执行 **`platforms/macos/macos-file-provider-extension/scripts/install_cloudreve_fpe_to_applications.sh`** 安装到 `/Applications`。不要用 `build_macos_fpe_manual_sign.sh` 作为主流程。

### 其它常见遗漏（Apple 论坛 / Stack Overflow）

0. **扩展沙箱开关与 entitlements 必须一致**：Xcode 里 **App Sandbox = ON** 时，`CloudreveFPE.entitlements` 里必须是 **`com.apple.security.app-sandbox` = true**，否则会持续 **-2014**。本仓库扩展已：**沙箱 ON** + `app-sandbox` true + **临时例外** `com.apple.security.temporary-exception.files.home-relative-path.read-write`（`.cloudreve`）以便访问 `~/.cloudreve/.../xpc.sock`。宿主 **CloudreveFPHost** 仍可保持沙箱关闭（仅扩展需与 File Provider 要求一致时开沙箱）。

1. **扩展 target 未链接 `FileProvider.framework`**：在 Xcode 扩展 target → General → Frameworks and Libraries 中应包含 **FileProvider.framework**（本仓库已写入工程）。
2. **宿主与扩展未配置同一 App Group**，且扩展 `Info.plist` 的 `NSExtension` 内缺少 **`NSExtensionFileProviderDocumentGroup`**（需与 App Group 字符串一致）。本仓库使用 **`group.com.cloudreve.desktop.fpe`**。
3. **首次使用 App Groups**：用 Xcode 打开工程 → 两个 target → **Signing & Capabilities** → **+ Capability** → **App Groups** → 勾选与上面相同的 group（让 Apple 在开发者侧登记该 group）。
4. 终端自检：执行 `pluginkit -m -v 2>/dev/null | grep -i cloudreve` —— 若 **完全没有** Cloudreve 相关行，说明扩展仍未被系统收录，优先检查 1～3。

`spctl -a -vv /Applications/CloudreveFPHost.app` 若显示 **rejected**（未公证），多数情况下仍可本地运行，但若 **File Provider 仍 -2014** 且 **`pluginkit -m -v` 里完全没有 Cloudreve**，可尝试：① 在 **Finder** 中对 `CloudreveFPHost.app` 右键 **打开** 一次并确认运行；② **系统设置 → 隐私与安全性** 若出现被阻止提示则点 **仍要打开**；③ 确保宿主在启动时已创建 App Group 容器（界面首行会显示 `App Group 容器已就绪: ...`）。长期分发需 **公证（notarize）**，开发阶段通常不必。

---

宿主窗口内已支持 **「查询已注册 domain」**：若此处也为空，优先检查 **扩展是否在系统设置中被禁用** 以及 **是否用 CloudreveFPHost 成功执行过注册**。
