# Agent Note: Aeroric 内置技能（B2 工作配方库）

Status: implemented

## Problem

新建会话首页已有「构建 / 协作 / 探索」配方胶囊，但点了只是往输入框里填一段提示词，Agent 并不知道该照哪套方法做。owner 要把工作配方做成 Aeroric 自带的技能，让四种 Harness 的会话都能用（Trellis 任务 `10-30-builtin-skills-recipes`，三轮访谈共 Q1–Q17，设计稿 2026-10-30 通过；owner 改定不要首页大卡）。

## Decision

**内容**
- 从 codeg 逐字收录 27 个 MIT 技能：开发 14 个（obra/superpowers v6.3.0），科研 13 个（K-Dense，钉在 4d97e293）。放在 `core/skills/builtin/`，附 NOTICE。
- `catalog.json` 里的中文名称和一句话说明由 Aeroric 自己撰写。

**分发**
- `aeroric-core` 的 `build.rs` 把整棵目录编进二进制（约 1.9 MB），App、无头 CLI 和测试共用这一份。
- 宿主按内容哈希释放到 `<data-dir>/builtin-skills/<hash>/`，内容不变就不重写，脚本的可执行位保留。

**挂载**（`extensions::builtin_skills::mount`，挂载失败只记日志，不挡会话）

| 会话 | 做法 |
|---|---|
| Claude Code 直连 | 会话级插件目录 `--plugin-dir`，插件名 `aeroric`，调用写法 `/aeroric:<id>`；不写 `~/.claude` |
| Claude Code / Codex 托管 home（第三方配置、本机登录网关） | 在 home 的 `skills/` 下放链接 |
| OMP（内建 / 配置）、DSH | 在 `PI_CODING_AGENT_DIR/skills`、`DSH_HOME/skills` 下放链接 |
| Codex 直连 | 复制进项目 `.agents/skills/<id>/` 并放标记文件；写入 `.git/info/exclude` 的受管区块；被关闭的副本移到废纸篓（owner 选 Q9-b） |

- 用户自己的同名技能优先，同名的内置技能不挂载（Q6）。判断时只读用户目录。
- OMP / DSH 在项目里已有 Aeroric 副本时，不在 home 里重复链接（Q17）。

**宿主方法**：`builtinSkills.list / setEnabled / projectsWithCopies / removeFromProject`。依赖检测只看 `python3`、`uv` 在不在 PATH。

**App**
- 新增「科研」分组（13 张卡片）。
- 能对上技能的配方胶囊加 ✦，点了先写这个 Harness 的调用写法，再写原模板。技能被关闭时退回只填模板。
- 加号菜单「工作配方…」打开面板。
- Agent 配置新增「技能」分页：开关、状态、来源，以及项目里副本的移除。

## Alternatives considered

- **Codex 直连用 `-c skills.config` 指向包内目录**：实测（`codex debug prompt-input`）不能加载新技能，只能开关已发现的技能。
- **Codex 直连只在点卡片时把技能全文作为消息发出**（Q9-a）：owner 选了写进项目（Q9-b）。
- **随 App 资源分发**：路径随构建位置变化，无头 CLI 也拿不到，所以改为编进二进制。
- **首页三张大卡**：owner 要首页保持简洁，用现有分组滑块。
- **翻译技能正文**：不选。逐字收录才能对着上游同步。

## Consequences

- 宿主二进制增大约 2 MB。数据目录多出 `builtin-skills/`，包括各版本内容、`settings.json` 和 `claude-plugins/`。
- Codex 直连会话会在项目里留下 `.agents/skills/<id>/`，不出现在 git status 里。可以在「技能」分页里移除。
- 远程 SSH 会话第一版不挂载。
- 实测：Claude（`claude plugin details`：两个技能，常驻约 144 tok）、Codex 托管 home 与项目副本（`debug prompt-input`）、OMP（RPC 的 `available_commands_update`）都能看到技能。DSH 按其 skill-filesystem 源码扫描 `$DSH_HOME/skills`，没有单独实测。
- 测试：core 10 个（目录、释放、开关与同名、链接增删、插件、项目副本与 exclude、调用写法）；Swift 6 个，以及已更新的配方数量断言。
