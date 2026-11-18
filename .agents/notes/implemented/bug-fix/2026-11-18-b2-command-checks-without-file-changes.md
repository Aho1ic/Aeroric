# Agent Note: B2 没改文件的回合也跑任务的命令检查

Status: implemented

## Problem

[调研落地批次 B](../../proposed/feature/2026-11-15-adoption-b-acceptance-and-artifacts.md) 的 B2。回合结束后的自动检查（`TurnChangeTracker::turn_ended` → `AutoChecker::start`）有一个前提：前后两次快照比出了文件差异。下面三种回合一律不检查，验收里写的命令也不跑：

- 回合只跑了命令；
- 只改了仓库外的东西；
- 工作目录不是 Git 仓库。

带验收命令的子任务、待办，在这几种情况下没有任何检查记录，看起来像"没出错"。

## Decision

- **检查不再以有文件变化为前提**：
  - 回合正常完成（`Completed`）就进 `AutoChecker::start`，多传一个 `CheckTrigger`；
  - 有差异：`FileChanges { revision }`，`revision` 是回合结束时的树对象 id；
  - 比得出但没有差异：`CommandsOnly`，备注"这一轮没有文件变化，只跑了验收里的命令检查"；
  - 比不了（不是 Git 仓库或读不到快照）：`CommandsOnly`，备注"无法比较文件（不是 Git 仓库或读不到快照），只跑了命令检查"。
- **仅命令方式只跑任务自己的命令检查**（未关闭义务里的 `check`，即 `extra`）：
  - 项目自动发现的测试、诊断不跑，没有新改动可看；
  - 任务没有命令时什么也不启动，和以前一样。
- **协议**：`autoCheck` 更新加三个可选字段 `trigger`、`note`、`inputRevision`，省略时就是旧格式。`inputRevision` 先记下来，B3 判断验收是否过时要用。
- **App**：
  - 检查行标题下显示备注；
  - "把失败发给 Agent"的开头改成"上一轮之后运行的验收命令检查没有通过"；
  - "结果已过时"原来按回合 id 判断，一条较新的仅命令检查会被较早的文件变化判成过时。改为按更新的先后：检查之后又有文件变化才算过时。

## Alternatives considered

**仅命令方式也跑项目自动发现的测试。** 最强的理由是检查更全。没选：没有改动时项目测试的结论和上一次一样，每轮都跑一遍整个测试套件浪费时间；任务明确要求的命令才是这一轮要回答的问题。

**输入版本和命令都没变时沿用上一次的结果。** 最强的理由是省时间（方案笔记的风险一节提过）。没选：第一版先保证"每个完成的回合都有检查记录"；命令本身可能依赖仓库外的状态，沿用要另外设计，留到 B3 判旧时一起看。

## Consequences

- **收益**：写了验收命令的任务，每个正常完成的回合之后都有一条检查记录，并写明这一轮为什么只跑了命令。
- **代价**：带命令义务的任务，只回答问题的回合也会跑一次这些命令。
- **验证**：
  - `turn_changes` 新增 `a_turn_without_file_changes_still_runs_the_tasks_commands`：Git 仓库里没有改动、普通文件夹各跑一次，都只跑了任务的命令，带对应备注和 `trigger: commandsOnly`；没有命令时不启动；
  - Swift 新增 `testACommandsOnlyCheckIsNotStaleUntilFilesChangeAgain`；
  - 契约检查：18 份文档仍逐字一致（新字段可选）；
  - 检查行的备注显示待目检，记在路线文档 §8。
