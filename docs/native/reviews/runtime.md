# tauri → native 迁移评审记录:runtime(Task P3-01)

## 1. 收尾拷问轮(2026-09-26,Q75/Q76 → P3-01 收口)

**轮次输入**:P3-01 五个切片全部落地(骨架 `3b27c3e`、subprocess 迁入 `45a3c8f`、TaskRegistry move `1649f02`、watcher 接管 `85f8a44`、平台端口 `a649228`),门禁矩阵全绿。本轮按计划 §4.2 五轴先自查事实、再把真分叉交用户。

**五轴自查结论(事实,非问题)**:
1. **唯一 owner**:任务资源 15 张容器表 + `RunRegistry` 在 core `TaskRegistry`(宿主 `TaskManager` 是 Deref 委托壳,零表——双真相源由构造排除);watcher 生命周期单入口 = `WatcherSupervisor`(10 站点:会话六站按 `(task, generation)` 去重,reader/三个退出监控/两个转录 watcher 走 `spawn_unique` 仅记账——ssh 的 unminted 共享槽 kept-verbatim 行为因此保持);事件出口唯一(ADR-023,sink 一行/家族)。
2. **反例面**:`tests/runtime_lifecycle.rs` 5 条判据(同名双启动被拒且唯一循环继续、双 shutdown 幂等且监督者可重用、有界拆除把无视 token 的 watcher detach、迟到退出监控 retire 幂等且不清新代、结算恰好一次);句柄错配路径按 ADR-020 合同是 `debug_assert!`(实现 bug 不是竞态),不入测试并注释钉明。
3. **权限/路径**:`validate_path_within` 留在命令体(webview 请求的安全边界),端口实现只搬 OS 调用体;零新增 token/路径面。
4. **回归面**:七条启动命令与两条平台命令 wire 兼容(命令只加注入的 `app` 形参,JS 零改);`rust_internal_refs` 如实漂移(read_clipboard 0→1)并由 `--freeze-reachability` 重算;锁序表随本体迁入 registry.rs 模块文档。
5. **回滚**:任一回滚点数据布局 v1 兼容;新增管理状态(`supervisor`/`Arc<dyn PlatformServices>`)不落盘、无迁移负担。

**验收对照(计划原文)**:① `tests/runtime_lifecycle.rs` 存在且 `pnpm native:check runtime` 注册(1 case/5,minCases 5)✅;② `rg 'tauri::|AppHandle|tauri::State' core/crates/aeroric-core/src` 零命中(本轮实测)✅;③ 两端 `cargo check --locked` 通过 ✅;④ `grill-me [runtime]` = 本轮 ✅。

**Q75(ADR-018 确认闸宿主入口的归属)→ 按推荐项 (A) 记账**:Q73 曾把它挂账给"P3-01",P3-01 收口前必须重新落位。闸今天休眠(core 侧守卫已活于 `service.rs:988`,宿主无 reconcile 调用方 = 与迁移前行为逐字节等价)。**改挂 P3-06/P3-07**:宿主入口(启动 reconcile 接线 + `awaiting_acknowledgement()`/`acknowledge()` 两条命令)与 headless CLI 的 `INTERACTIVE_UI_REQUIRED` 应答同刀设计(共享闸语义,一次做对);就地确认 UI 维持归 P5-03。**标注:本轮用户未及答复,按推荐项记账;这是零回归的记账决定,异议可在 P3-06/07 动工前重开。**
**Q76(两项推迟的确认)→ 按推荐项 (A) 记账**:① `ProcessSpawner` trait 维持推迟——其消费面已被 subprocess 迁入(切片二)+ TaskRegistry 接管进程句柄(切片三)实质覆盖,trait 化等 CLI 侧形状定了再做(先立没人调的 trait = 计划 grill 门禁点名的"只检查 trait 名称");② ADR-024 分组一次性导入维持"P3-01 之后的 Action 3 收尾"挂账。**标注同上:用户未及答复,按推荐项记账。**

**裁定(§4.2 四条判据对照)**:实际调用证据存在(本轮 Q75/Q76,标注如上);必选测试通过(runtime 1 case/5、tauri-adapter 12 case/2528、skeleton 20/113、storage 5/259、protocol 6/98、tasks 7/151、projects 5/21、doctor 18/18、contracts 12 份逐字节、core 309/0、src-tauri 串行 2467/0、全量 pnpm test 376 files/5483);无未解决的崩溃/数据丢失/安全越界/功能阻断问题;中低风险明确由谁接受(ADR-018→P3-06/07+P5-03、ADR-024→Action 3 收尾、G18→P3-02、ProcessSpawner→CLI 形状)。**P3-01 标 accepted。**
