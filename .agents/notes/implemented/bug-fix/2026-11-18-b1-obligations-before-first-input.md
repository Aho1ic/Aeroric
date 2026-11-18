# Agent Note: B1 验收义务先于子任务的第一条输入安装

Status: implemented

## Problem

[调研落地批次 B](../../proposed/feature/2026-11-15-adoption-b-acceptance-and-artifacts.md) 的 B1。带验收条件的委派原来是这样启动的：

- core 先调 `host.start_child`，宿主建任务并发出第一条输入；
- 返回成功后，core 才把验收条件作为义务装到子任务上（`add_obligations`）。

子任务很快结束时，它可能在义务存在之前就被算作完成，验收栏是空的。

另外两处：

- `add_obligations` 落盘失败只写日志，子任务照常启动，义务只在内存里，重启就没了；
- 宿主的"收下澄清卡"（`clarify_accept`）也装义务。顺序本来就是先装再继续，但同样不管落盘是否成功。

## Decision

- 子任务的 id 在委派开始时已预先选定（`card.child_task_id`），所以在调 `start_child` 之前，就按这个 id 装义务；
- 新增 `CheckpointSink::install_obligations`：
  - 返回是否写到了盘上；
  - 写失败时把内存里的义务恢复原样；
  - 旧的 `add_obligations` 删掉，只留这一条路。
- 装不上：委派卡记失败，原因"没能启动：验收条件没存下来（…）"，子任务不启动，上级照常收到失败。
- `start_child` 失败：用 `remove_obligations` 删掉已装的义务，这个 id 以后重试时不会带着旧义务。
- 宿主启动的子任务 id 和预选的不同：把义务搬到实际 id 上，再从预选 id 上删掉。
- `clarify_accept` 改用 `install_obligations`，写失败就拒绝这次请求（`IoError`），会话不继续。
- 审查循环和讨论不装义务，不用改。

## Alternatives considered

**`start_child` 里先建任务、装义务，再发第一条输入。** 最强的理由是"装"和"发"在同一个地方，顺序一眼可见。没选：义务属于 core 的检查点，宿主那边要多一个回调才能装；id 本来就预先定好了，在 core 里先装更简单。

**装失败时照样启动，只在卡片上警告。** 最强的理由是不挡住用户的工作。没选：没存下来的义务重启后就没了，子任务会在没有验收的情况下被算作完成，这正是 B1 要消除的情况；如实失败，用户可以重试。

## Consequences

- **收益**：任何带验收条件的子任务，第一条输入发出时义务已经在盘上；存不下来时如实失败。
- **代价**：委派启动多一次检查点落盘，在 `start_child` 之前。
- **验证**：
  - delegation 新增三个测试：
    - `obligations_are_installed_before_the_child_starts`：假宿主在 `start_child` 时数到 2 条义务；
    - `a_child_whose_obligations_cannot_be_saved_does_not_start`：检查点目录被一个文件占住，卡片失败、子任务没启动、内存里也没有义务；
    - `a_child_that_fails_to_start_leaves_no_obligations`：先装上，启动失败后删干净；
  - 没有界面改动，不需要目检。
