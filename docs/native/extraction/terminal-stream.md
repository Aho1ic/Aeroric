# P3-03 侦察底稿:terminal 字节流、租约、回放与退出(2026-09-27)

> 性质:实施前的机械盘点,数字来自 P3-02 收口后的真实源码。每节都是后续切片的输入。

## 1. 现状管道(宿主 pty.rs)

`spawn_pty_reader` → 32 KiB buf 原始读 → **立即 `std::str::from_utf8` 转 String**(非法 UTF-8 截到 valid_up_to,丢弃尾部)→ `run_emit_batcher`(String 攒批,交互档 4ms 窗口 / 32 通道容量)→ `send_pty_chunk`(tauri emit / Channel)。**raw bytes 今天不保真**:终端历史(`append_task_terminal_history`)与 remote `terminal_hub`(tokio broadcast,`publish(task_id, data:String)`)拿到的都是已解码字符串。P3-03 的核心转变:**core 先发布原始 bytes(offset=原始字节计数),旧 Tauri sink 才增量解码**。

## 2. 现有语义清单(迁移时逐条对照)

| 面 | 现状 | P3-03 目标 |
|---|---|---|
| 流身份 | 无(仅 task_id 字符串) | `TerminalIdentity = (task_id, generation)`(Action 2) |
| offset | 无 | 原始字节计数;回放按 offset |
| 历史 | `storage.rs` append + tail 读 + 4 MiB compaction slack | 与有界回放共享一个 owner(Action 4) |
| resize | `pending_pty_sizes` 信箱(unminted 键);hidden view 可发 0×0? | 单控制租约;hidden view 无写权限(Action 3) |
| input | `write_task_input`(writer_for 最新代) | 控制租约持有者才可写 |
| 慢消费者 | `terminal_hub` broadcast(慢者丢数据?) | 显式 gap,不阻塞 PTY reader(Action 4) |
| 退出结算 | `finalize_task_exit`(SIGTERM/非零/EOF/迟到 hook) | 每种来源只结算一次(P3-03 验收) |

## 3. 首片范围(纯 core,可验证)

- `TerminalIdentity`;`ByteStream`(append 原始字节 → offset;`read_from(offset, max)` → Chunk/Gap;有界 compaction——低于保留线的请求给 Gap,不撒谎"能恢复屏幕");bytes 不透明(NUL/非法 UTF-8 原样)。
- `ControlLease`:单控制者(input+resize);`acquire` 显式;被顶替的租约写操作返回 `Superseded`;只读订阅者不需要租约。
- 单元属性测试:跨帧 append、gap 语义、有界内存、租约顶替。

## 4. 后续切片

2. 宿主管道接流:reader 把 raw bytes 发布进 core ByteStream,旧 String 管道改为增量解码消费(Tauri sink 兼容);`tests/pty_bytes.rs`。
3. 退出结算单一化(SIGTERM/非零/EOF/正常/迟到 hook 各一次)+ `tests/pty_exit.rs`;fixtures/fake-agent.mjs。
4. resize 信箱与 hidden-view 规则;`tests/terminal_replay.rs`/`terminal_lease.rs` 补齐;`terminal/executor.rs` CorePtyExecutor 只换本地 PTY 分支;注册 `pnpm native:check terminal-core`;`grill-me [terminal-core]`。

## 5. 门禁对照

- `pnpm native:check terminal-core`:**已注册**（补记九十四提前一片注册,本切片产出 `tests/pty_bytes.rs`,§3.2 不允许测试存在而没人跑）。切片三之后 = **5 case / 51 用例**:`terminal-unit-rust` 35 + `terminal-pty-bytes-rust` 6 + `terminal-pty-exit-rust` 10,另两条是宿主源码守卫。
- core 基线:321 → 切片一 334 → 切片二 352 → 切片三 **372**(实测 18+ 个测试二进制求和,非推算)。`-p aeroric-core` 那条口径同步走:284 → 302 → **322**。

## 6. 切片一实施结果(2026-09-27,补记九十三)

上一版本文档随附的 `terminal/{mod,stream,lease}.rs` **从未进模块树**——`lib.rs` 里没有 `pub mod terminal;`,所以 §5 那句"core 基线 321,本片测试在模块内"是一条从没被任何命令验证过的声称(§3.1 第 6 条禁止的正是这个)。接线之后实测:

- 3 处编译错:`b"你好"` 非 ASCII 字节串字面量、`StreamError` 缺 `PartialEq`、`ControlLease::release(&LeaseHandle)` 内经共享引用写 `handle.live`。
- 2 处逻辑缺陷:(a) Gap 分支 `take(max_bytes)` 未对 `retained.len()` 取小,追平点第一帧可超出调用方上限;(b) 旧设计 `release` 把 `current` 归零 + `acquire` 递增复用 → **释放后重新取得会让调用方手里的旧 handle 复活**。数据结构改为 `next_ticket` 单调递增 + `current: Option<u64>`(票据永不复用),`live` 布尔删除。
- 1 处断言写反:`compaction_...` 第二个断言拿 offset 10(已落在保留区 `[8,16)` 内)期望 `CDEF`,正确的无缝产物是 `ABCDEF`。按语义改断言并补 offset 12 的情形,**未改实现迁就断言**。
- 新增用例:跨帧劈开的 4 字节 emoji + ANSI 逐字节保真、Gap 读取尊重 `max_bytes`、`release→reacquire` 不复活旧 handle、32 轮票据不重复、重复释放报错。
- 计数:terminal 模块 **13** 条(6 lease + 7 stream);core `--lib` **179**;`cargo test -p aeroric-core` 口径 **284**;core workspace 全量 **334**(= 基线 321 + 13)。
- `storage-core-rust` 地板 81 未动:它是补记六十五点名的假地板,且其口径(`-p aeroric-core`)与 P3 补记一直记录的 core 全量(workspace)**不是同一条**。合并成一条可数地板属门禁工具改动,留给注册 `terminal-core` 那一轮一起做(→ 已在 §7 那轮注册,但两条口径仍未合并,见 §8 第 3 条)。

## 7. 切片二实施结果(2026-09-27,补记九十四)

落点:`core/src/terminal/pty.rs`(`TerminalService` + `Utf8Projection`)、宿主 `lib.rs::TaskManager.terminal` 字段、`pty.rs::spawn_pty_reader` 换形、`tests/pty_bytes.rs` + `tests/fixtures/fake-agent.mjs`、`checks.json` 注册 `terminal-core`(3 case/31)。

- **次序**:reader 现在对每一块读到的字节先 `terminal.publish(identity, chunk)`,再 `projection.push(chunk)` 出字符串。下游(startup_tx → output_filter → 历史/terminal_hub/session_tx → emit)顺序一格未动;`output_filter` 吃掉数据时那些字节仍完整留在 core 流里——"raw 是真相、字符串是投影"就是这条分工。
- **有界规则是三条**:每流 1 MiB 窗口;**已收尾**的流按插入序 FIFO 退场(≤64 条);**在写的流绝不静默退场**——清空一条还在写的流会让新建的那条从 offset 0 起计数,等于谎报"历史完整",所以配额用尽改为拒绝并计数(`RefusedFull`),宿主只记诊断。
- **修掉的旧缺陷(有对照用例,不是推测)**:① 非法 UTF-8 被无限期扣在 `leftover` → 终端卡死 + 缓冲无上限增长;② 进程在半个字符中间退出 → 那半截静默丢失;③ `read_from` 的 `start + max_bytes` 在 `usize::MAX` 下 debug 溢出 panic。用例把**迁移前那段算法原样重放**当对照(`legacy_decode`),断言它卡流且留着尾巴。
- **真 PTY 实测**:4 MiB / 4139 帧 / 112 ms;**100 MiB / 102418 帧 / 6824 ms**,保留量恒等 1 MiB,未截断区间逐字节 + sha256 双比对;2 MiB vs 8 MiB 保留量相同。fidelity 模式的替身块长取图案周期(24 B)的整数倍 ⇒ 拼起来永远合法 UTF-8,而内核 tty 队列(~1 KiB/帧)不按周期对齐 ⇒ 跨帧劈开是必然而非运气,并有一条反空转断言守着。
- **计数**:core workspace 352/0(18 二进制;321 基线 + 13 切片一 + 18 本片)、`-p aeroric-core` 口径 302、`terminal-core` 3/31、`tauri-adapter` 12/2526(src-tauri 2465 = 地板)、全量 `pnpm test` 376/5483。

## 8. 切片三/四必须带上的三条口径

1. **两套 offset 不是同一个东西**:`src/terminalRingBuffer.ts` 的绝对偏移是 **JS 字符串长度 = UTF-16 计数**,core 的 offset 是**原始字节计数**;非 ASCII 输出下两者不等。跨端"按 cursor offset 续看"(P5-01)必须先有显式映射,否则重连必然重复或漏读。`terminal/replay.rs` 落地时定义,别拖到 P5-01 才发现。 **切片三未动它**(本片不含 replay 代码),已升格为切片四的硬性出口条件并写进 §续跑指令。
2. **`cargo fmt --all` 的正确用法**:门禁 argv 是 `cargo fmt --check --all --manifest-path core/Cargo.toml`(core 工作区)与不带 `--all` 的 src-tauri 版。手工加 `--all` 到别的路径会把 `../dbx/vendor/wry` 的 examples 清单拖进遍历(15 条 `Error: file ... does not exist`);本轮核实零写入 dbx、其脏文件数仍是基线 38,但**这不是可以重复利用的余地**——用门禁 argv,或先确认路径不越界。
3. **两条 core 计数口径仍是两条**:`storage-core-rust` 的地板 81 测的是 `cargo test -p aeroric-core`(今天 302),而 P3 各补记记的是 workspace 全量(今天 352)。`terminal-core` 注册时没有顺手抬那条假地板——合并它属于门禁工具改动,谁做都要连同 `minCases == runner.floors` 的同改规矩一起,别在终端切片里顺手改。

## 9. 切片三实施结果（2026-09-27，补记九十五）

落点：`core/src/terminal/exit.rs`（唯一决策面，10 条单元）、宿主 `pty.rs::finalize_task_exit` 换形、`tests/pty_exit.rs`（10 条真 PTY 真进程）、`tests/fixtures/fake-agent.mjs` 扩 `exit`/`signal`/`eof`/`child`/`orphan`/`interactive`、门禁 `terminal-core` +2 case。

- **决策面**：`ExitFacts` → `ExitDecision{NoStatus|Done|Failed{reason}}`；`SessionEvidence{Absent|PlaceholderOnly|Real}` 由宿主如实上报，家族差异只在 `session_counts_as_success_for_family` 一处。reason 用枚举不用字符串，宿主文案经 `host_sentence()` 逐字保住——core 未入账路径 `tasks::settled_by_exit` 那句措辞与它不同，合并属文案改动，交 grill 轮。
- **命名冲突**：新类型最初也叫 `ExitSettlement`，与 `TaskRegistry::settle_marks_for_exit`（registry.rs:329）同名，而宿主同一函数 6 行之内两处都要用。改的是**未提交的新类型** → `ExitDecision`。
- **宿主没变的地方**：三段守卫次序、取消/手动完成的早退与清理、ADR-031 的会话路径反查释放、`cleanup_task_events` 在取消路径上仍被跳过（既有不对称）。
- **两条与计划预期相反的事实**：① portable-pty 把信号致死折成 `exit_code=1`（`From<std::process::ExitStatus>` 里 `code()` 为 None ⇒ `unwrap_or(1)`），信号名只剩在 `Display`（macOS `strsignal` 给的是 `Terminated: 15`）——决策面看不见"哪个信号"，SIGTERM 与非零退出在表里同一个形状；② "EOF 早于进程退出"的窗口造不出来（替身关掉 tty 的 0/1/2 之后，reader 仍在进程退出那一刻才拿到 EOF），改用 `orphan` 形状（组长先退、后代以 `stdio:inherit` 继续持有 tty）证同一件事：结算**先于** EOF 发生，EOF 到了也不会再来一次。
- **门禁与反例**：`terminal-unit-rust` 地板 25→35；新增 `terminal-pty-exit-rust`（10，串行）与 `host-exit-settlement-goes-through-core`（禁 `had_agent_session` / `if exit_ok ||` / `status == "failed"`，5 条正控）。两次变异各有命中（守卫 1 关掉 ⇒ 集成红；家族表统一 ⇒ 集成＋单元各红一条）。其中一条正控当场抓到我自己写错的正则（写成 `remove_pty_handles_if_current(run, &child_handle)`，宿主实际是 `(run, child_handle)`）——正控拒绝把"读不到"当成"没有"，这条纪律又一次真的生效。
- **这条守卫说不了的话**："`settle_exit` 恰好被调用一次"是**计数**，absent 型 case 断不了；已写进该 case 的 `$note` 并归 P3-03 收尾（gate 脚本，照 `scripts/native/adapter-invariants.mjs` 那种）。
- **宿主测试写真实 `~/.aeroric`**：`tauri-adapter` 窗口内 `remote-audit.log` + 3 个 `agent-credentials/*` 被写（mtime 10:44:18–10:45:00，当时 Aeroric.app 未在跑）。本切片零触碰 `app_settings/*`、`remote/audit.rs`，但**未做基线对照**，按待查项记，不写成"确认无关"。

## 10. 切片四A 实施结果（2026-09-27，补记九十六）：重放映射了结 §8 第 1 条

落点：`core/src/terminal/replay.rs`（8 条单元）+ `tests/terminal_replay.rs`（4 条，含真 PTY）+ 门禁 +1 case。本片零 src-tauri 改动。

- **三条规则**：① 字节 offset 是唯一的续看凭证（`plan_replay` 只对 `ResumePoint::Bytes` 给无缝）；② 码元游标**不做换算**，只如实 `Reset{UnitsNotAnchorable}`；③ 追平不上必须具名：`Compacted{earliest}` / `UnknownStream` / `CursorBeyondStream{next_offset}` / `UnitsNotAnchorable`。推进游标只能靠 `resume_after()`，无缝与重置同算法。
- **"码元→字节没有换算"是实测不是论断**：`😀中`（7 字节 / 整段 3 码元）逐切点的分帧之和 = 3,5,4,3,3,5,4,3。1/2/5/6 破坏可加性；而 cut 3 落在 emoji 内部却**同样等于 3** —— 相等也反推不出对齐，所以任何"按码元猜字节"的实现早晚在某个切点上静默漏读或重复。
- **真 PTY 侧的完整路径**：2 MiB 输出 > 1 MiB 窗口 ⇒ offset 0 必得 `Compacted`+追平点；从追平点按 64 KiB 逐帧取到末端，与测试侧 oracle **逐字节相等**（`frames > 8` 反空转）；取完后是合法的末端空帧。
- **并发边界**：一条持续发布的流上只按 `resume_after()` 推进 ⇒ 已交付字节恒等于"已发布的前缀"，收尾追到末端与整条序列一字不差。这就是 Action 4 那句"snapshot 与 live 的边界用锁/单线程序列化保证"的可执行形式。
- **一条刻意保留的不对称**（记给 grill 轮）：`ByteStream` 在**第一次 publish** 时才建立 ⇒ "还没输出"与"已退场"同答 `UnknownStream`。后果可接受（空视图重置，客户端仍从 0 问）；消掉它要给 `TerminalService` 加开流入口并改宿主 reader 一处（附带一次 pin 重定位），不顺手做。
- **两次变异**：gap 谎成 `Seamless` ⇒ 集成①＋单元各红；把码元游标当字节偏移 ⇒ 单元 `a_utf16_cursor_never_becomes_a_guess` 红，**集成侧没红**（只在退场流上测非零码元游标，那里 `read_from` 本就先失败）——4B 要补一条活流上的码元游标断言。
- **门禁纪律的两条事实**：`runner.floors` 缺条目时 `doctor` 仍 18/18，真守卫是 `inventory --check`（1 inconsistency）+ `skeleton` 门禁（红）——实测确认；改 `checks.json` 的脚本必须先 `json.loads` 验证再落盘（本轮写坏过一次，靠 `git checkout ff1fd07 -- scripts/native/checks.json` 还原）。
- **计数**：`terminal-core` 6 case/**63**（unit 地板 35→**43** ＝ 7 stream + 6 lease + 12 pty + 10 exit + 8 replay）；core workspace **384**/0；`-p aeroric-core` 口径 **334**；clippy/fmt 干净。§8 第 3 条那两条口径仍未合并（收尾轮）。

## 11. 切片四B 实施结果（2026-09-27，补记九十七）：resize 裁决上收 core、控制租约接宿主

落点：`core/src/terminal/input.rs`（8 条单元）+ `tests/terminal_lease.rs`（4 条真 PTY）+ 宿主 `pty.rs::resize_task_pty` 裁决上收 + 门禁 6→**8 case**（`terminal-lease-rust` 4、`host-resize-rules-come-from-core` 守卫；unit 地板 43→**51**）。

- **分工口径**："谁能改这条 PTY"归 core（`decide_resize` 具名表：HiddenView/ZeroExtent/Malformed/Apply，隐藏视图优先；`take_control`/`check_write` 把"隐藏视图连租约都拿不到、被顶替者写入 `NotHolder`、只读订阅免租约"变成可数规则）；"有没有活跃 master、暂存到下一次运行"仍在宿主——core 不猜窗口/进程事实。信箱三处（`stage_pending_size`、`register_pty_handles` 对 `unminted` 键排水、reset 丢弃）一字未动。
- **行为等价是可数的**：旧内联判定与 `decide_resize(·, Foreground)` 的忽略集相同（`rows==0` 被 `rows<2` 蕴含），边界 2 与 10_000 本身合法——单元四条边界逐值钉住。`HiddenView` 行是给 P5-01 native coordinator 备的，旧命令层没有焦点概念一律按前台。
- **真 PTY 证据**：20 次 resize（混隐藏/0×0/畸形）断"应用 11／忽略 9"表数、内核 `get_size()` 与**进程侧 SIGWINCH 自报**同为最后一次前台 (50,200)、被拒形状零到达；顶替控制者用两次有序回显做栅栏证明 ghost 从未到 agent；被拒的隐藏取得不动现有控制权；两读者共享一条流。
- **pin 事故（守卫第一次在真实工作上开火）**：本片宿主 Δ=+8（12 增 4 删），首遍按 +7 迁 8 条 pin ⇒ `--freeze-check` 照样 noop、**`--check` 的 fn 声明回读报 4 条 inconsistency**。结论：`--freeze`/`--freeze-check` 都不校验行号正确性，重定位必须以 `--check` 全绿收尾。
- **变异实证**：(A) 摘掉 `decide_resize` 的隐藏视图早退 ⇒ 单元 `a_hidden_view_is_rejected…` 红 + 集成 `twenty_resizes…` 红（lease 路径用例仍绿＝`take_control` 自己的焦点闸独立成立）；(B) 宿主换回内联形状 ⇒ 守卫禁形命中（FAIL）且 `decide_resize` 正控同时消失，还原字节一致。
- **计数**：`terminal-core` 8 case/**75**；core workspace **396**/0（22 二进制）；`-p aeroric-core` 口径 **346**；tauri-adapter 12/2526（src-tauri 2465＝地板）。未做：history.rs（四C）、executor.rs + 四方调用点（四D）、grill 收尾轮、CPU/RSS 采样。

## 12. 切片四C 实施结果（2026-09-27，补记九十八）：history 压实策略上收 core、共享 owner

落点：`core/src/terminal/history.rs`（7 条单元）+ 宿主 `storage.rs` 三调用点消费 core（文件 I/O 留宿主）+ 门禁 8→**10 case**（`terminal-history-rust` 7、`host-history-policy-comes-from-core` 守卫；unit 地板 51→**58**）。

- **Action 4 前半句的落地形状**：迁移前"尾切 + 续字节跳过 + 高水位判定"在宿主 storage.rs 有**三份近似重复**（str 版 `utf8_tail`、字节版 `utf8_tail_bytes`、读路径内联 `take(4).take_while`），算术又在两条调用点各写一遍。现在 core 一份实现：`HistoryRetention::LEGACY`（8 MiB 保留 + 4 MiB slack，理由注释随迁）、`needs_compaction`（追加与目录巡检＝同一问，巡检＝incoming 0）、`disk_tail_bytes`（压实回 max 而非 max+slack——摊薄成立的前提）、`utf8_tail_bytes`、`skip_leading_utf8_continuations`（读路径传 4）。
- **两条有界面的差异是刻意的**：replay 窗口按原始字节切、截断点落在字符中间就如实交出（四A 用例钉着）；磁盘历史对齐后才落盘（读者直接当文本）。共享的是"保留尾部、丢头部、对丢失如实"的形状，不是参数——不要"统一"。
- **行为不变**：宿主 4 条既有 `terminal_history_*` 测试原样跑通；core 侧把迁移前的 str 版 is_char_boundary 算法与读侧 take(4) 语义**原样重放当 oracle**，合法 UTF-8 上逐字节相等。
- **变异实证**：C1 `needs_compaction` 的 `>` 翻成 `>=` ⇒ 2 条单元红；C2 宿主重新注入本地 `fn utf8_tail_bytes` ⇒ 守卫 FAIL，还原字节一致。守卫首版禁形误伤刻意保留的委托壳——按补记九十五先例改注释措辞并加严禁形（`is_char_boundary` 注释里也不许），不放宽扫描。
- **纪律实录**：新 test case 漏配 `runner.floors` 条目 ⇒ `inventory --check` 报 inconsistency（"真守卫是 inventory --check 不是 doctor"第三次实证）；storage.rs 5 条 pin 重定位到当前真实声明行（367/384/406/582/592），`--check` 回读全绿收尾。
- **计数**：`terminal-core` 10 case/**89**；core workspace **403**/0（22 二进制）；`-p aeroric-core` 口径 **353**；tauri-adapter 12/2526（src-tauri 2465＝地板）。未做：executor.rs + 四方调用点（四D）、grill 收尾轮、CPU/RSS 采样。

## 13. 切片四D 实施结果（2026-09-27，补记九十九）：PTY 装配唯一启动体 + 四方 acquire/release 盘点

落点：`core/src/terminal/executor.rs`（`activate_pty_session` + `PtySessionHandles`，2 条真 PTY 单元）+ `runtime/registry.rs::register_pty_session`（宿主注册函数逐字迁移）+ 宿主 **7 个站点**换调 core（local launch / local resume / open-shell / SSH 任务 / SSH 终端 / WSL 任务 / WSL shell）+ 门禁 `terminal-core` 10→**11 case**（`single-pty-activation-mechanics` 守卫；unit 地板 58→**60**）。

- **Action 6"不保留两份同 target 的启动 body"的落地形状**：spawn→丢 slave→reader→writer→按 RunKey 注册（未铸造键暂存 resize 排水）这五步机械此前在宿主有**七份逐字相同**的拷贝；现在唯一一份在 core，注册本体在 TaskRegistry（句柄表是它的字段）。SSH/WSL 与 shell 的**命令构造与编排**留在原分支（SSH/WSL 分支本体留 P7、DSH 留 P3-05）——它们只是不再各自手写装配步骤。
- **四方 acquire/release 盘点（Action 3 的收尾记账，按约定如实标注现状而不是宣称完成）**：
  - **legacy（Tauri，今天）**：不取得控制租约——`send_input`/`resize_pty` 直接写 PTY（resize 已过 core `decide_resize` 前台表）。这是迁移期行为不变的如实记账，不是缺口：提前给 legacy 接租约=改变旧客户端行为（INV-04）。cutover（P8-05）后由宿主唯一入口统一 acquire。
  - **native（P5-01）**：TerminalCoordinator 视图聚焦时 `take_control(Foreground)`，失焦/切模式按 §2.4 顺序 release；被顶替者 `NotHolder`、隐藏视图拿不到租约——core 侧反例已就位（切片四B）。
  - **CLI（P3-07）**：计划原文"task input 对 PTY 要求有效控制租约"，attach 的 SIGWINCH→resize 走宿主命令=前台表；acquire/release 接入与验收在 P3-07。
  - **mobile（P8-03）**：终端流订阅只读免租约（terminal_hub 现语义）；多设备抢控制权显式确认、失效设备不能继续输入（P8-03 验收原文），经同一 acquire 面。
- **变异实证**：D1 注册时跳过暂存排水（`filter(|_| false)`）⇒ executor 用例红；D2 宿主重新出现 `.spawn_command(` ⇒ 守卫 FAIL，还原字节一致。
- **两条过程教训（照实记）**：① 宿主文件在 pin 重定位**之后**又被 `cargo fmt` 重排（+2 行）⇒ `--check` 的 fn 声明回读抓出 38 处错位——**pin 重定位必须是包括 fmt 在内的最后一次编辑之后做的事**；② 又一次踩了"管道后读 `$?` 读到的是 head 的退出码"（补记九同款），D2 变异首跑因此假绿，改用 `rg -c; rc=$?` 后才拿到真判据。
- **计数**：`terminal-core` 11 case/**91**；core workspace **405**/0（22 二进制）；`-p aeroric-core` 口径 **355**；tauri-adapter 12/2526（src-tauri 2465＝地板）。四C/四D 之后 terminal 的 core 侧原件齐了：stream/lease/pty/exit/replay/input/history/executor。

## 14. 收尾轮·自主部分（2026-09-27，补记一百）

- **四A 覆盖不均了结**：terminal_replay 第 5 条在**活流**上钉住码元游标拒收的完整契约——`Reset{from:0, reason:UnitsNotAnchorable, bytes=现存窗口}`（重置视图+补屏，不是空手而回——四A 的候选断言把 bytes 写成空是错的，真跑当场纠正）；`resume_after()`=窗口末端、末端合法空帧；唯一无缝例外＝units 0 且保留线 0。变异 E（吞掉重绘窗口）被它当场抓住。地板 4→**5**。
- **CPU/RSS 证据**（验收"压力结果与 CPU/RSS 在证据中"）：200ms 采样 21 样本——测试进程 RSS 峰值 **6.0 MiB**（对照 100 MiB 吞吐=有界）、node 替身 50.9 MiB、测试 CPU ~99% 单线程、102434 帧/6.8s/保留 1024 KiB。
- **§8 第 3 条了结**：`storage-core-rust` minCases/floors 81→**356**（包口径=机器地板，workspace 406=叙事口径不做地板；此后新增 core 用例必须同批抬）。
- **复跑对照**：typecheck 绿；全量 `pnpm test` 376/5483 绿（与切片三基线同数）。
- **剩余（用户在场）**：grill-me [terminal-core] 收尾拷问（三条待裁决+两条新挂账，见补记一百）、`~/.aeroric` 真实终端会话手测；完成后 P3-03 才标 accepted。
