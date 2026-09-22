# ADR-004：git 语义的验收归属——一致性归 `git`，适配归 `connections`/`platforms`

- 状态：**accepted（用户在 Phase 1 Gate 拷问中拍板）**
- 日期：2026-09-22
- 关联：ADR-002、ADR-003、`MIGRATION_PLAN.md` P6-04 / P6-05 / P7-01 / P7-02、§5.4（禁止用 skip 当通过）、§6.1（Windows/Linux 与 mobile 保持可用）

## 实测前提（不是观点）

`command-map.json` 里 105 条 git 相关命令的归属本来就分成了三套：

| 来源模块 | 条数 | `core_service` | 现在的 check 模块 | RPC 前缀 |
|---|---|---|---|---|
| `git` | 40 | `git` / `git::worktree` | `git` 36 + `worktree` 4 | `git.` |
| `remote_git` | 32 | `git(ssh target)` | `connections` | **`git.`（已经同名）** |
| `wsl_git` | 32 | `git(wsl target)` | `platforms` | （ADR-003 后并入 `git.`） |

也就是说：**协议面本来就是统一的 `git.*`**，分歧只存在于"谁来验收"。因此这不是协议决定，而是验收边界决定——也正是它容易被随手改动的地方。

## 决策

1. **`git` check 模块**负责**目标无关的 git 语义一致性**：status / diff / index / commit / history / refs / stash / conflict 用**同一套 fixtures** 覆盖 local、ssh、wsl 三种 target（target 只是入参）。这样 Phase 6 就能对语义下结论，而不必依赖尚未实现的传输。
2. **`connections` check 模块**负责 SSH 侧适配的反例：主机密钥、远端路径（非 ASCII、空格、前导 `-`）、quoting、断线后"命令可能已执行"的 reconcile 优先于重放（INV/P7-01）。
3. **`platforms` check 模块**负责 WSL 适配 + **ADR-003 的能力门控反例**：macOS 上 `wsl=false` 时客户端禁用并给出原因、服务端拒绝调用；Windows 真机跑 WSL 路由。
4. 三处**共用** `git` 的一致性 fixtures：`connections`/`platforms` 不得另写一份 git 行为断言。这条是本 ADR 的主要约束目的——否则同一语义会在三个门禁里各自漂移，而漂移只能靠人读 diff 发现。

## 被否决的方案与理由

- **保持现状各自负全责**：remote/wsl 的 git 语义在 `connections`/`platforms` 各测一遗。阶段出口最干净，但同一语义三处实现，且 macOS"不支持 + 原因"无人把守。
- **105 条全部由 `git` 门禁统管**：语义只测一遗，但 Phase 6 的门禁会要求 Phase 7 才存在的传输实现——结局要么把 P7 提前（破坏阶段出口），要么在 macOS 上默默 skip 关键用例，而 §5.4 明确禁止拿 skip 当通过。

## 后果

- P6-04 的 `pnpm native:check git` 必须包含 target 参数化的 fixtures（不是只测本机）；P7-01/P7-02 的门禁引用同一份 fixtures 文件，不复制断言。
- `check_module` 因此成为**受 ADR 约束的归属字段**，与 `category`/`rpc_method` 同级：改动它需要理由，不能跟着源码搬家顺手改。
- WSL 能力的"不支持"验收在 `platforms` 有一处唯一归属，Phase 7 出口对它可机械判定。
