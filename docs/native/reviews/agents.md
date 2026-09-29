# tauri → native 迁移评审记录:agents(Task P3-02)

> **数字口径（2026-09-29 加，审计出处 `MIGRATION_PLAN.md` 补记一百七十九）**：本文件里的 case 数／用例数／`minCases` 一律是**验收当天的快照**，只作证据留档，不是今天的地板值——模块在此之后被别的 Task 扩过。要看今天的数字：现场跑 `node scripts/native/check.mjs <module>`，或读 `scripts/native/checks.json` 各 case 的 `minCases`（`inventory --check` 会核对地板与冻结清单一致）。**既不要把这里的旧数字当成回归证据，也不要为了两边对齐而去改任何一份带日期的记录。**

## 审查 commit 锚点（2026-09-29 反查补齐；§4.2 要求的这个字段当时没有记）

`MIGRATION_PLAN.md` §4.2 明文要求每份评审保存七项——**tool 来源/revision、实际调用入口、审查 commit/diff hash、问答、证据、问题级别、修复后复验结果**。本轮插入本节之前全量普查 `docs/native/reviews/`：**40 位十六进制串只有 2 处（插入之后本节自己会再引入锚点，那是预期结果），且两处都是外部技能 pin**（`c55ee46073ed923f86ce59a5eb3b6d895095d1b7`＝`mattpocock/skills` 的 grill-me，见 `docs/native/reference-provenance.md`）⇒ **九份载体没有一份记过被审查仓库的 commit**。这一节用 `git log -1 --until=<日期>T23:59:59` 反查补齐，**是事后重建，不是当时的记录**。

- Task **P3-02** accepted_on＝2026-09-27 ⇒ 当天最后一个提交＝`2befb12bf4dcdacdd09f42ca0621ec254ef9b987`（2befb12，2026-09-27，「test(terminal): P3-03 收尾轮自主部分——活流码元游标契约、100MiB CPU/RSS 证」）

**局限（必须一起读）**：反查锚点只覆盖**当时已提交**的树；acceptance 常在脏树上完成，未提交部分**无法事后重建**，所以 §4.2 要的「diff hash」这一格今天仍然是空的。缺口已登记为 `G31`：今后的每一次 `grill-me` 必须**当场**写 `git rev-parse HEAD` ＋ `git status --porcelain` 的文件数与路径摘要（`docs/native/reviews/*` 的内容不被任何 case 解析，机器无法事后追补）。本文撰写时 HEAD＝`797e06c3c35ce9b28b53be3f0149894e6e038a8a`（797e06c 2026-09-28 11:04:54 +0800）。

## 1. 收尾拷问轮(2026-09-27,Q77/Q78/Q79 → P3-02 accepted)

**轮次输入**:P3-02 四个切片全部落地(映射表+LaunchPlan `9772e1b`、四族计划函数+parity fixtures `cdcb86a`、env 顺序操作表+OMP 不变量 `ab6a22a`、hook 资源与纯构造迁 managed_home `3b349cf`),门禁矩阵全绿。§4-5 侦察记档于 `35f91bf`。

**五轴自查结论(事实,非问题)**:
1. **唯一 owner**:命令规划(LaunchPlan/四族 plan 函数/权限映射表)、hook 脚本资源、hook 配置纯构造全部在 core `agents/`;宿主只剩机械翻译(`*_command` → CommandBuilder)与 I/O 包装(路径解析/原子写/安装状态)。
2. **反例面**:parity fixtures 18 case 双侧逐字节(core 重放 + 宿主等价守卫);OMP 三条不变量(`.mjs` 静默丢弃反例、willContinue 短路缺失即红、`--mode` 永不产出的全组合性质测试);权限档四表快照测试(本地/远端一致性契约)。
3. **权限/路径**:零新增 token/路径面;hook 安装 I/O 未动;路径校验留宿主。
4. **回归面**:argv/env/cwd 经 fixtures 逐字节相等;宿主旧测试经 re-export 照跑(2465/0);JS 调用点零改。
5. **回滚**:hook 脚本 git mv 内容 100% 相同;revert 即回迁;无落盘格式变化。

**验收对照(计划原文)**:`pnpm native:check agents`(1 case/2,parity fixtures 比较迁移前后 argv/env/cwd)✅;OMP 7 档 thinking + ultra 封顶 max 不向上游发 ultra ✅;hook 扩展名/willContinue 反例 ✅(切片三);移交项 SwitchEvidence/目标选择 ✅(见 Q77/78 处置);`grill-me [agents]` = 本轮 ✅。

**Q77(switch_config 入口形状)→ 按推荐项 (A) 记账**:`TaskService::switch_config`(executor-reset 复合)已无生产调用方;生产切换 = `SwitchConfig` 意图(#12)+ 宿主 run-flow kill/relaunch,行为有测试锁定。**处置**:switch_config+SwitchEvidence 标记为 CLI/后续复合入口的预留形状并挂账(不删——headless CLI 需要复合入口的可能形状),避免"两个生产入口"并存。**标注:用户未答,按推荐项记账,异议可重开。**
**Q78(adopt_session_for_agent 归属)→ 按推荐项 (A) 记账**:转录复制 I/O 留宿主/客户端;core 持判定(`resolve_session_owner`/`resolve_config_switch_strategy`)。与 ADR-035"证据来自客户端、权威在 core"一致;真要迁属 P3-04 sessions 领地(会话根目录抽象先建)。**标注同上。**
**Q79(G18 claude_gpt55 族归属)→ 按推荐项 (A) 结案**:**维持现状**——记录侧三字段(`launchFamily`/`sessionFamily`/`resumeFamily`)各表其是,ADR-028/029 已把三站点事实落成可评审字段,行为=迁移前;统一任一族都是真实用户行为变更(改启动命令 / 丢 `~/.claude/projects` 历史),不属于迁移。**G18 就此结案**(结案方式 = ADR-028/029 的记录侧方案已覆盖;若未来用户要求行为统一,另立 ADR)。**标注:用户未答,按推荐项记账。**

**裁定(§4.2 四条判据对照)**:实际调用证据存在(本轮 Q77/Q78/Q79 + 侦察记档);必选测试通过(agents 1 case/2、tauri-adapter 12 case/2526、skeleton 20/113、runtime 1/5、doctor 18/18、contracts 12 份逐字节、core 321/0、src-tauri 串行 2465/0、全量 pnpm test 376 files/5483);无未解决的崩溃/数据丢失/安全越界/功能阻断问题;中低风险明确由谁接受(上列三项处置 + switch_config 挂账各有归属)。**P3-02 标 accepted。**

## §4.2 七字段落点（2026-09-29 反查补齐，出处 `MIGRATION_PLAN.md` 补记一百九十二；本节只回答「每个字段落在本文哪一处、哪一格当时是空的」，不重述内容）

- **tool 来源/revision**：本文没有独立 provenance 小节（它是收尾拷问轮的短记录）；pin `c55ee46073ed923f86ce59a5eb3b6d895095d1b7` 只出现在上面的锚点节里，权威出处是 `docs/native/reference-provenance.md` 与计划 §4.2 ⇒ **当时未记**。
- **实际调用入口**：§1 的问题本体即入口内容（三个问题逐条可追），发送动作本身没有单独记档。
- **审查 commit/diff hash**：见上面「审查 commit 锚点」一节——commit 由 `git log -1 --until` **事后反查**，diff 句柄**当时没记、今天补不回来**（缺口＝`G31`）。
- **问答**：§1「收尾拷问轮(2026-09-27,Q77/Q78/Q79 → P3-02 accepted)」——3 问 3 答，问题与用户答复一一对应。
- **证据**：没有「证据」标题：证据就是 §1 三个问题的结论本身 ＋ 当天 `native:check agents` 的输出（旧数字按顶部横幅只当验收快照读）。
- **问题级别**：**全文无级别标签**（事后判读）：本轮零缺陷级发现，Q77／Q78／Q79 全是产品决策，不属「发现的问题」。**接受人**：无中低风险项需要接受，决策已由用户当场答复。
- **修复后复验**：`native:check agents` 在 2026-09-29 同日全门禁复跑中 **1／2 exit 0**（数字见 `docs/native/baseline.md` 同日节）；**这是验收之后的复验，不是当时的记录**。
