# Cloudreve macOS File Provider (FPE) skeleton

目标：在 Finder 里显示类似 OneDrive 的逐文件同步状态（CloudOnly / Syncing / Synced / Error 的角标/覆盖层）。

## 重要说明（为什么必须有 FPE）
- 如果系统没有安装并注册你的 File Provider Extension，那么 Finder 不会调用 `xpc.sock`，也就不可能显示 OneDrive 风格的系统级角标。
- 你当前系统里只有 `iCloudDriveFileProvider` 与 `OneDrive-mac.FileProvider`，没有 Cloudreve 的 domain/provider。

## 你当前仓库里已有的能力
- Rust 宿主侧提供了一个 IPC socket：`~/.cloudreve/macos-file-provider/xpc.sock`
- 宿主侧能回答：
  - `get_item_state`：返回 `category`（`CloudOnly|Syncing|Synced|Error`）
  - `fetch_placeholders`：返回目录下的 placeholder 元数据
  - `fetch_data`：按 range 返回文件内容（base64）

## 推荐路径：已内置 Xcode 工程（可直接编译）

仓库里已经包含：

- `CloudreveFileProvider.xcodeproj`（宿主 `CloudreveFPHost` + 扩展 `CloudreveFPE`）
- 详细命令行编译说明见 **`BUILD.md`**
- **看不到 Finder 角标 / `fileproviderctl` 里没有 Cloudreve**：见 **`TROUBLESHOOTING.md`**（含系统设置里启用「文件提供程序」扩展）。

你只需要在本机完成 **代码签名 Team**（`DEVELOPMENT_TEAM`）以及首次 Xcode 组件安装（若 `xcodebuild` 提示 `runFirstLaunch` / CoreSimulator 缺失）。

## 备选路径：自己手工建 Target（不推荐）

如果你更想从零创建：

1. 打开 Xcode
2. `File` -> `New` -> `Target...`
3. 选择模板：`File Provider Extension`
4. 把本目录 `extension/*.swift` 添加到该 target
5. 在你的 extension `Info.plist` 里补上 `NSFileProviderDecorations`（重点是 `Identifier` 要与代码返回的规则一致）
6. 在主 app（或一个独立的工具）里调用 NSFileProviderManager 注册 domain（见 `host/DomainRegistrar.swift`）

## 运行方式（域注册参数）
`host/DomainRegistrar.swift` 通过环境变量读取：
- `CLOUDREVE_MOUNT_ID`：对应 Rust 侧 drives.json 的 `mount_id`
- `CLOUDREVE_SYNC_PATH`：对应你希望 Finder 显示/同步的根目录（应与 Rust core 的 `sync_path` 一致）
- `CLOUDREVE_DISPLAY_NAME`：显示名（可选，默认 `Cloudreve`）

> 说明：这里的 “注册 domain/provider” 是系统级关键步骤；没有这一步 Finder 不会显示我们的角标。

## 协议字段说明（和 Rust IPC 对齐）
- 请求 JSON `type`：
  - `get_item_state`
  - `fetch_placeholders`
  - `fetch_data`
- `get_item_state` 请求包含：
  - `mount_id`：对应 Rust 侧注册的 mount_id（drives.json 里的 mount_id）
  - `path`：对应 Rust 侧使用的 local path
- 返回：
  - `category`: `CloudOnly | Syncing | Synced | Error`

## 备注
- 本骨架先把 Finder decorations（角标）打通；后续再把 placeholder materialization 与内容 fetch 完整对齐到你们 core 的占位/水化语义。

## Finder decorations：Identifier 对齐规则
我们的 `CloudreveFileProviderItem.decorations()` 返回的 Identifier 形如：
`$(PRODUCT_BUNDLE_IDENTIFIER).decoration.<category>`
其中 `<category>` 为：`cloudOnly` / `syncing` / `synced` / `error`

把下面 `NSFileProviderDecorations` 片段加入到 extension 的 `NSExtension` 字典里即可：

```xml
<key>NSFileProviderDecorations</key>
<array>
  <dict>
    <key>BadgeImageType</key>
    <string>com.apple.icon-decoration.badge.cloud</string>
    <key>Category</key>
    <string>Badge</string>
    <key>Identifier</key>
    <string>$(PRODUCT_BUNDLE_IDENTIFIER).decoration.cloudOnly</string>
    <key>Label</key>
    <string>CloudOnly</string>
  </dict>
  <dict>
    <key>BadgeImageType</key>
    <string>com.apple.icon-decoration.badge.sync</string>
    <key>Category</key>
    <string>Badge</string>
    <key>Identifier</key>
    <string>$(PRODUCT_BUNDLE_IDENTIFIER).decoration.syncing</string>
    <key>Label</key>
    <string>Syncing</string>
  </dict>
  <dict>
    <key>BadgeImageType</key>
    <string>com.apple.icon-decoration.badge.checkmark</string>
    <key>Category</key>
    <string>Badge</string>
    <key>Identifier</key>
    <string>$(PRODUCT_BUNDLE_IDENTIFIER).decoration.synced</string>
    <key>Label</key>
    <string>Synced</string>
  </dict>
  <dict>
    <key>BadgeImageType</key>
    <string>com.apple.icon-decoration.badge.warning</string>
    <key>Category</key>
    <string>Badge</string>
    <key>Identifier</key>
    <string>$(PRODUCT_BUNDLE_IDENTIFIER).decoration.error</string>
    <key>Label</key>
    <string>Error</string>
  </dict>
</array>
```

