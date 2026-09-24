# ADR-033：ADR-023 的移交面是 28 处而不是 8 处，且一次性全交（含 DSH 那 9 处）

- 状态：已接受（Task P2-05 门禁，Q62 —— 用户拍板）。**本 ADR 更正 ADR-023 与 `reviews/tauri-adapter.md` §3 事实修正第 3 条里的数量口径**，不改变 ADR-023 的决定本身。
- 日期：2026-09-24
- 相关：ADR-023（core 唯一发射者）、ADR-021（writer 只在 macOS 递交）、ADR-025（资源型/意图型寻址）、ADR-016（账本权威在 core）

## 背景

计划与评审文档一路写着"要删的 legacy 发射点是 ~8 处（包装函数 4 个调用点 + `task-session` 6 处字面量）"。
本轮真要动手前用 `command-map.json` 的 `events[].emitters` 逐条数了一遍，那个数是错的，而且错得会让
这一步**做完仍然有两个发射者**：

| 事件 | 冻结工件里的 emitters | 其中 core 的 sink | 其中 legacy |
| --- | --- | --- | --- |
| `task-status` | 23 | 1（`core_adapter.rs:446`） | **22** |
| `task-session` | 7 | 1（`core_adapter.rs:452`） | **6** |

22 处 legacy `task-status` 的分布：`dsh_webui.rs` 9、`pty.rs` 5、`ssh.rs` 3、`wsl.rs` 3、
`session.rs` 1（就是 `emit_task_status` 包装函数自己）、`event_watcher.rs` 1。

原来那个"8 处"只数到了 `emit_task_status` 的调用点和 `task-session` 的字面量，**漏掉了所有绕过包装
函数直接 `.emit(...)` 的字面量**——也就是漏掉了 pty/ssh/wsl/dsh_webui 的全部 20 处。这属于本仓库反复
踩过的那一类：一个从没被任何脚本重新派生过的散文数字被当成事实往下用。

顺带量出来的第二条事实，直接决定"抑制闸放在哪"：`is_task_active` 这道抑制**今天只覆盖 3 处**
（`session.rs:517/650/668` 经 `emit_active_task_status`），其余 **25 处都是无条件发射**。
所以"哪些事件被抑制"根本不是一条事件级策略，而是 3 个调用点上的局部行为。

## 决策

1. **移交面按实测 28 处登记**（22 + 6），并逐条以 `command-map.json` 的 `events[].emitters` 为准，
   不再以散文数字为准。判据也照这个字段来：两个计数各自必须**降到 1**（只剩 core 的 sink）。
2. **一次性全交，包括 `dsh_webui.rs` 那 9 处**。用户拍板时明确否掉了"只交 core 今天真能驱动的那部分"
   这条窄路（与 Q44 同方向的取舍，用户一贯选完整的那条）。
3. 因此本决策**带着一条必须一起交付的前置**：DSH 的状态机今天不在 core 里（ADR-021 下 writer 只在
   macOS 递交，而 `LegacyTaskExecutor::launch` 对 dsh 直接返回 `Unsupported`，理由是
   `workspace_id`/`prompt_mode`/`client_time_zone` 进不了 `LaunchRequest`）。要删 dsh 的 9 处发射点，
   必须先给它建一条 **runtime → core 的观察通道**（`TaskRuntimeObserver` 一侧），
   让 dsh 的状态变化作为**观察**进入 core 的 reducer，再由 core 唯一发射。
   等价于把 P3-05 的"DSH host 变成 core 服务"里**通知这一半**提前到 P2-05 做掉；DSH 的启动路径本身
   仍然走它自己的命令，不在本 ADR 范围内移动。
4. **禁止的实现方式**：从 dsh 的 9 个点直接改调 core 的 sink（那等于换个发射者而不是收口，
   状态仍未过 reducer），也禁止为了让计数好看而把任何一处改成"静默不发"。
   每一处删除都必须能指出"它现在经由哪个观察进入哪个 reducer 转移"。

## 更正（2026-09-24，Q64 → ADR-035）

本 ADR 的"现在就全交"落地时撞到一条 ADR-033 写下来时不知道的事实：**`TaskService::observe` 会落盘**
（`commit → save_covering → require_writer`，`storage/tasks.rs:73`），而写权按 ADR-021 只在 macOS 递交。
在 Windows/Linux 上直接删这 28 处，两个事件会全部消失——正是本 ADR §4 禁止的"静默不发"。

裁决（ADR-035）：把观察的**决策**与**落盘**解耦。core 在所有平台都是事件权威（决策只依赖读，
`load_stamped` 不需要写权），只在递交了写权的平台是数据权威；非 writer 平台"决策 + 发射 + 不提交"，
磁盘仍由旧客户端的保存路径写（这件事今天就是这样：28 处发射点没有一处负责落盘）。
由此发射面移交与 store 切换解耦，28 处不必等写权移交。

同时暴露一条新的前置：dsh 的运行从未经过 core 的账本，`live_for(task)` 恒为空，所以删除 dsh 那 9 处之前，
宿主必须先能把 dsh 的**存活代登记**进 core（ADR-035 后果第 3 条）。

## 后果

- P2-05 的剩余工作量比计划原口径大约 **3.5 倍**（28 处 vs 8 处），并且新增一条 core 侧端口接入
  （DSH 观察通道）。这是排期事实，不是缺口。
- "core 是唯一发射者"从今往后是**可核对的**：`events.schema.json` 的这两个数字等于 1，
  由 `tauri-adapter` 门禁断言，而不是靠 review 时数一遍。
- `is_task_active` 抑制闸的归属仍未定（Q63）。本 ADR 只固定了一条前提：它今天只管 3 处，
  任何"把它做成全局策略"的方案都会改变其余 25 处的行为，需要单独论证。
