# Agent Note: 技能提案收件箱（B5-3b）：高置信经验升格为技能、审批、版本与回滚、按项目挂载

Status: implemented

## Problem

B5-3a 之后，经验只有在 Agent 主动调用 `memory_recall` 时才会被用到。被反复验证过的解法应该变成会话一开始就能用的技能，但不能由模型自己改技能，要走 A06 那套"提案、审批、版本、回滚"（路线文档 §6.5）。owner 2026-10-31 定了 Q-M22–Q-M26，全部按推荐。第一版只做"高置信经验 → 技能"这一条流程，A06 的其他信号和评测留到 B6。

## Decision

**提议**（core `memory/skills.rs`）

- `Memory::feedback` 让一条经验从别的档位升到"高"时，调用 `propose_skill`。
- 同一条经验永远只有一份提案：`proposals.experience_id` 是 UNIQUE，被拒绝的也算，所以不会再提。
- 提案内容：
  - 名字：`learned-` 加经验 id 里的 6 位十六进制；
  - 描述：`{标题}。遇到「{问题前 60 字}」时使用。`；
  - 正文：按模板生成 SKILL.md（frontmatter、何时使用、做法、适用环境、来源），不调用模型；
  - 另记下产生经验的会话 `task_id`，"需要我处理"把提案挂在这个会话下。
- 整份提案以 JSON 存在 `memory.db` 的 `proposals` 表里。

**审批与版本**

- 待审时编辑直接改提案，不产生版本。
- 采纳：
  - 名字必须合法（`extensions::skills::validate_skill_name`），也不能和内置技能重名；
  - 宿主补上项目目录 `project_path`；
  - 写出 v1，状态改为 accepted。
- 采纳后再编辑：生成新版本并设为当前版本。正文要过密钥扫描。
- 回到第 N 版：把当前版本改成 N，名字、描述、正文跟着恢复。
- 停用和启用：在 disabled 与 accepted 之间切换。
- 拒绝：只改状态，不写任何文件。
- 每个版本写在 `<data>/builtin-skills/learned/<name>/v<n>/SKILL.md`，写一次就不再改。
- 每次变化后重写 `learned/index.json`，里面是所有 accepted 技能的当前版本目录，以及它是全局的还是属于哪个项目路径。

**挂载**（`extensions/builtin_skills.rs`）

- 新增 `Offer { id, dir, version }`。`link_into`、`claude_plugin`、`sync_project` 的内部实现改成 `link_offers`、`claude_plugin_offers`、`sync_offers`，原有函数保留为包装，签名不变。
- `mount()` 在内置技能后面追加 `learned_offers(data_dir, cwd)`：
  - 作用域是全局的，或者会话目录在项目路径下面的，才会挂上；
  - 用户自己有同名技能、或者名字和内置技能相同的，不挂。
- 三种挂载方式自然都覆盖到：
  - 托管 home：建软链接；
  - 直接启动的 Claude：插件的缓存键里加入版本，版本一变就换一个插件目录；
  - 直接启动的 Codex：项目副本的标记文件写 `learned-v<n>`，版本变了就替换副本。
- 学会的技能放在 `builtin-skills/` 下面，`points_into_store` 照样认得出这是 Aeroric 的链接，所以清理时不会碰别人的东西。
- 宿主所有挂载调用点都不用改：`mount()` 自己读索引。

**宿主与界面**

- 宿主新增方法：`memory.proposals`、`proposalEdit`、`proposalDecide`（从提案所属项目找到目录，作为 project_path 传进去）、`skillVersion`、`skillDisable`。`memory.attention` 的返回值多了 `proposals`，列出待审的提案。
- 记忆页新增"技能提案"标签（`MemoryTab.skills`），没有添加行：
  - 按待审、已采纳、已停用和已拒绝分组，最后一组默认折叠；
  - 每行显示名字、描述（悬停显示正文）、作用域徽标、版本徽标；
  - 待审行有采纳、拒绝、编辑…；
  - 其他行的 ⋯ 菜单里有：编辑…、版本历史（回到某一版）、停用或启用、打开来源会话。
- `SkillEditSheet`：名字和描述用 CapsuleField，作用域用 CapsuleSlider，正文用不带滚动条的等宽 TextEditor。
- 经验行显示"已提议升格"或"已成技能"。
- "需要我处理"新增"技能提案待审"，点开跳到记忆页的技能提案标签。

## Alternatives considered

- **用模型润色技能正文**：要额外花钱，还可能写出没验证过的内容。模板加用户编辑就够了。
- **写进仓库的 `.claude/skills`，或各 Harness 自己的全局目录**：会改用户的仓库或配置。Q-M24 定为放在 Aeroric 数据目录里挂载。
- **给每个挂载调用点传 project_id**：`mount()` 只拿得到 cwd；让索引记下项目路径、由 mount 自己按路径过滤，改动最小，也不会漏掉哪个调用点。
- **原地覆盖同一个目录来出新版本**：已经挂上的会话和项目副本会悄悄变掉，也就没法回滚。所以每个版本单独一个目录。

## Consequences

- 改了名字的技能，旧版本的目录还用旧名字。回到旧版时，链接名也跟着变回去。这是有意的：每个版本的内容和名字保持一致。
- 只有新开或恢复的会话才会拿到新技能，正在运行的会话不受影响，内置技能也是这样。
- 测试：
  - 挂载 1 个：项目内外、全局、三种挂载方式、换目录后链接被清理；
  - core 3 个：高置信才提议、只提议一次；采纳、编辑、版本、回退、停用、内置名被拒；拒绝后不再提议；
  - 宿主端到端 1 个；
  - Swift 1 个：分组、徽标、动作、attention。
- 没有在真实会话里调用过学会的技能，NOT RUN，等 owner 目检。
- 2026-10-31 目检报告（computer-use `2026-10-31-1512-aeroric-b5-skill-proposals`）后修了三处：
  - **配置型 Codex 拿不到"本项目"技能（P1）**：
    - 原因：一个配置的 Codex app-server 由所有项目共用，启动时以 `cwd = None` 挂载到共享的托管目录，于是项目技能被过滤掉了；逐会话的挂载钩子又只处理直接启动的 Codex。
    - 修法：`before_codex_session` 遇到 `ServerKey::Config` 时，走新的 `Mount::CodexProjectLearned`，把这个项目适用的学会技能写成项目 `.agents/skills` 里带标记的副本（`sync_learned`）。清理时只动 marker 以 `learned-` 开头的副本，不碰直接启动的 Codex 留下的内置副本。同时，没有 cwd 的共享 home 不再挂任何学会的技能，免得同一个技能出现两份。
    - 已知限制：配置型 Claude 和 omp 启动时带 cwd，项目技能能挂上；但同一配置的托管目录在并发会话之间共用，两个不同项目的会话同时启动时，理论上可能看到对方项目的技能。
  - **同一会话的待确认记忆遮住了技能提案（P2）**：`AttentionQueue` 原来每个会话只出一行，记忆待确认排在技能提案前面。现在会话自身的那一行之外，记忆和技能提案各占一行，行 id 分别是 `<task>#memory`、`<task>#skill`。
  - **停止后任务一直显示运行中（P2）**：
    - 原因：关闭原生会话时会先退订，收不到回合结束；会话进程已经退出时，`cancel` 又找不到驱动。两种情况下都没有人回写终态。
    - 修法：执行器记下每个原生会话的（项目，代次），取消时回写 `Cancelled`。这一步要在任务服务的锁外异步做：我第一版是同步写的，结果死锁，被新测试抓住了。
    - 宿主端到端测试：回合进行中取消，任务变成 cancelled。
