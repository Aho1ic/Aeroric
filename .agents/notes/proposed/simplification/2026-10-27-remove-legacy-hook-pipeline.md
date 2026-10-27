# Agent Note: 移除 Claude/Codex hook 管线，先清理旧版装下的 hook

Status: proposed

## Problem

Agent 改为结构化协议驱动后（见 [结构化协议](../../implemented/architecture/2026-10-25-structured-agent-protocols.md)），任务状态来自协议事件，hook 管线已无生产消费方：

- 写侧：`core/crates/aeroric-core/src/agents/managed_home/`（`claude_hooks.rs`、`codex_hooks.rs`、`resources.rs`、`aeroric-hook.mjs`）构造 Claude settings 注入块、Codex TOML 注入块与共享 hook 脚本——全仓没有调用方。
- 读侧：`core/crates/aeroric-core/src/sessions/hooks.rs` 的 `HookEventReader` 与路径布局——只有 `tests/hook_reader.rs` 在用。

但旧版本曾把 hook 注入托管 Claude home 的 settings 与 `~/.codex/config.toml`，并把 `aeroric-hook.mjs` 写到 `~/.aeroric/hooks/`。清掉这些注入的唯一代码就是这里的 `uninject_claude_value` / `uninject_codex_text`，而它们今天也没被调用：老用户的 Claude/Codex 每次事件仍会执行一个需要 Node 的脚本，写一份无人读取的事件文件。直接删除代码会让这份残留永远无法自动清掉。

## Proposal

1. 宿主启动时（与 omp 旧 hook 清理同一处）调用 `uninject_claude_value` / `uninject_codex_text` 清除带 Aeroric 标记（`AERORIC_MARKER_FIELD` / `LEGACY_CLAUDE_MARKER_FIELD`、`CODEX_BEGIN…CODEX_END`）的注入块，并删除 `~/.aeroric/hooks/aeroric-hook.mjs` 与事件目录；只动带标记的内容，用户自己的 hook 原样保留。
2. 清理逻辑保留一个版本周期后，删除 `managed_home/` 的构造函数、`aeroric-hook.mjs`、`sessions/hooks.rs` 与 `tests/hook_reader.rs`，只留下标记常量与 uninject 函数。
3. 同批更新 `docs/subsystems/agents.md` 与结构化协议笔记中关于 hook 的段落。

## Alternatives considered

- **现在就整块删除**：净删最多，代码最干净。否掉是因为会丢掉唯一能识别并移除旧注入块的逻辑，残留 hook 变成永久的。
- **保持现状**：零风险改动。否掉是因为老用户的 agent 持续执行无用脚本，且无调用方的代码会继续误导后来者以为 hook 仍在工作。

## Acceptance criteria

- 用隔离 HOME 的夹具：带 Aeroric 标记的 Claude settings 与 Codex config 在宿主启动后被还原，用户自定义 hook 不变；`~/.aeroric/hooks/` 被清空。
- 构造函数与读侧删除后，`cargo check --workspace --tests` 与 `pnpm native:check storage` 通过，`storage-core-rust` 的地板不需要下调（该 case 统计整个 aeroric-core 包）。

## Risks

- 误删用户自己写的 hook：只按标记识别，标记缺失的条目一律不动。
- 用户在 Aeroric 之外仍依赖 `~/.aeroric/hooks/` 的事件文件：概率低，清理前在发布说明里注明。
