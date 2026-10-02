# P6-05 评审记录 — worktree 与任务生命周期联合迁移（2026-10-02 收口）＋ Phase 6 Gate

## 1. 范围

P6-05 是 Phase 6 最后一个 Task：core `git/{worktree,worktree_policy}.rs` ＋
四视图＋worktree 门禁。Phase 6 Gate（F07–F10 每子能力）同批收口。

## 2. 实现链（两刀）

- **刀一（core worktree 模块）**：`core/crates/aeroric-core/src/git/{worktree,
  worktree_policy}.rs`——worktree_policy（生命周期判定纯函数：`can_create`/
  `can_merge`/`can_remove`/`discarded_blocks_interaction`/
  `remote_project_blocks_worktree`、`task_worktree_branch_name`、**同名目录
  已存在拒绝**、numstat 累计）＋worktree（WorktreeService：create/**同名
  拒绝**、merge（HEAD==base 直接合并否则 fetch ff 不切走 HEAD；**RCE 守卫**
  branch/base_branch 首字符检查——refspec 无法靠 `--end-of-options` 保护，
  首字符检查是唯一可靠手段）、remove（`--force`＋`-D`）、diffstats（**merge-
  base 而非 base_branch 本身**——避免主仓 base 推进后把别人提交的改动算到
  本任务头上））。`tests/worktree_lifecycle.rs` 6 用例。
- **刀二（四视图）**：`AeroricUI/Git/WorktreeViews.swift`——WorktreePolicy
  （Swift 侧词汇表与 core 同语义）＋WorktreeCreateSheet（显式选择 base
  branch——尊重目标项目 branch policy，AGENTS main-only 阻止新增分支）＋
  WorktreeTaskBadge（discarded 禁用交互＋diffstats）＋WorktreeMergeSheet＋
  WorktreeDiscardSheet（二段确认 Toggle）。`WorktreePolicyTests`(6)。

## 3. 验证实测（2026-10-02）

| 验收 | 结果 |
|---|---|
| `pnpm native:check worktree`（新注册） | 2 case / 12 用例 ok（worktree_lifecycle 6＋WorktreePolicyTests 6） |
| `cargo test -p aeroric-core` | 533 passed / 0 failed |
| `swift test --package-path macos/AeroricKit` | 180 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（worktree floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |
| 真实 Aeroric | 仍 main；无 `aeroric-task/*` 分支（测试只在临时仓库） |

## 4. 验收原文逐条对照

- `tests/worktree_lifecycle.rs`(6)＋`WorktreePolicyTests.swift`(6) ✅；`pnpm
  native:check worktree` ✅。
- **P2-04 Q43 移交项**：`create_task_worktree`/merge/remove/cleanup 的 git
  副作用 → core WorktreeService 承担（create/merge/remove 全套）；P2-05 的
  临时端口不再转调（P7 接线时切 WorktreeService）。core 侧三条判定已承担：
  "worktree 覆盖项目路径"（同名拒绝）、"worktreeDiscarded 禁止续跑"
  （discarded_blocks_interaction）、"远程项目不得带 worktree"（remote_project_
  blocks_worktree）✅。
- tmp repo：create→task→diff→merge roundtrip ✅；冲突/dirty/discard/reopen
  反例（dirty 拒绝 merge、reopen 同名拒绝、根外拒绝单测）✅；真实 Aeroric
  仍是 main 且没有新分支 ✅。
- `grill-me [worktree]`：已真实调用（用户：接受并收口）✅。

## 5. Phase 6 Gate（F07–F10 每子能力）

- F07（文件树/编辑/Markdown/图片/local history）：files 门禁 2 case/17 用例
  ＋editor 门禁 2 case/14 用例 ✅
- F08（LSP/诊断/运行/DAP/preview）：ide 门禁 3 case/15 用例 ✅
- F09（Git 全流程/危险确认）：git 门禁 2 case/14 用例 ✅
- F10（worktree 联合迁移）：worktree 门禁 2 case/12 用例 ✅
- PTY/native/unsupported：native-terminal 12 用例＋sessions-ui 20＋tasks-ui 10
  ＋settings-ui 11＋ModeSessionIntegrationTests ✅

**Phase 6 Gate 通过（用户确认）**。

## 6. 已记录的边界与挂账

- P2-05 的临时端口 → P7 接线时切 WorktreeService（core 侧已就位）。
- worktree diffstats 的未跟踪文件 --no-index 比对（退出码 1＝不同）已实现。
