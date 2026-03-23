# Windows Validation Checklist

这份备忘录用于验证最近的架构调整没有破坏原有 Windows 能力。

目标不是只证明“能编译”，而是分层验证：
- Windows 平台 crate 仍然能接回 `cloudreve-sync`
- `src-tauri` 仍然能正确装配 Windows provider
- 已有用户配置仍然能恢复
- MSIX / Shell / CFAPI 相关能力仍然可用

## 使用方式

建议按顺序执行，每完成一项就记录结果：
- `PASS`: 行为符合预期
- `FAIL`: 行为异常，记录日志、截图、报错
- `SKIP`: 当前环境无法验证

建议同时记录：
- Windows 版本
- Rust 版本
- 是否为全新环境
- 是否使用旧配置目录
- 当前验证分支 / commit

## 0. 前置准备

执行环境：
- Windows 10 1903+ 或 Windows 11
- 已启用 Developer Mode
- 已安装 Rust、Node.js、Yarn、Windows SDK

验证前准备：
- 保留一份“旧版本可工作配置”用于升级兼容测试
- 准备一个可登录的 Cloudreve 测试账号
- 准备一个包含文件、文件夹、图片、冲突场景的测试目录

建议保留两套测试场景：
1. 全新安装 / 空配置
2. 旧配置升级 / 已有 drive

## 1. Windows 本机编译验证

在项目根目录执行：

```powershell
cargo check -p cloudreve-platforms-api -p cloudreve-platforms-windows -p cloudreve-sync -p cloudreve-desktop
cargo tauri build
```

检查项：
- `cloudreve-platforms-windows` 编译通过
- `cloudreve-sync` 不依赖 Windows 实现细节也能与 Windows 平台正常联编
- `src-tauri` 能正确引用 Windows 平台 crate
- 无新的链接错误、trait 装配错误、模块路径错误

通过标准：
- 上述命令全部成功

## 2. 未打包启动验证

执行：

```powershell
cargo tauri dev
```

检查项：
- 应用启动成功
- 托盘图标出现
- 主窗口 / 设置窗口可打开
- 不崩溃、不循环报错
- `DriveManager` 正常初始化
- 现有 drive 配置能够加载

通过标准：
- 应用可稳定运行，基本 UI 可操作

## 3. 升级兼容验证

目的：确认旧配置在架构调整后仍可恢复。

准备：
- 使用旧版本留下的配置目录启动新版本

检查项：
- 已有 drive 能被识别
- 不要求用户重新添加 drive
- 旧配置中的同步根 / mount 信息能恢复
- 启动后不会因为字段改名导致配置失效
- 同步状态、图标路径、账户信息正常

重点关注：
- 旧 `sync_root_id` 配置兼容读取
- drive 列表和本地同步目录恢复正常

通过标准：
- 老用户升级后无需手工修配置即可继续使用

## 4. MSIX 注册与 Shell 验证

执行：

```powershell
.\dev-install.ps1
```

如果已经构建过：

```powershell
.\dev-install.ps1 -SkipBuild
```

检查项：
- MSIX 注册成功
- 应用包能正常安装 / 更新
- 同步根成功注册
- 资源管理器中可见相关云文件状态
- Shell 集成组件正常加载

通过标准：
- 注册成功，且 Explorer 中能看到 Cloudreve 的集成行为

## 5. Shell / CFAPI 功能回归

### 5.1 同步根与基础展示

检查项：
- 同步目录正常显示
- 占位文件和真实文件状态符合预期
- 图标、状态、基础展示正常

### 5.2 占位文件行为

检查项：
- 打开在线文件时能触发下载
- 下载完成后可正常打开
- 文件夹按需展开正常
- 大文件读取不中断

### 5.3 本地到远端

检查项：
- 新建文件上传成功
- 修改文件上传成功
- 删除文件同步成功
- 重命名 / 移动文件成功
- 新建 / 删除文件夹成功

### 5.4 远端到本地

检查项：
- 远端新增文件可下发到本地
- 远端删除文件可同步到本地
- 远端重命名 / 移动可同步到本地
- 远端更新后本地状态正确刷新

### 5.5 Shell 扩展

检查项：
- 右键菜单正常显示
- `View online` 正常
- `Sync now` 正常
- 冲突处理入口正常
- 缩略图正常
- 自定义状态正常
- 状态 UI 正常

### 5.6 通知与冲突

检查项：
- 普通通知正常
- Token 过期通知正常
- 冲突 toast 正常
- 冲突后操作能正确传回应用

## 6. 生命周期回归

检查项：
- 重启应用后状态恢复正常
- 重启系统后应用行为正常
- 删除 drive 后能正确清理 mount / 注册信息
- 卸载开发包后系统状态干净

卸载命令：

```powershell
Get-AppxPackage *Cloudreve* | Remove-AppxPackage
```

## 7. 打包回归

执行：

```powershell
.\build-msix.ps1
```

可选：

```powershell
.\build-msix.ps1 -Arch x64
.\build-msix.ps1 -Arch arm64
```

检查项：
- 正式包能成功生成
- 生成的 MSIX / MSIXBundle 可安装
- 安装后的行为与 dev-install 注册结果一致

## 8. 最小回归结论

如果时间有限，最少完成以下 10 项：

1. `cargo check` 通过
2. `cargo tauri build` 通过
3. `cargo tauri dev` 可启动
4. 旧配置可恢复
5. `.\dev-install.ps1` 成功
6. 占位文件可按需下载
7. 本地修改可同步
8. 远端修改可同步
9. 右键菜单 / 缩略图 / 状态 UI 正常
10. 删除 drive 后清理正常

## 9. 结果记录模板

可直接复制下面这段做每轮验证记录：

```md
## Windows Validation Run

- Date:
- Machine:
- Windows version:
- Rust version:
- Branch / commit:
- Scenario: fresh install / upgrade

### Results
- Build:
- Tauri startup:
- Old config restore:
- MSIX registration:
- Placeholder hydration:
- Local -> remote sync:
- Remote -> local sync:
- Context menu:
- Thumbnail:
- Status UI:
- Notifications / conflict:
- Cleanup / uninstall:

### Notes
- 

### Verdict
- PASS / FAIL / PARTIAL
```
