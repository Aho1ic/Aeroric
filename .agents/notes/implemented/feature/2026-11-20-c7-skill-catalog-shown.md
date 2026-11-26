# Agent Note: C7 第二步：技能目录页、会话里的挂载列表，以及各扩展入口的现状

Status: implemented

## Problem

[C7 第一步](2026-11-20-c7-skill-catalog-and-mounts.md)在宿主上做了 `skills.catalog` 和 `skills.mounted`，但 App 还看不到（`main@4e776b69`）。方案 C7 第 5 条还要求：逐个检查 Skill Hub、MCP 管理、插件管理的入口，如实记录，不留点了没反应的按钮。

逐个读代码后的结果：

- **侧栏"扩展"一栏**的状态是 `.comingSoon`，只显示"扩展 · 即将支持：技能、MCP 服务器与 Hook 的管理"。下面的三个视图不会显示。
- **`SkillHubView`**："安装 Skill…"和"停用"两个按钮在 App 里没有传处理函数，`ExtensionStore.ingestSkills` 没有调用方，宿主也没有对应方法。
- **`MCPSettingsView` / `HookStatusView`**：同上。[A10 笔记](../bug-fix/2026-11-17-a10-font-registration-and-mcp-save.md)已记过 MCP 保存没有调用方。
- **`PluginManagerView`**：从未在任何界面里使用。
- **内置技能**的开关在"Agent 配置 → 技能"（`BuiltinSkillsPane`），这是能用的。

所以现在没有"点了没反应"的按钮，因为根本看不到；但"扩展"一栏一直是空的承诺，技能目录也没有地方放。

## Decision

**"扩展"一栏改为技能目录**（`AeroricUI/Skills/SkillCatalog.swift`，`TaskWorkspaceView` 的 `.extensions`）：

- 状态从"即将支持"改为部分可用，顶部写"这里列出 Agent 能用到的全部技能。MCP 服务器、Hook 和插件的管理尚未提供。"
- **页面**（`SkillCatalogPage`）：
  - 按来源分组：内置、记忆沉淀、你自己的目录、Claude Code 插件。组标题悬停说明这种来源是什么、在哪里管理。
  - 每行写：名字（与 id 不同时附 id）、版本徽标（内置写"随应用内置"，其他写版本加内容哈希前 6 位）、摘要、哪些 Harness 会得到、说明（如"在 Claude 里被你自己的同名技能盖过"），以及路径。
  - 不生效的名字变灰，说明用警示色。
- 可以选一个项目，让项目自己的技能目录和项目专属的记忆技能也算进来；有搜索框。
- 这一页只读，不提供安装、删除。各来源的管理入口不变：内置的开关在 Agent 配置，记忆技能在记忆页。

**会话信息里的"技能"**（`SessionSkillsSection`，挂在 `SessionInfoCard` 的最后）：

- 一行汇总，例如"31 个 · 9:41 启动 · 共启动 2 次"。C7 之前启动的会话写"未记录（这个会话在记录技能之前启动）"。
- 点开逐个列出最近一次启动的技能：名字 · 原因（默认挂载 / 你采纳的 / 你自己的目录 / Claude Code 插件）· 版本。
- 启动过不止一次时，先写与上一次相比的变化：某技能换了版本、新增、去掉。这样能看出会话中途是否换过技能。
- 数据由 App 环境里的 `SkillCatalogBook` 提供，打开会话时读一次 `skills.mounted`。

**旧视图不删，只是不再有入口**：`SkillHubView`、`MCPSettingsView`、`HookStatusView`、`PluginManagerView` 留在 `ExtensionViews.swift` 里。各入口的现状写进 `docs/subsystems/macos-app.md` 的"扩展入口的现状"一节。UI 测试里"扩展"一栏期望的标识改为 `skills.catalog`。UI 测试仍暂停，没有运行。

## Alternatives considered

**删除 Skill Hub、MCP、Hook、插件这几个旧视图。** 最强的理由是不留死代码。没选：

- `docs/native/command-map*.json` 把它们登记为迁移目标界面，删掉要连带改迁移映射；
- `ExtensionStore` 的信任判定和 env 掩码有单元测试，将来接 MCP 时还要用。

现在它们没有任何入口，不会被用户点到，现状已写进子系统文档。

**技能目录放进"Agent 配置 → 技能"，与内置技能开关合并。** 最强的理由是技能相关的都在一处。没选：那一页是内置技能的开关和依赖检查，按 Harness 显示；目录要跨四种来源、可选项目、可搜索，塞进去会让开关不好找。"扩展"一栏本来就是为技能留的位置，一直空着。

**会话信息里列出全部启动记录。** 最强的理由是信息最全。没选：恢复多次的会话会有十几次启动，每次几十个技能。只列最近一次，再加与上一次相比的变化，已能回答"现在用的是什么、中途换过没有"。

## Consequences

- **收益**：
  - 用户能在一处看到 Agent 会用到的所有技能、来源、版本和谁盖过谁；
  - 任意一次会话能看到它挂了什么；
  - "扩展"一栏不再是空的承诺，没接上的部分如实写明。
- **局限**：
  - 目录页只读；
  - 会话信息只显示 C7 之后启动过的会话；
  - MCP、Hook、插件管理仍未提供。
- **验证**：
  - Swift 测试新增 2 个（`SkillCatalogTests`）：
    - 目录的解码、按来源分组的顺序、同名两条各自独立、版本文字、搜索过滤、读取失败时保留已读内容；
    - 会话的启动记录解码，汇总文字（含"未记录"和"读取中"），两次启动之间换版本、新增、去掉的说明，原因和来源的文字。
  - 界面待目检，记在路线文档 §8。
