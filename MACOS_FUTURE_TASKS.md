# macOS Future Tasks

这份备忘录记录 `macOS Sync MVP` 之后的后续开发任务，目标不是继续做抽象，而是逐步把 macOS 能力从“可用本地同步”推进到“接近 Windows 用户体验”的状态。

原则：
- 先把当前 MVP 验证稳定，再做能力增强
- 先做会影响正确性和可用性的事情，再做体验增强
- 抽象只服务于 `Windows + macOS` 两个目标平台

## P0: 当前 MVP 收口

这些任务建议在完成几轮本机验证后尽快处理。

- 补一轮真实运行日志审查，确认没有远端落地后被 watcher 反向误传的残余边界问题
- 审查 `remote_events` 与 `sync` 的交互，补充更多非虚拟模式下的回归测试点
- 为 `VirtualFileMode::None` 增加更明确的行为测试，覆盖创建、修改、删除、重命名、冲突
- 评估本地文件 mtime / size / inventory 的一致性策略，减少误判上传或误判下载
- 检查大文件下载、覆盖写入、断点失败后的恢复逻辑

## P1: 可用性增强（已完成）

## P2: 稳定性与测试建设

这些任务会决定 macOS 后续能否持续演进。下面拆成 **已交付（MVP 级）** 与 **仍待扩展**，避免与「整体验收」混淆。

### P2 已完成（MVP 级：回归 / 冒烟 / 可离线跑的测试）

- `crates/cloudreve-sync/tests/mvp_smoke.rs`：`DriveConfig` 序列化与 `sync_root_id` 别名兼容冒烟
- `crates/cloudreve-sync/tests/p2_persistence_smoke.rs`：多 drive `DriveState` JSON 往返、`extra` 字段保留、临时路径下 `InventoryDb` 初始化与二次打开；`DriveState::write_to_path` / `read_from_path` 与 `drives.json` 磁盘格式及旧字段 `sync_root_id` 兼容；空 `drives`、缺失文件、非法 JSON 的读取边界
- `crates/cloudreve-sync/tests/p2_inventory_tasks_smoke.rs`：任务队列 `insert_task_if_not_exist` 去重、`cancel_tasks_by_path`、`query_recent_tasks` 活跃/已完成划分
- `crates/cloudreve-sync/tests/p2_fs_event_grouping.rs`：`group_fs_events` 对 Remove/Create/Modify（含重命名）分桶与 `normalize_event_kind` 行为一致
- `drive/manager/types.rs`：`format_bytes` 单元测试（B / KB–TB）
- `DriveState::read_from_path` / `write_to_path`（`drive/manager/types.rs`）：与 `DriveManager::load` / `persist` 共用同一套读写逻辑，便于回归真实配置文件格式
- `scripts/smoke_mvp.sh`：聚合 `app-config` / `cloudreve-sync` /（macOS 下）`platforms-macos` 测试与 `cloudreve-desktop` 的 `cargo check`
- `crates/app-config`：`LogLevel` / `AppConfig` 的 JSON 往返与部分字段默认、`from_str` 边界
- `drive/remote_events.rs`：`BackoffState` 指数退避与 `reset` 单元测试（远端事件监听重连）
- `platforms/macos`：`ensure_mount_id` 稳定性、`escape_plist`、通知节流、`capabilities` 等单元测试

### P2 仍待扩展（整体验收 / 高成本场景）

以下仍作为 **P2 方向**，需 mock、集成环境或更长工时，**未**与上一节一并视为「已收尾」：

- 增加 `cloudreve-sync` 在 **非虚拟模式** 下的 **端到端 / 近端到端** 集成测试（非仅类型与分桶）
- **双向同步** 专用 smoke（当前 `smoke_mvp.sh` 以编译 + 单测为主，不覆盖真实双向同步链路）
- **macOS provider** 更细场景（权限、边界 API、与 UI 联动等），在现有单测上继续加
- **进程级** 重启恢复、异常退出恢复（超出「配置 / 库存文件」层面的单测）
- **多 drive 同时挂载** 的压力与交互场景（配置层已有覆盖，运行时未系统测）
- **大目录、深层路径、批量远端事件**（性能、去抖、任务积压）

## P3: macOS 平台体验增强

这些任务是“更像原生 macOS 客户端”的方向，但优先级应低于同步正确性。

- 研究 Finder 扩展边界，明确哪些能力必须放在 Finder Extension / File Provider Extension
- 设计 macOS 上的状态展示方式，不直接照搬 Windows shell UI
- 研究冲突处理在 macOS 上的更自然入口
- 评估缩略图、右键动作、在线打开等能力的 macOS 原生承载点

### 类 OneDrive 的逐文件同步状态（目标体验，跨阶段）

对标 **OneDrive / iCloud 云盘**：用户在 **Finder（及列表视图）** 中能区分至少以下几类状态（图标角标或覆盖层 + 可选文案），与 **Windows 上 CFAPI + Explorer 云状态** 对齐为长期目标：

| 状态（示例） | 用户感知 |
|--------------|----------|
| 仅云端 / 未落盘 | 占位或仅元数据，本地无完整内容或按需下载 |
| 同步中 | 正在上传或下载 |
| 已同步 | 本地与云端一致、可用 |
| 异常（可选） | 冲突、失败、需用户处理 |

**阶段划分：**

- **应用内**：按 drive / 任务 的同步概况已在 P1 范围交付；完整 **按路径枚举的列表** 可作为应用内二级视图逐步补齐。
- **系统级（Finder 角标）**：依赖 **P4 File Provider（及/或 Apple 文档要求的扩展形态）** 向系统注册逐项状态；P3 负责交互与信息架构，与 P4 技术路线对齐。

## P4: File Provider 路线

这是未来真正接近 Windows 虚拟文件能力的阶段，但不建议在当前 MVP 未稳定前启动。

- 调研并拆分 File Provider 需要的新 crate / 新宿主结构
- 明确 `src-tauri`、主 app、扩展进程之间的职责边界
- 设计 File Provider 所需的数据访问和事件传递接口
- **将「类 OneDrive」的逐文件状态（云端 / 同步中 / 已同步等）纳入域模型与 Provider 回调**，使 Finder 能展示与业务一致的状态机，而非仅普通文件夹同步
- 把当前 `VirtualFileMode::None` 的 macOS 实现作为 phase-1 路线保留
- 在不破坏现有本地同步模式的前提下，增加未来切换到 File Provider 的升级路径

## 建议执行顺序

建议按下面顺序推进：

1. 先完成 `MACOS_VALIDATION_CHECKLIST.md` 的至少一轮完整验证
2. 修掉验证中暴露出的 `P0` 问题
3. 补最关键的 `P2` 测试，保证后续改动可回归
4. 再决定是否进入 `P3 / P4`（含「类 OneDrive」逐文件状态）

## 下一阶段建议

如果要继续让我直接推进代码，建议下一轮聚焦下面方向之一：

1. `P0` 非虚拟模式正确性收口
2. `P2` 为 macOS MVP 补测试

---

## 备忘：当前非最高优先级

本节专门放**不阻塞**当前「同步正确性 + MVP 可用」主线的内容：有价值，但**刻意后置**，需要时再捡回。

- **整体验收 / 压测 / 重集成**（与上文 **「P2 仍待扩展」** 重叠）：非虚拟端到端集成、双向专用 smoke、进程级恢复、大目录与批量远端事件、多 drive 并行压测等——需 mock 或长时间环境时，排在 P0/P2 核心回归之后。
- **体验与系统深度集成**：见 **P3**、**P4**（Finder 扩展、File Provider、类 OneDrive 逐文件状态等）。
- **工程与周边**（举例，无用户故障或合规要求时不优先）：安装包体积与签名流程优化、崩溃上报/遥测、多语言与文案打磨、对外开发者文档与示例扩充。
