# Agent Note: 宿主启动时清理旧版装下的 Claude/Codex hook

Status: implemented

## Problem

Agent 改为结构化协议驱动后（见 [结构化协议](../../implemented/architecture/2026-10-25-structured-agent-protocols.md)），任务状态来自协议事件，hook 管线已无生产消费方：`core/crates/aeroric-core/src/agents/managed_home/` 的构造函数与 `aeroric-hook.mjs` 没有调用方，`core/crates/aeroric-core/src/sessions/hooks.rs` 的 `HookEventReader` 只有 `tests/hook_reader.rs` 在用。

旧版本却在用户机器上留下了痕迹：`~/.claude/settings.json` 里带 `_aeroric_managed`（或旧产品名标记）的 hook 条目、`~/.codex/config.toml` 里 `CODEX_BEGIN … CODEX_END` 包住的块、`~/.aeroric/hooks/` 下的脚本与配置文件和 `events/`。这些注入让 Claude/Codex 每次事件都执行一个需要 Node 的脚本，写一份无人读取的事件文件。能识别并移除注入块的只有 `uninject_claude_value` / `uninject_codex_text`，直接删除代码会让残留永远清不掉。

## Decision

- 生产启动组合（`run_startup_with_secrets_and_legacy_agent_migration`，即 `aeroric-cli serve`）在数据兼容检查之后、listener 之前调用 `aeroric_core::agents::managed_home::remove_legacy_agent_hooks(paths.user_home())`（`legacy_cleanup.rs`）。它与 omp 明文 key 迁移同处，尽力而为：失败只记 stderr 日志，不挡启动。
- 清理范围与规则：
  - Claude：只在 `settings.json` 确有带标记的条目时才改写，移除带标记的条目，保留用户自己的 hook、其余键与空数组；文件不是合法 JSON 时原样不动并记入 `skipped`。
  - Codex：移除标记块；移除后的文本必须仍是合法 TOML 才写回，否则不动。
  - `~/.aeroric/hooks/`：只删旧版写下的文件名（`aeroric-hook.mjs`、`claude-settings.json`、`claude-runtime-fast{,-hooks}.json`、`codex-hooks.toml`）与 `events/`；目录清空后才删除目录，里面还有不认识的文件就保留。
  - 改写用户文件时同目录临时文件 + rename，沿用原文件权限；没有痕迹时不写任何文件，重复运行是空操作。
- home 取自 `AppPaths::user_home()` 而不是 `$HOME`，所以测试里以临时目录构造的 `AppPaths` 天然隔离，不会碰开发者的真实 `~/.claude`。
- 自定义 codex-like Agent 的包装脚本（`settings/agent_scripts.rs`）在片段文件缺失时静默跳过，`codex-hooks.toml` 删除后它们照常启动，且其 `CODEX_HOME/config.toml` 每次启动整体重写，不留残块。

## Alternatives considered

- **现在就整块删除 hook 代码**：净删最多，代码最干净。否掉是因为会丢掉唯一能识别并移除旧注入块的逻辑，残留 hook 变成永久的。
- **保持现状**：零风险改动。否掉是因为老用户的 agent 持续执行无用脚本，无调用方的代码也会误导后来者以为 hook 仍在工作。
- **清理放在每次启动 Claude/Codex 任务前**：只在真正用到时才动用户文件。否掉是因为宿主启动只有一处、天然串行，而任务启动是并发入口，多个任务同时改写同一份 `settings.json` 需要额外加锁。

## Consequences

- **收益**：升级后的首次启动即移除旧注入，用户的 Claude/Codex 不再每次事件都拉起 Node；用户自己的 hook 与配置不受影响。
- **代价与已知上限**：宿主每次启动多读三个文件；清理改写的是用户 home 里的配置文件，这是除托管 home 之外 Aeroric 唯一会写的用户 agent 配置，且只限于移除自己的标记内容。若用户把 Aeroric 标记手工复制进自己的 hook，那些条目也会被移除。

## Verification

- `cargo test -p aeroric-core --lib managed_home`：带标记条目被删而用户 hook 保留、干净文件不被改写或重排、坏 JSON 原样保留并报告、Codex 块移除后其余配置仍可解析、hooks 目录只删已知文件且有陌生文件时保留、改写保留 0644 权限。
- `cargo test -p aeroric-host --test host_lifecycle production_startup_removes_hooks_older_builds_installed`：生产启动在隔离 home 上移除 Codex 块与 hooks 目录。

## Deferred

- 清理逻辑随一个发布周期送达用户后，删除 `managed_home/` 的构造函数（`build_claude_settings_value`、`build_codex_block`、`inject_codex_text`、`hook_command`、`with_claude_fast_mode`、`toml_quote`）、`resources.rs` 与 `aeroric-hook.mjs`、`sessions/hooks.rs` 与 `tests/hook_reader.rs`、`agent_scripts.rs` 中追加片段的脚本段，只留标记常量、两个 uninject 函数与本清理。`storage-core-rust` 统计整个 aeroric-core 包，删除 `hook_reader` 的 7 个用例不需要下调地板。
