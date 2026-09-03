# Aeroric Knowledge Base

面向 agent 与新成员的结构化知识库。**只放 WHY、契约、踩坑结论**——WHAT 由代码自身负责。索引文件只列链接 + 描述 + 标签，正文在子文档。

写作规范见 [`.claude/skills/repo-kb/SKILL.md`](../.claude/skills/repo-kb/SKILL.md) 与 [`reference/core-beliefs.md`](../.claude/skills/repo-kb/reference/core-beliefs.md)。

---

### xterm 终端渲染

| 文档 | 描述 | 标签 |
|------|------|------|
| [终端渲染与选区卡顿排查](./xterm/rendering-and-selection-lag.md) | WKWebView 下 `.xterm` 合成层长帧的真因与定论，含 CSS containment 禁用、WebGL 保留的实测权衡，面向后续动渲染链路前的必读校准 | `xterm`, `wkwebview`, `composite`, `webgl`, `selection`, `regression-guard` |
| [Agent 终端的滚轮语义与鼠标上报](./references/terminal-wheel-scroll.md) | 为什么 alt screen 里滚轮会顶掉输入框草稿、鼠标上报的两个禁用来源（含 launchd 污染这个坑）、滚轮兜底为何只装 agent 终端，以及"上报开/关"各自的代价 | `xterm`, `wheel`, `alt-screen`, `mouse-reporting`, `claude-code`, `env-inheritance` |


### 性能与资源

| 文档 | 描述 | 标签 |
|------|------|------|
| [长跑内存增长的采样口径与静态结论](./references/long-run-memory-profiling.md) | 存活计数 + RSS 双路采样怎么对照判读、已修的四处结构性泄漏(SSH 会话、WebGL 配额、prose 缓存字节上界、面板常驻),以及为何 scrollback / 轮询间隔等参数在拿到实测曲线前不动 | `performance`, `memory`, `leak`, `profiling`, `webgl`, `ssh`, `cache` |

### 安全与凭据

| 文档 | 描述 | 标签 |
|------|------|------|
| [凭据存储威胁模型](./references/credential-storage.md) | SSH/DB 密码落盘位置、权限、运行时暴露与非目标，面向安全审查与后续钥匙串改造 | `security`, `credentials`, `ssh`, `database`, `threat-model` |

### 外部参考

| 文档 | 描述 | 标签 |
|------|------|------|
| [Claude Code 与 Codex 的 Hook 支持](./references/agent-hooks-support.md) | 两个 agent 当前版本的 hook 事件/payload 字段/配置方式/信任机制全量对照，以及 Aeroric 订阅哪些事件、为何这样映射，面向 hook 链路开发与排查 | `hooks`, `claude-code`, `codex`, `event-watcher`, `session-discovery`, `input-required` |
| [Aeroric 多项 Bug 提示词处理流程](./references/aeroric-bug-report-workflow.md) | 将“逐个修改、先复现、参考截图、检查整个项目、测试审查、打包替换”等中文提示词映射为固定工程流程，面向下次多 bug 修复任务前的必读校准 | `bug-report`, `workflow`, `review`, `packaging`, `aeroric` |
