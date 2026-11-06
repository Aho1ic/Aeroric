# Agent Note: 任务元数据入口也走串行闸，改名不再被并发写盖掉

Status: implemented

## Problem

委派端到端测试 `a_delegation_runs_in_a_child_and_its_result_comes_back` 偶尔失败：子会话的记录里没有 `name`，可是改名明明在启动之前就执行过。原来以为大约 1/13 才失败一次，到 2026-11-05 机器负载高时，单独跑也有大约 2/5 会失败。

原因在 `TaskService`。`mutate`、`create`、`begin` 等入口都会在串行闸 `serial()` 里把“读取 → 决定 → 提交”一次做完。但公开的元数据入口只是读取快照、跑 reducer，然后直接提交，不拿这个闸。这些入口包括 `rename`、`toggle_star`、`archive`、`unarchive`、`attach_session`、`detach_session`、`set_worktree`、`discard_worktree`、`record_diff_stats`。

于是改名可能和另一个 `mutate` 交错执行：双方各自读到同一份快照，后提交的一方把先提交的那次修改覆盖掉。在这个测试里，委派标记子会话、子会话状态更新这类写入用的是 `mutate`，改名因此偶尔丢失。

## Decision

- 给上面这几个公开入口各自套一层 `self.serial(...)`。
- 内部的 `metadata_edit`、`metadata_batch`、`metadata_one`、`run_rename` 保持不拿锁。原因：`run_intent` 已经持有串行闸再调用它们，而串行闸不可重入，在它们内部拿锁会死锁。
- 上锁前查过调用方：`service.rs` 内部没有调用这些公开入口的地方。宿主的调用来自 RPC 处理，以及 `live.rs` 的会话回调（它们都不在持锁期间触发）。全量测试也没有出现卡死。

## Alternatives considered

- **只在测试里加长等待或重试**：丢写是真实存在的问题，用户同时改名、归档或标星时也会撞上，不能只修测试。
- **在 `finish_intent` 提交前比对快照指纹，不一致就重试**：改动更大；所有入口都已经有 `serial` 这一个闸，直接用它就够了。

## Consequences

- 元数据写入和其他任务写入现在完全串行，同一进程里不会再互相覆盖。锁只在内存里持有，写一次文件的时间也很短，对性能没有可见影响。
- 以后新增公开入口，同样要在最外层拿 `serial`，而且不能在已经持锁的路径里再调用它。

## Verification

- 修复前：这个测试单独跑 5 次失败 2 次，整套会话测试跑 3 次失败 2 次。
- 修复后：这个测试单独跑 8 次全部通过，整套会话测试（33 条）连跑 3 次全部通过。门禁全量也通过。
