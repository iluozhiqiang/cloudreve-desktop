# macOS Validation Checklist

这份备忘录用于验证当前 `macOS Sync MVP` 是否已经达到“可启动、可添加 drive、可做真实文件同步”，以及 P3/P4 合并后的系统级状态展示最小闭环。

这一轮可分两层验证：
- 基础同步层（原 P0-P2）
- 系统级状态层（P3/P4 合并 MVP：File Provider + 自定义 XPC + Finder 状态）

## 使用方式

建议按顺序执行，每完成一项就记录结果：
- `PASS`: 行为符合预期
- `FAIL`: 行为异常，记录日志、截图、报错
- `SKIP`: 当前环境无法验证

建议同时记录：
- macOS 版本
- Rust 版本
- Node.js / Yarn 版本
- 当前验证分支 / commit
- Cloudreve 服务端版本
- 测试账号和测试目录说明

## 0. 前置准备

执行环境：
- macOS
- 已安装 Rust
- 已安装 Node.js 18+ 和 Yarn
- 本地可启动 Cloudreve 服务端，或有一个可用的测试服务端

验证前准备：
- 准备一个全新空目录作为本地同步目录
- 准备一个测试账号
- 准备一个远端测试目录，至少包含：
- 一个空文件夹
- 一个小文本文件
- 一个中等大小文件
- 一个可重命名/删除的文件

建议分两组场景：
1. 全新安装 / 空配置
2. 已有配置 / 已添加 drive 后重启恢复

## 1. 编译验证

在项目根目录执行：

```bash
cargo check -p cloudreve-platforms-api -p cloudreve-platforms-macos -p cloudreve-sync -p cloudreve-desktop
```

检查项：
- `cloudreve-platforms-macos` 编译通过
- `cloudreve-sync` 与 `cloudreve-platforms-macos` 在 `VirtualFileMode::FileProvider` 路径下可正常联编
- `src-tauri` 能正确装配 macOS provider
- 无新的 trait 装配错误、模块路径错误、平台条件编译错误

通过标准：
- 上述命令成功

## 2. 开发态启动验证

执行：

```bash
cd ui
yarn install
cd ..
cargo tauri dev
```

检查项：
- 应用成功启动
- 主窗口可打开
- 设置窗口可打开
- 托盘图标正常
- 没有 `mount/connect` 启动失败
- 没有明显的 Windows-only 运行时错误

通过标准：
- 应用可稳定运行，基本 UI 可操作

## 3. 新增 Drive 验证

准备：
- 在本地创建一个空目录，例如 `~/tmp/cloudreve-macos-test`
- 使用测试账号登录并添加一个新的 drive

检查项：
- drive 可以成功添加
- drive 可以进入运行态
- 配置成功持久化
- 不要求系统级 mount 注册
- 不因为 `connect_mount()` 缺失而失败

通过标准：
- 添加 drive 后应用不报错，drive 状态正常

## 4. 本地 watcher 验证

检查项：
- drive 启动后本地同步目录被 watcher 监听
- 在本地目录中新建文件时，会触发同步流程
- 修改本地文件时，会触发同步流程
- 删除本地文件时，会触发同步流程
- 重命名本地文件时，会触发同步流程

建议验证动作：
1. 新建 `local-created.txt`
2. 修改一个已有文本文件
3. 删除一个测试文件
4. 重命名一个测试文件

通过标准：
- 上述动作都能被识别，且不会因为没有 placeholder 能力而中断

## 5. 远端到本地验证

目的：确认远端变化能落地成真实本地文件，而不是依赖 placeholder。

检查项：
- 远端新增文件后，本地出现真实文件
- 远端新增文件夹后，本地出现真实目录
- 远端更新文件后，本地内容被更新
- 远端删除文件后，本地被删除
- 远端重命名 / 移动后，本地路径同步变化

重点观察：
- 本地物化后的文件是真实文件
- 远端同步落地后不会立刻被本地 watcher 误判成新的上传

通过标准：
- 远端变更可以稳定落地到本地，且没有明显“远端更新后被反向重新上传”的异常

## 6. 本地到远端验证

目的：确认真实本地文件路径上的改动，仍然能被调度到上传流程。

检查项：
- 本地新建文件可以上传
- 本地修改文件可以上传
- 本地删除文件可以同步到远端
- 本地重命名 / 移动文件可以同步到远端
- 本地新建 / 删除文件夹可以同步到远端

通过标准：
- 双向同步基本闭环可用

## 7. 重启恢复验证

目的：确认这轮实现不是“一次性运行成功”，而是基本可持续使用。

步骤：
1. 添加 drive 并完成一次同步
2. 完全退出应用
3. 重新启动应用

检查项：
- 已有 drive 能被加载
- drive 能自动恢复运行
- watcher 能再次启动
- 配置中的 `mount_id` 能稳定复用
- 不要求用户重新添加 drive

通过标准：
- 重启后可继续正常使用

## 8. 桌面集成最小验证

这一轮只验证 phase-1 的最小能力。

检查项：
- Finder reveal 正常
- 普通通知可触发
- Token 过期通知可触发
- 冲突通知路径不会导致应用崩溃
- 自启动能力保持关闭，不影响当前同步主流程

通过标准：
- 最小桌面能力可用，未实现能力不会拖垮主流程

## 9. 最小回归结论

如果时间有限，最少完成以下 8 项：

1. `cargo check` 通过
2. `cargo tauri dev` 可启动
3. 可以新增 drive
4. drive 能进入运行态
5. 本地 watcher 正常
6. 远端新增/修改可落地到本地真实文件
7. 本地新增/修改可进入上传流程
8. 重启后 drive 可恢复

## 10. 结果记录模板

可直接复制下面这段做每轮验证记录：

```md
## macOS Validation Run

- Date:
- Machine:
- macOS version:
- Rust version:
- Node.js / Yarn version:
- Branch / commit:
- Cloudreve version:
- Scenario: fresh install / restart recovery

### Results
- Build:
- Tauri startup:
- Add drive:
- Watcher:
- Remote -> local:
- Local -> remote:
- Restart recovery:
- Finder reveal:
- Notifications:

### Notes
- 

### Verdict
- PASS / FAIL / PARTIAL
```

## 11. 系统级状态（P3/P4 合并 MVP）专项验证

目标：验证 Finder 可读取并展示四档状态（`CloudOnly` / `Syncing` / `Synced` / `Error`）的最小闭环。

前提：
- macOS 端已启动主应用（会拉起 `~/.cloudreve/macos-file-provider/xpc.sock`）
- File Provider Extension 已装配到 Xcode target（当前仓库提供 skeleton）

检查项：
- Rust 侧自定义 XPC 服务已监听：`~/.cloudreve/macos-file-provider/xpc.sock`
- Extension 发起 `get_item_state` 请求后，可收到合法 JSON 响应
- 状态映射规则符合预期：
  - 本地不存在且无活动任务/错误 -> `CloudOnly`
  - 有 pending/running 任务 -> `Syncing`
  - 本地存在且无活动任务/错误 -> `Synced`
  - 有 failed 任务或冲突标记 -> `Error`
- 应用重启后状态仍可被查询（socket 恢复、状态文件仍可读）

建议验证动作：
1. 构造一个仅库存存在、未本地物化的路径（验证 `CloudOnly`）
2. 触发上传或下载任务（验证 `Syncing`）
3. 任务完成后重新查询（验证 `Synced`）
4. 人为制造失败任务或冲突（验证 `Error`）
5. 重启应用后重复查询上述路径（验证一致性）

通过标准：
- 四档状态都可稳定复现
- Extension/主程序重启后仍可正确返回状态

## 附录：协作 / 共享目录「本地已删、Web 未删」时的日志抓取

若仍出现本地删除后云端未删除，可按下面步骤留一份**脱敏**日志，便于对照 API 与 URI：

1. **提高同步相关日志级别**（终端启动示例）  
   ```bash
   RUST_LOG=cloudreve_sync=debug,drive::commands=debug cargo tauri dev
   ```  
   或仅过滤 drive 命令路径：  
   `RUST_LOG=drive::commands=debug`

2. **复现操作**：在问题目录下删除文件后，等待约数秒（批量删除会走 `process_fs_delete_events`）。

3. **在日志或终端输出中搜索关键字**（与代码中 `tracing` 一致）：  
   - `Processing filesystem delete events` — 是否收到删除事件、URI 映射 `uris`  
   - `Sending batch delete request to server` — 是否发起删除请求  
   - `Successfully deleted all files from server` — 整批成功  
   - `Batch delete operation failed` / `Partial batch delete failure` — 服务端拒绝或部分失败  
   - `Failed to convert local path to remote URI` — 本地路径无法转成 URI（会跳过该条）  
   - `No valid URIs to process` — 没有可处理的 URI（例如全部被过滤）

4. **脱敏后再分享**：将实例域名、用户 ID、完整 `cloudreve://` 路径中的敏感段替换为占位符；保留**错误码/错误信息结构**与**是否出现上述关键字**即可。

5. **说明**：客户端已用库存中的服务端 `FileResponse.path`（元数据键 `sys:desktop_inventory_uri`）优先作为删除 URI；若旧库存未带该字段，可对目录做一次同步或重新列目录后再试删除。
