# tauri → native 迁移评审记录:agents(Task P3-02)

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
