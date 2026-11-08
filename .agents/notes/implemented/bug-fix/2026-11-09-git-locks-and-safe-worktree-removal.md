# Agent Note: Git 写操作共用一张锁表；任务 worktree 按路径删除、保留改动和未合并分支

Status: implemented

## Problem

New-addition-1102 复核（参照 worktrunk `src/git/remove.rs`、`src/commands/remove.rs`）指出两处：

- **锁形同虚设**：`RepoLocks` 按仓库根串行化写操作，防止并发撞 `index.lock`。但宿主每个请求都 `RepoLocks::default()` 或 `WorktreeService::new()` 新建一张表，同一仓库的两个写请求各拿各的锁，互不排队。宿主里这样的地方有 15 处 Git 写操作和 4 处 worktree 操作。
- **任务 worktree 删除不设防**：`WorktreeService::remove` 直接 `worktree remove --force` 再 `branch -D`：未提交的改动、没合并的分支一起丢；要删哪个分支由调用方传入，不核对它是不是这个 worktree 检出的分支。

`git.*_task_worktree` 目前没有界面调用，研究定级"接界面之前必须修"；用户把它放进了推迟清单，2026-11-09 夜间按推荐先做。

## Decision

- `RepoLocks::shared()` 是进程内唯一一张锁表；宿主所有 Git 写操作和 `WorktreeService::new()` 都用它。测试里的 `RepoLocks::default()` 照旧各自独立。
- `WorktreeService::remove(root, path, branch, force)` 返回 `WorktreeRemoved { branch, branchDeleted, branchKept }`：
  - 按路径定位：从 `git worktree list --porcelain` 找到这个路径（规范化后比较）和它检出的分支；不是本仓库的 worktree 就报错；传入的分支和它不一致就拒绝。
  - 有未提交的改动时拒绝，`force` 才丢弃（这时才带 `worktree remove --force`）。
  - 分支用 `branch -d`，只删已合并的；没合并的保留，`branchKept` 说明原因；`force` 才 `-D`。
- 宿主 `git.remove_task_worktree` 新增可选参数 `force`（缺省 false），成功时回上面的结构。

## Alternatives considered

- **只在宿主里留一个长期的 `WorktreeService`**：也能共享锁。没有采用：Git 的其他写操作（暂存、提交、切分支……）同样每次新建锁表，要一起管，一张进程级锁表最直接。
- **照 worktrunk 把删掉的目录先改名进 trash 再后台清理**：删除更快、可撤销。没有做：目前没有界面调用，等接界面时看是否需要。
- **保持 `-D`、由界面先问用户**：界面还没有。核心层先守住，界面再问也不会丢东西。

## Consequences

- **收益**：同一仓库的并发写操作真正排队；接界面后，误删工作区里的改动或没合并的分支要用户明确带 `force`。
- **代价**：宿主里不同仓库互不影响，同一仓库的写操作会互相等待（这正是锁的本意）；进程级锁表随仓库数增长，不回收（每个仓库一把锁，量很小）。
- **兼容**：`git.remove_task_worktree` 原来成功回 `null`，现在回一个对象；目前没有调用方。
- **验证**：`worktree_lifecycle::remove_checks_the_path_keeps_work_and_unmerged_branches`（分支对不上拒绝、脏工作区拒绝、未合并分支保留、`force` 删除）、`worktree_services_share_one_lock_table`；原有的往返用例改为断言已合并分支被安全删除；宿主 `worktree_services` 4 个用例照旧通过。
