# ADR-027：拿不到 lease 的第二个实例以只读模式启动，并持续可见地告知

- 状态：已接受（Task P2-05 门禁，Q54）
- 日期：2026-09-23
- 相关：ADR-010（writer 资格）、ADR-011（拒绝映射到冻结错误码）、ADR-021（writer 只在 macOS 递交）、ADR-026（冲突分两类处置）、INV-03

## 背景

`CoreContext::acquire` 会取数据目录租约，冲突时返回 `LeaseError::AlreadyInUse { directory, holder }`（`core/crates/aeroric-core/src/storage_lock.rs:57,175`）。今天这条错误在 `src-tauri` 里**没有任何处理点**——旧客户端根本不取 lease，两个窗口可以同时写、谁最后写谁赢。writer 移交后它变成一条真实可达的新路径：第二个实例必须决定自己是谁。

已知的产品约束（用户既有决定）：**`AlreadyInUse` 绝不自动 kill 外部 PID**。

## 决策

1. **第二个实例照常开窗口，但进入只读模式**：能列项目、能看任务与历史；所有写意图在 adapter 入口就被拒，不起进程、不落盘。
2. **不抢租约**：lease 只能由持有者自己释放。"后启动的接管、前一个转只读"被明确否掉——那会让先开的窗口在无人知晓的情况下停止落盘，界面还一切正常。
3. **不退到临时目录**：`resolve_data_dir` 今天的"拿不到就退到 temp"退路**不能**用于这种情况。两份数据从此分叉、而用户以为在用同一个资料库，这是所有选项里最接近静默丢数据的一个。
4. **告知必须持续可见，不是一条会消失的提示**。落地时复用既有机制而不是新造：`startup_diagnostics::StartupDegradation { component, reason, fallback }` + 前端 `useStartupDegradationToasts`（`src/state/app/useAppMaintenance.ts:22-44`）按 `component` 选文案。**已知不匹配**：那条机制是 toast，会自动消失，而只读是整场会话持续的状态——因此本决策附带一条必须补的东西：只读态需要一个不随 toast 消失的常驻指示（沿用既有横幅/状态位的形式，不新造控件）。把这点写下来，是为了不让"复用了 StartupDegradation"被当成"已满足持续可见"。

## 后果

- "只读"由 adapter 一处判定（`Option<CoreContext>`：拿不到就是只读），不是每条命令各自判断——后者一定会漏。
- 只读模式下 `list_startup_degradations` 必须报告这条，所以 `component` 命名要进 `tauri-adapter` 门禁的期望值，避免文案与代码分头演化。
- 拒绝写时的错误语义与 ADR-026 共用同一条通道：只读的拒绝不是 `REVISION_CONFLICT`，而是"本实例没有写权"，两者在 UI 上的处置不同（前者可能要重放，后者重放也无意义）。
