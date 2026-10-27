# Agent Note: 会话文件路径 claim 的释放靠反查会话表

Status: implemented

## Problem

`claimed_session_paths` 保证"一份会话 jsonl 只能被一个持有者看着"。允许同任务多代在飞、转录收养、以及 `claude_gpt55` 这类跨族配置之后，不同运行可能引用同一份文件；若删掉一条会话条目就释放 claim，两个 agent 可能静默往同一文件写。

## Decision

- `claimed_session_paths` 保持路径集合形态，不改成持有者计数，也不按 `(task_id, generation)` 键（那样每代都能各自认领同一路径，互斥消失）。
- 释放的唯一规则：先删掉本次要销的会话条目，再反查各族会话表；只要还有任何条目引用这条路径，就不释放。反查集中在宿主函数 `release_session_path_if_unreferenced`，除它之外不存在释放 claim 的调用点。
- 退出收尾的顺序是硬约束：删会话条目 → 反查 → 释放 claim。

## Alternatives considered

- **持有者集合（`path → {RunKey}`）**：判定最快最局部。否掉是因为它是第二份"谁持有什么"的账；任何绕过会话表的删条目路径都会让计数漏掉，变成永远释放不掉的 claim。
- **删条目即删 claim（原样搬）**：改动最小。否掉是因为它赌"两代不会引用同一路径"，而收养与跨族配置正在制造这种场景，失效形式是静默的。

## Consequences

- **收益**：claim 的事实来源只有会话表。
- **代价与已知上限**：每次释放要扫描各族会话表，规模等于活任务数、释放频率等于退出频率，未建索引；若活任务数量级显著上升需要重访。
