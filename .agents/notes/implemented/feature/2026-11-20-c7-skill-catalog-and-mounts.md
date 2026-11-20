# Agent Note: C7 第一步：技能目录，以及每次启动挂了哪些技能

Status: implemented

## Problem

[批次 C 方案](../../proposed/feature/2026-11-15-adoption-c-memory-gateway-skills.md)第 C7 片要求：能列出全部技能及其来源、版本和摘要；任意一次会话，能查到它启动时挂载的每个技能及版本。动手前（`main@f799d71d`）：

- **四种来源各管各的，没有统一视图。**
  - 内置技能编译进 App（`extensions/builtin_skills.rs`），"技能"页的 `builtinSkills.list` 只列内置的；
  - 记忆沉淀的技能有自己的索引（`learned/index.json`）；
  - 用户自己目录里的技能（`~/.claude/skills`、项目的 `.agents/skills` 等）只在判断"被用户同名技能盖过"时读名字；
  - Claude Code 插件里的技能，Aeroric 完全没读。
- **挂了什么没有记录。** `builtin_skills::mount` 每次启动时决定链接、插件或复制哪些技能，做完不留痕迹。会话启动后再升级技能，就说不清那次会话当时用的是哪一版。
- **B7 重跑对比只能写"技能未记录"。** [B7 笔记](2026-11-19-b7-rerun-inputs.md)里，技能版本一律为空。

## Decision

**统一投影，各自存储不动**（协议 `aeroric-protocol/src/skills.rs`，核心 `extensions/catalog.rs`）：

`SkillDefinition` 的字段是：名字、来源、可读版本、内容短哈希、路径或"内嵌"、摘要、哪些 Harness 会得到它、是否生效，以及一句说明。四种来源各自这样投影：

- **内置**：版本写"随应用内置"，哈希是整个内置包的哈希，没有路径，`embedded`。摘要用 Aeroric 自己的中文说明。
- **记忆沉淀**：版本是 `v{n}`，路径是当前版本的文件夹，摘要读 `SKILL.md` 的 `description`。
- **用户目录**：按 `user_skill_dirs` 读每个 Harness 的目录，同一个文件夹只列一次，写明哪些 Harness 会读到它。Aeroric 自己的复制和链接不算。
- **Claude Code 插件**：读 Claude Code 自己的 `~/.claude/plugins/installed_plugins.json`，第 1、2 版格式都认，格式变了就当作没有。每个插件 `skills/` 下的技能以 `插件:技能` 命名，版本用插件版本，只给 Claude。

**同名时都列出，写明谁生效**，规则与挂载一致：

- 用户自己的同名技能盖过内置的。只盖过部分 Harness 时，内置的仍是生效的，说明写"在 Claude 里被你自己的同名技能盖过"。
- 内置的盖过记忆沉淀的。
- 关闭的内置技能写"已关闭"。
- 项目专属的记忆技能写"只在项目 … 里"；查别的项目时不生效。

**挂载计划和记录**：

- `catalog::plan(data, harness, cwd, home)` 给出一次启动会得到的技能，每个附原因。用的是 `mount` 同样的函数（`offered`、`learned_offers`、`user_skill_dirs`），不是另写一份规则。原因有四种：
  - 默认挂载（内置）；
  - 你采纳的（记忆沉淀）；
  - 你自己的目录（Agent 自己读）；
  - Claude Code 插件。
- 执行器在每次启动和恢复会话时（`launch` / `resume`，只限在这台 Mac 上运行的）调用 `MountLog::note`，记到 `<data>/skill-mounts/<任务>.json`：
  - 每次一条：时间、Harness、技能列表，只保留最近 20 次；
  - 写失败只记日志，不影响会话启动；
  - 任务 id 不是普通文件名时，按哈希存。
- 宿主新增：
  - `skills.catalog {projectId?}`：只接受已登记的项目，不接受任意路径；
  - `skills.mounted {taskId}`：每次启动的记录，最旧的在前。C7 之前启动的会话返回空，App 应写"未记录"。

**B7 读技能**：

- 原回合：取该回合开始前最后一次启动的记录。
- 重跑：按目标 Harness 和副本路径算一次 `plan`。
- 两边都写成"N 个 · 指纹"，指纹是全部"名字@内容哈希"排序后的哈希。相同就是一致；不同时，具体差在哪里由 `skills.mounted` 查。

## Alternatives considered

**在 `mount` 里顺手返回它挂了什么，由各启动路径记录。** 最强的理由是记录的就是真正做的事，不是再算一遍。没选：

- `mount` 在宿主的九个启动路径里调用，其中 Codex 的在 `before_codex_session`，只有工作目录，没有任务 id；
- 按配置启动的 Codex 服务器被所有项目共用，挂载发生在服务器启动时，不对应某个会话；
- 用户目录和插件里的技能也不经过 `mount`。

在执行器的 `launch` / `resume` 里，任务 id、Harness 和工作目录都齐了。规则用同一组函数，结果与实际挂载一致。

**记录每个技能的完整内容或全部文件哈希。** 最强的理由是最精确。没选：`SKILL.md` 的哈希已能分辨版本；内置技能整包一个哈希，与 App 版本一一对应。记更多只会让记录变大。

**B7 里列出每个技能的名字和版本。** 最强的理由是对比时一眼看出差在哪。没选：内置技能就有 27 个，并排显示会把对比表撑得没法看。指纹回答"一不一致"，细节由 `skills.mounted` 给。

## Consequences

- **收益**：
  - 技能从哪来、哪个生效、版本是什么，有了一个统一的回答；
  - 任意一次 C7 之后启动的会话，能查到当时给了哪些技能和版本，之后升级也不影响记录；
  - 重跑对比能判断技能是否一致。
- **代价**：
  - 每次启动多读一遍技能目录，再写一个小文件；
  - 读用户目录和 Claude Code 的插件清单，只读不写。
- **局限**：
  - 记录的是 Aeroric 给的和 Agent 目录里有的。Agent 实际有没有加载某个技能，Aeroric 看不到。
  - 远程会话不记。
  - 没有"角色要求"这种原因，目前没有按角色挂技能的机制，不编造。
  - 方案第 5 条（逐个检查 Skill Hub、MCP 管理、插件管理的入口）和第 6 条（App 界面）在第二步。
- **验证**：
  - 核心层新增 4 个测试（`extensions/catalog_tests.rs`，全在临时目录）：
    - 四种来源各一个：版本、哈希、摘要、路径各自正确；Claude 的启动得到四种，Codex 没有插件和 Claude 目录的技能；
    - 同名：用户的 `brainstorming` 与内置的两条都列出，说明写清楚；Claude 启动得到用户的，Codex 得到内置的；与内置同名的记忆技能不生效；关闭的不挂；
    - 项目专属技能只在那个项目里生效、被挂载；
    - 会话中途升级记忆技能：前一次启动仍记旧版本，回合取它开始前的那次启动，指纹不同；只留 20 次；奇怪的任务 id 不会写到目录外；没设置目录时什么都不写。
  - 宿主集成测试新增 1 个：`skills.catalog` 列出全部内置技能并标为内嵌，带项目也能列；`skills.mounted` 对没有记录的任务返回空，缺参数报错。
