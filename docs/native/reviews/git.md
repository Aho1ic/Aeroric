# P6-04 评审记录 — Git 全流程及危险操作确认（2026-10-02 收口）

## 1. 范围

P6-04 是 Phase 6 第四个 Task：core `git/` 模块（transport＋service 业务主体）
＋Git 十视图＋门禁 2 case。G15 前置（4 个未注册命令名）客户端面闭合。

## 2. 实现链（两刀）

- **刀一（core git 模块）**：`core/crates/aeroric-core/src/git/{mod,service,
  transport}.rs`——transport（**argv＋`--`、选项注入守卫**：`validate_pathspec`/
  `validate_revision` 拒 `-` 开头/NUL/换行；read timeout kill child 不留持锁
  进程；**相同 repo mutating 串行化** RepoLocks——并发 mutating 撞 index.lock）
  ＋service（status porcelain -z 解析 verbatim、show_diff/working_diff
  **不截断**、stage/unstage/commit/commit_allow_empty/log/branches/checkout/
  create_branch/push/pull/fetch/discard_files/stash 全套/conflict_files/
  preview/resolve_conflict/blame/branch_graph/commit_detail——push/discard/
  冲突覆盖需独立确认由调用方 UI 承载）。`tests/git_service.rs` 9 用例
  （**临时 repo＋本地 bare remote**）。
- **刀二（Git 十视图）**：`AeroricUI/Git/{GitStore,GitViews}.swift`——
  GitStore（变更/提交/分支/stash 簿记＋冲突路径高亮＋diff 不截断）＋GitPanel
  ＋ChangeList（staged/unstaged 分节＋冲突高亮）＋StageActions（commit＋
  discard All 二段确认）＋CommitSheet＋HistoryView＋DiffView＋BranchGraph＋
  BlameView＋StashView＋ConflictResolver（ours/theirs 侧提取）。`GitStoreTests`(5)。

## 3. 验证实测（2026-10-02）

| 验收 | 结果 |
|---|---|
| `pnpm native:check git`（新注册） | 2 case / 14 用例 ok（git_service 9＋GitStoreTests 5） |
| `cargo test -p aeroric-core` | 527 passed / 0 failed |
| `swift test --package-path macos/AeroricKit` | 174 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（git floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |

## 4. 验收原文逐条对照

- `tests/git_service.rs`(9)＋`GitStoreTests.swift`(5) ✅；`pnpm native:check
  git` ✅。
- 临时 repo＋本地 bare remote：untracked ✅、rename（空格文件名完整）✅、
  冲突 ✅、空提交（--allow-empty）✅、失败 push（无 remote）✅、完整 remote
  roundtrip（push→fetch）✅；不触碰 Aeroric 真实暂存区（全部 temp repo）✅。
- 参数以 `-` 开头、换行、空格文件名不成为选项注入（`validate_pathspec`/
  `validate_revision` 单测）✅；取消 commit 后 index 不被额外改变（失败
  commit 前后 status 相同，单测覆盖）✅。
- `grill-me [git]`：已真实调用（用户：接受并收口）✅。

## 5. G15 前置的落实

- 客户端面已闭合：**不在 Swift 直接调用 `git`**（业务主体在 core git 模块，
  Action 1 明确）；4 个未注册命令名（`remote_git_checkout_branch`/
  `wsl_git_checkout_branch`/`remote_git_create_branch`/`wsl_git_create_branch`）
  的业务主体（checkout/create_branch）在 core git 模块有实现，本地/远程/WSL
  目标形态由 ProjectTarget 承载。
- 宿主面接线 → P7 逐项接（用户确认记账）。

## 6. 已记录的边界与挂账

- push/fetch/pull 的凭据助手（不会因等凭据而挂住的策略）→ P7 接线时接。
- 分块操作（hunk-level stage）→ P7 接线时接（文件级 stage 已交付）。
