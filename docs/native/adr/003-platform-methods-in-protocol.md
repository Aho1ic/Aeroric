# ADR-003：平台特定命令进入本地协议 v1，并一律走能力门控

- 状态：**accepted（用户在 Phase 1 Gate 拷问中拍板）**
- 日期：2026-09-22
- 关联：ADR-001 §命令类别→归属表、ADR-002（命名口径）、`MIGRATION_PLAN.md` P2-01（`capability.rs`、`methods.schema.json`）、P7-02 Action 3（`wsl=false`）、§6.1（Windows/Linux 保持可用）

## 背景：两份工件互相矛盾，是拷问时发现的

- ADR-001 第 74 行：`platform_specific`（62 条）归 **`aeroric-core` 的 `cfg` 分支**，"**仅当 host 实际支持**"，macOS 侧显示"不支持 + 原因"，不得伪装本地。
- 而 `command-map.json` 实测：这 62 条 **`rpc_method` 全部为空**（`wsl_git` 32、`wsl` 19、`wsl_fs` 9、`platform` 2；`check_module` 全是 `platforms`）。

也就是说：一边规定它们必须由 core 执行，一边没给它们任何线名。若照现状进入 P2-01 冻结，`methods.schema.json` 会**静默缺掉整个平台面**——而协议面是"改名字就是协议变更"的那种东西（660 个命令名实测全局唯一）。

## 决策

1. **62 条 `platform_specific` 命令全部进入本地协议 v1 的方法面**，命名沿用 ADR-002 规则：`<服务面>.<旧命令名>`，服务面由 `meta.rpc_resolution` 推导（`wsl_git → git`，与已存在的 `git.remote_git_*` 对齐；`wsl`、`wsl_fs`、`platform` 归各自服务面）。
2. 每条方法在 schema 中带 **`requires_capability`**（如 `wsl`）。**能力由 host 声明，client 不得用自己所在的 OS 去推断 host 能力**（这条不是新发明，`MIGRATION_PLAN.md` P7-02 Action 3 已如此规定；本 ADR 只是把它接到协议面上，使其有机械可验的落点）。
3. host 未声明该能力时：客户端控件禁用/隐藏，并显示**能力缺失的原因**；调用被服务端拒绝。macOS 因此对 `wsl*` 报 `wsl=false`；未来远程 Windows host 可以声明 `wsl=true`，同一份协议即可覆盖。
4. `host_local_os_ui`（19 条）**保持无线名**，这不是遗漏而是设计：它们由客户端适配层实现，CLI 侧返回 `INTERACTIVE_UI_REQUIRED`（ADR-001 既定）。这条边界今后不得被"顺手也给它一个 RPC"绕过。

## 备选与被否决的理由

- **留到 v1.1 再补**：等于做一个明知不完整的冻结；计划禁止"稍后迁移"行，且后续补录就是协议变更（命名唯一性已实测），补进来的名字还要与 62 条旧命令名保持一一对应。
- **永久排除在本地协议外（只留在旧 Tauri）**：这会把 §6.1 的"原 Tauri Windows/Linux 保持可用"在原生客户端上直接作废——Windows 上的 SwiftUI/CLI 永远无法管理 WSL 项目。这是**产品范围收缩**，不是技术选型，因此不能由迁移顺带决定；用户明确选择了不收缩。

## 后果

- P2-01 的方法面从 579 增至 **641**（+62），schema 需要新增能力门控字段，fixtures 需要"能力缺失被拒"与"能力具备可执行"两类反例。
- 能力声明成为**协议的一部分**：`AgentCapabilities` 的字段命名与演进受 ADR 约束，不能各客户端各猜一套。
- P7-02 的验收有了可机械检查的目标（不是"看起来禁用了"），也让 macOS 的"不支持 + 原因"与 ADR-001 的措辞真正对应。
- 若未来某天决定不迁移某个平台能力，必须**新写 ADR 推翻本决定**，而不是把方法从 schema 里悄悄删掉——删除方法同样是协议变更。
