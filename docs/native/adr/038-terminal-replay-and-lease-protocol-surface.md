# ADR-038：终端重放面与租约面成为 v1 协议入口——cursor 补流一个方法、UnknownStream 拆成两个具名理由

- 状态：已接受（Task P5-01 具名前置；来源＝P3-03 补记一百零二 审计移交 + Q2 用户拍板 A「现在只补文档＋具名前置」的兑付）
- 日期：2026-10-02
- 相关：ADR-023（core 是事件唯一发射者）、ADR-028/029（家族装不进宿主）、§2.3（本地协议 v1 字段与传输约束）、§3.1 #8（core 服务必须同步 command-map/协议/CLI 可达性）、P3-03 补记一百零二（重放面/租约面在本地协议 v1 里没有方法入口的审计发现）、Q2（用户 2026-09-27 拍板 A）

## 背景

core 的终端流设计里，**回放与活流是同一个机制**：`TerminalService` 持有每条流的字节环，订阅者交回一个 `ResumePoint`（字节游标为唯一无损形态），`plan_replay` 返回 `Seamless{offset,bytes}` 或 `Reset{from,bytes,reason}`，`resume_after()` 给出下一轮该记的游标——初始回放与活流尾巴走同一条路，客户端唯一要记的数是「已交付字节之后的 offset」。

但这条面在 2026-09-29（P3-03 收口审计，补记一百零二）是 **Rust 侧孤岛**：v1 在册终端方法只有 `send_input`/`resize_pty`/`read_task_terminal_history`/`delete_task_terminal_histories` 等 eleven 条，`plan_replay`/`ResumePoint`/`ControlLease`/`acquire_control`/`release_control` 六个符号在 host/schema/Swift/macos/cli 全部零读者。P5-01 Action 4「同 task 返回时使用 cursor 补流」与 Action 5「按 host replay 从安全边界重建」因此被具名阻塞。

Q2（用户 2026-09-27 拍板 A）同时留了一条**不得 silently 继承**的不对称：core 的 `StreamError::UnknownStream` / `ResetReason::UnknownStream` **一个理由表示两件事**——「这一代还没产出任何字节」与「流已收尾并被 FIFO 退场」。前者是活流的正常起点（客户端挂上等尾巴即可），后者是终态（没有任何历史可重放，历史走 `read_task_terminal_history`）。不拆的后果：原生端「重连后一片空白」无法回答「是等还是判死」。

## 决策

1. **一个方法承接回放与活流**：新增 v1 方法 `terminal.plan_replay`（读，`max_bytes` 由 `MAX_TERMINAL_CHUNK_BYTES` 钳制）。请求 `{taskId, generation, cursor: {kind: "bytes"|"unitsFromStart", value}, maxBytes}`；应答 `{kind: "seamless"|"reset", from, bytes(base64), reason, resumeAfter}`，`reason` 与 core 的 `ResetReason` 一一映射；`resumeAfter` 恒为字节游标（core 的 `resume_after()` 语义照搬——重置视图的订阅者下一轮也走字节车道）。**活流尾巴不复刻 Tauri 的事件推送**：同一个方法按客户端自持的节奏轮询即是活流，字节环就是缓冲。被否掉的替代：给终端输出开一条事件流——那会在 ADR-023 的「core 是事件唯一发射者」清单里塞进一个高频通道，且与字节环形成双真相（事件通道与字节环各自记账，谁权威？）。
2. **`UnknownStream` 在协议层拆成两个具名理由**：协议 `reason` 枚举在 core 四值（`compacted`/`unknownStream`/`cursorBeyondStream`/`unitsNotAnchorable`）之外新增 **`notStarted`**（这一代已登记但尚未产出任何字节——客户端应挂起等活流，不得判死）与 **`evicted`**（流已收尾并被 FIFO 退场——无可重放，历史走 `read_task_terminal_history`）。**分类发生在宿主服务层**：host 手里有任务注册表，`任务+generation 在册且流未登记 ⇒ notStarted`；`任务已收尾且流未登记 ⇒ evicted`；`任务本身不可知 ⇒ unknownStream`。core 的 `ResetReason` **不改**（blast radius 最小：core 如实回答「这条流不在我这里」，「在册/收尾」的判定材料在宿主手里，塞进 core 反而要给 core 开任务注册表的读口）。
3. **租约面上协议**：新增 `terminal.acquire_control`（mutating——顶替任何旧持有者）与 `terminal.release_control`（mutating——过期票据按 P3-07 CLI 语义**静默 no-op 成功**，不报错）。请求都带 `{taskId, generation}`；acquire 应答 `{ticket, supersededTicket?}`，release 应答 `{released: bool}`。不做 `check_write` 的协议入口：写检查内嵌在 `send_input`/`resize_pty` 的既有路径里，单独暴露只会造出第二个调用顺序约定。
4. **capability**：`terminal.plan_replay` 记 `requiresCapability: "terminal.replay"`——嵌入宿主（P2-05 的 embedded adapter）可以按实现进度选择不声明，客户端得到的是服务端 `UNSUPPORTED_CAPABILITY` 拒绝而非猜测；`acquire/release` 不设 capability（有终端就有租约，多一道门只是多一次不匹配）。这是 capability 模型注释里写明的用法：`v == 1` 从不蕴含能力。
5. **CLI 可达性按 §3.1 #8 走 `rpc` 通道**：不为这三个方法开专用子命令（它们是 renderer 的机器面，不是人手用的面）；`aeroric-cli rpc terminal.plan_replay ...` 即可达，受 P3-07 的 schema/capability 约束。
6. **unitsFromStart 诚实降级**：协议接受旧客户端的码元游标（`kind: "unitsFromStart"`），core 的既有规则（只有「游标 0 且保留线 0」可换算，其余一律重置）原样生效——协议不为猜测字节位置开后门。

## 后果

- **command-map.json 必须同刀登记三条**（`terminal.plan_replay`/`terminal.acquire_control`/`terminal.release_control`），export-schema 从冻结清单派生 method 表——三侧（schema/宿主注册/CLI rpc）同红同绿是这条链该有的形状，漏一侧就是 §3.1 #8 的再现。
- **Swift 客户端与 renderer 的消费顺序**：P5-01 的 coordinator 只依赖 `plan_replay` 的应答形状；`notStarted` 分支的 UI 表现（挂起等活流）是 renderer 的职责，不是协议的。
- **轮询节奏是客户端私事**：协议不定义轮询间隔；字节环的 `capacity` 决定「落后多远会被重置」，renderer 的保留线策略归 P5-01 Action 5。
- **`evicted` 与 `read_task_terminal_history` 的分工写死**：前者是「这一代无可重放」，后者是「跨代的历史转录」——renderer 收到 `evicted` 不得把历史转录塞进屏幕缓冲冒充重放。
- **`terminal.replay` capability 的宿主开关**：embedded adapter（P2-05）实现进度落后时，不声明该 capability 即可整体拒绝重放面；原生端据此显示降级提示而不是无限等待。
