# ADR-032：`STARTUP_SIGNAL_GENERATION` 不删，但降格为"条目版本戳"（修订 ADR-020 决策 2）

- 状态：已接受（Task P2-05 门禁，Q61）。**本 ADR 修订 ADR-020 决策 2 的第二条**，其余不变。
- 日期：2026-09-23
- 相关：ADR-020（运行身份唯一化）、ADR-021（writer 只在 macOS 递交）、ADR-016（generation 权威在 core 独占账本）

## 背景

ADR-020 决策 2 写的是："`STARTUP_SIGNAL_GENERATION` 这个全局计数器**删除**，改用同一个 per-task 数。"
重键第一波真去改的时候发现这句话在当前架构位置下**做不到不等价的替换**，理由有两条实测事实：

1. **它保护的不是运行身份，是"别删了别人的登记"**。`initial_input_signals` 是**按 task_id 单值**的表
   （`pty.rs:1218-1229` 的 `insert(task_id, …)`），所以同任务第二次登记**今天就会**顶掉第一次的 sender；
   全局序号唯一的用途是让后到的 `clear_initial_input_signal_if_current`（`pty.rs:1232-1241`）
   不去删"顶替我的人"那条。
2. **旧命令路径拿不到 per-task 数**。writer 尚未递交，`run_task`/`resume_task` 不经过
   `TaskService`，因此没有 core 的 generation 可用。若用本波的 `RunKey::UNMINTED = 0`，同任务两代都带 0
   → 老代的清除会真的删掉新代的登记 = **相对今天的回归**（表现是启动 prompt 注进错误窗口或干脆不注入）。

另一条可行路径是"让旧命令也经 core 取号"，但它不是免费的：`OperationLedger::next_generation` 的产出要
落进 core 独占的 `task-operations.json`，也就是一次 core 写。按 ADR-021，非 macOS 平台还没交出 writer，
那等于让旧客户端在 Windows/Linux 上开始写 core 账本——与刚定下的平台口径正面冲突。

## 决策

1. **保留** `STARTUP_SIGNAL_GENERATION`，但把它的作用域**钉死在 `initial_input_signals` 这一张表内部**：
   它是"我登记的那一条还在不在"的版本戳，**不是运行身份**。
2. 任何其它容器**不得**读取它当身份；运行身份的唯一来源仍是 core 的 per-task generation
   （加 `RunKey::UNMINTED` 这个保留值）。这条要能被机械断言：全仓除 signal 登记/清除两处外，
   不得再出现对该序号的引用。
3. `initial_input_signals` 在 wave 2 里**改成按 `RunKey` 键**，但**条目内部仍带版本戳**——
   两代 unminted 共享同一键时，版本戳继续提供今天那层保护；等旧路径经 core 起进程之后，
   版本戳自然变成冗余（届时可单独删除，不需要现在赌）。

## 理由与被否方案

- **"让旧命令路径也经 core 取号"**：身份最干净（0 彻底只属于 shell），但要么在非 macOS 平台开 core 写
  （违反 ADR-021 的移交范围），要么把 Action 3 的 store 改造提前到本波之前——那是排期决策，不是这题的选项。
- **"接受 0 的歧义，纯按 RunKey"**：语义最统一，但是行为回归，且回归形式是静默的（用户只看到 prompt 没进去）。
- **改写而非删除**：ADR-020 的**意图**是"运行身份只有一个来源"。这个意图完整保留；被修订的只是
  "顺手把这个计数器删掉"这条实现手段。

## 后果

- ADR-020 决策 2 的文本要就地标注本 ADR（不静默改写，未来读者看到的第一句是"删除"，那就得让他立刻看到
  它被修订了）。
- "全局计数器"这个坏味道仍在，但它现在有一个明确的边界与一条门禁断言；比它更糟的是**用两个身份系统**
  或**为删它而引入跨平台写入**。
- wave 2 的 `initial_input_signals` 改造范围因此变小：只换键，不动判等逻辑。
- `wsl_active_ids` 顺带定了性：它同时装 shell id 与 task id（`wsl.rs:1417/1446` 是 shell，
  `wsl.rs:956/1713` 是任务），职责是"这台机器上还有没有 WSL 侧的活运行"（`wsl.rs:1342` 判空）。
  这是**存活**语义，因此按 RunKey 集合走，`is_empty()` 的含义不变，不需要新决策。
