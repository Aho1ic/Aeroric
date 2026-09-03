# Agent 终端的滚轮语义与鼠标上报

结论：**agent 终端能不能用滚轮翻看，取决于 agent 有没有开鼠标上报**。上报开着，滚动由 agent 自己做；上报关着，alt screen 里根本没有可滚的东西，xterm 会把滚轮合成成方向键——这是"滚一下顶掉草稿"的真因。

## 为什么 alt screen 里滚轮会改写输入框

- Claude Code 开局就进 alt screen 并一直待到退出（transcript 第 200 字节 `ESC[?1049h`，退出前才 `1049l`）。
- alt buffer 没有 scrollback，于是命中 xterm 的兜底 `node_modules/@xterm/xterm/src/browser/CoreBrowserTerminal.ts:814`：把滚轮翻译成 `ESC[A` / `ESC[B` 发给程序。
- Claude Code 的键位表里 `Chat` 上下键是 `history:previous` / `history:next`，`Scroll` 才是 `pageup/pagedown → scroll:pageUp/pageDown`、`wheelup/wheeldown → scroll:lineUp/lineDown`。所以方向键有害、PageUp 安全、滚轮上报最理想。

## 踩坑：鼠标上报有两个禁用来源

2026-08-22 改完 `src-tauri/src/pty.rs` 里 `build_claude_cmd` 的 `CLAUDE_CODE_DISABLE_MOUSE=1` 后症状照旧，因为**跑的还是旧二进制**——`/Applications/Aeroric.app` 当时并没有被换掉。这个变量是旧二进制**逐个子进程注入**的，不是登录会话级污染：app 进程自己的环境里没有它（`ps eww -p <app pid>` 数出 0），`launchctl getenv CLAUDE_CODE_DISABLE_MOUSE` 也是空的，只有被它拉起的 claude 子进程有。

所以验证"这类 env 改动生效了没"要看的是**新拉起的 agent 子进程**，而且前提是 app 本体确实已替换（比对 `Contents/MacOS/aeroric` 的时间戳，或直接在二进制里 grep 变量名）。顺带一个连带效应：`cargo test` 跑在 agent 自己的 shell 里，那个 shell 的环境就带着被注入的变量，而 `CommandBuilder::new()` 会快照父进程环境（见 `pty.rs` 中 `setup_env` 的注释），所以断言"env 为 None"会失败——要跟基线 `CommandBuilder` 对比才只证明"我们没有再自己设它"。

配套的一个认知错误也记在这里：`d18976871` 的注释称关掉鼠标后"滚轮回退到 xterm 自身 scrollback"，这是错的（alt screen 没有 scrollback），错误前提又催生了"alt screen 里直接吞掉滚轮"的补丁，症状从"顶掉草稿"变成"完全没反应"。

## 上报开着，但一个滚轮事件只换来一行

开了上报之后还有第二个坑：**xterm 算出了行数却只发一条上报**。`CoreBrowserTerminal.bindMouse` 的 wheel 分支拿 `consumeWheelEvent` 的结果只当有无判断，随后 `action = deltaY < 0 ? UP : DOWN` 发一次就结束，注释写得很直白——"has been simplified to simply send a single up or down sequence"。而 agent 那边一条上报就是一行（Claude Code 的 `wheelup → scroll:lineUp`），于是滚轮转多远都只滚一行，症状是"滚了很长行程、终端几乎不动"。

`scrollSensitivity` 治不了：倍数加在那个被丢弃的行数上，下游只看正负号。`consumeWheelEvent` 里还有一条反向的阻尼——`|deltaY| < 50` 判定为"疑似触控板"后乘 `0.3`。

`attachTerminalWheelScroll` 因此在上报开着时接管事件：按 `deltaY ÷ 行高` 算出真实行数，再往 xterm 自己的 listener 上派发同样多个 `deltaMode = LINE`、`deltaY = ±1` 的合成事件。走 LINE 分支时 `consumeWheelEvent` 既不碰 cell 高度也不碰触控板阻尼，一个事件恰好换一条上报，于是行程 1:1。编码、协议门禁、坐标换算全留给 xterm，我们不自己拼鼠标序列。

几个必须记住的约束：

- 合成事件要带原事件的 `clientX/clientY`。`getMouseReportCoords` 用它算格子，缺了拿不到 pos，整条上报被静默丢弃。
- 合成事件**不要**带修饰键。`_applyScrollModifier` 对带修饰键的事件乘 `fastScrollSensitivity`（默认 5），那样每条合成事件就不止一行；快滚倍数本来就已经体现在原事件的 `deltaY` 里。
- 不足一行的余量要按实例累计，方向反转时清零。丢掉余量就是触控板"轻滑没反应"，不清零则回滚的第一下会被抵掉。
- 一个事件要有上限（现在是三屏）。macOS 惯性甩动单个事件能给上千 px，不设限会一次往 pty 写几百条序列。
- 拿不到 `term.element` 或量不到行高时交回 xterm。至少还能滚一行，比彻底不动好。

验证这条链路别只信 jsdom：`getMouseReportCoords` 依赖真实排版，jsdom 给 0×0 rect 会让上报全被丢掉，测试可能因为错误的原因通过。要用真实 `Terminal` 验证得先补三样东西——`_renderService.dimensions`、`_charSizeService.hasValidSize`、`_coreBrowserService.dpr`（都是只读 getter，得 `defineProperty`），再给 `.xterm-screen` 一个 `getBoundingClientRect` 和显式的 `padding-left/top`（jsdom 的 computed padding 是空串，`parseInt('')` = NaN 会把坐标算成 NaN）。补齐后实测：480px 事件在原生 xterm 下只产生 `\x1b[<65;1;1M` 一条，接管后按 16px 行高产生 30 条。

## 行程对了，但手感还是卡：开环节流

行程 1:1 之后剩下的症状是"卡顿感"——不是速度不够，而是**帧间距不均**。成因是那版 pacer 是开环的：固定每帧发 4 条上报，完全不管 agent 画完没有。一次全屏 TUI 重绘通常超过一帧，于是我们持续跑在 agent 前面，上报堆在 pty 里，画面以"憋一下、跳一段"的方式回来。

`term.onWriteParsed` 是这里的闭环信号：每帧最多触发一次、在解析完成后。发一批就等它，超过 `WHEEL_REPAINT_GRACE_MS`（50ms，约三帧）无条件推进。**超时兜底不能省**：agent 滚到顶/底时一个字节都不回吐，只等信号会把队列锁死，症状是"滚到顶之后再往回滚要卡一下"。宽限期也**不能定得比一帧还短**，否则每帧都走超时推进，闭环退化成开环。

第二处是**上报不能算用户输入**。上报走 `term.onData` → `sendInput` → `writer.pauseForUserInput()`：48ms 输出挂起 + 两次 `refreshTerminalCursorLine`。滚动期间每帧好几条，等于反复把 agent 的重绘往后推。`resumeOnAnyOutput` 会在下一次输出时撤销挂起所以很少真停死，但那些光标行重绘和写队列的 hold/cancel 抖动是白付的。判据用 `isReplayingWheel()`——合成事件是同步 dispatch 的，`onData` 就发生在 `beginReplay`/`endReplay` 之间，这个标志天然精确，比正则匹配 `\x1b[<…M` 稳得多。

## 闭环反而把跟手感吃掉了：改成"突发 + 闭环长尾"

2026-08-23 的症状是"滚轮有明显延迟"。上一节那套 pacer 把**每一条**上报都排进 rAF 并等重绘，于是最快的一次响应也要等一帧 + 一次 agent 重绘；缓动那层更进一步——`ceil(剩余/3)` 夹进 `1..4` 意味着一次普通滚动（约 3 行）要三帧才走完，一次甩动要十几帧、十几次 agent 重绘。用户感知到的不是"匀速"，是"手停了画面还在爬"。

现在的策略是**第一屏同步发、超出部分才排队**：

- `WHEEL_BURST_SCREENS`（1 屏）的额度在 `enqueue` 里当场发掉，不排 rAF、不等重绘信号。手感的全部来源就是这一步：普通滚动整个落在突发额度内，和事件同一个 tick 就进了 pty。
- 只有超出突发额度的部分（惯性甩动）才进队列，按帧发、等 `onWriteParsed`。长尾本就落后于手，闭环在这儿仍然有用——它是防洪，不是手感。
- 突发额度只在队列空着时给。队列里还有货说明我们已经跑在 agent 前面了，再突发就是加深管道深度。
- 每帧配额从 4 提到 `MAX_WHEEL_REPORTS_PER_FRAME`（16）。反直觉但是对的：**一次 write 带多条上报，agent 是一次 read、一次重绘**；拆细既加延迟又加重绘次数。旧配额在两个轴上都更差。
- 缓动整层删掉了。它是为"匀速台阶感"设计的，而现在普通滚动根本不进队列，缓动只剩下"让甩动的尾巴变慢"这一个效果——那正是延迟感的来源。

写测试时注意：算行数要把两道上限一起算进去，否则期望值凭空多出一截。`MAX_WHEEL_LINES_PER_EVENT_SCREENS`（3 屏）截单个事件，`MAX_PENDING_WHEEL_SCREENS`（3 屏）截队列存量——`rows: 10` 时 `deltaY: 800` / 16px 行高不是 50 行而是 30 行。断言"突发是同步的"要在 `handler()` 返回后**立刻**数，一旦先 `await nextFrame()` 就分不清同步发出和排队发出。断言"等重绘"时别让等待横跨宽限期（现在只有 50ms，两个 rAF 就够超时）。改完拿"把 `WHEEL_BURST_SCREENS` 归零、每帧配额换回 4"跑一遍，测试必须真的红。

## 正常缓冲区自己滚，不要交回 xterm

不在 alt screen 时（agent 空闲、`git log` 之外的普通输出）滚轮走的是本地 viewport，这条路径也不能交回 xterm：`consumeWheelEvent` 会对 `|deltaY| < 50` 的事件乘 `0.3` 当"疑似触控板"阻尼，行程直接缩到三分之一，而这一节要的恰恰是 1:1。所以正常缓冲区用同一套 `wheelLinesForEvent`（含亚行余量累计）算行数，再 `term.scrollLines()`。

一个容易吞掉滚轮的陷阱：`wheelLinesForEvent` 返回 `0` 有两种含义——「余量还没攒满一行」（该吃掉事件）和「量不到行高」（该交回 xterm）。不加区分就会在还没渲染 / rect 为 0 时让滚轮**彻底没反应**。所以先单独判 `measureCellHeight() === null`，且只对 PIXEL 事件判——LINE / PAGE 根本不需要行高。

`smoothScrollDuration` 同时从 100ms 调成了 **0**。它用 rAF 插值 viewport，定义上就让画面滞后于手指一个插值周期——"跟手"和"插值平滑"是互斥的，这里选跟手。它只作用于 xterm 亲自滚 viewport 的场景；开了鼠标上报的 alt screen 由 agent 重绘，那条路径最细的单位就是一行，这个选项完全管不到。要让 agent 终端真的浏览器式平滑，只能关掉鼠标上报并自己维护本地 scrollback 视图，代价见文末那张表。

## 滚轮兜底只装 agent 终端

`attachTerminalWheelScroll` 只在 `TerminalView` 里装，不要放回 `initTerminal`：shell / SSH / WSL 面板共用 `initTerminal`，那边的 `less`、`man`、`git log`、`vim`、`htop` 正是靠这条 alternate-scroll 方向键滚动的，吞掉等于它们也滚不动，而 shell 里没有会被方向键顶掉的输入框。

## 怎么验证上报真的开了

扫最新 transcript（`~/.aeroric/terminal-history/*.log`）里的 DECSET 参数，**只数 `h`（开启）不要数 `l`**：agent 退出时会无条件复位 `1000/1002/1003/1006`，即使全程没开过。混数 `h` 和 `l` 会得出"24 个会话开了鼠标"的错误结论，实际是 0 个。`?1049h` = alt screen，`?1002h`/`?1006h` = 鼠标上报真的开了。

## 上报开着时，压住输出等于把 agent 画的选区憋到松手

macOS WKWebView 那道选区守卫（`attachMacWebKitTerminalGuard`，见文件头注释）在 pointerdown 时会 `setSelectionPaused(true)` 暂停写入，避免拖动期间的重绘和 `characterIndexForPoint` 风暴打架。问题是它原来**无条件**暂停。

上报开着时拖动是转发给 agent 的，选区由 agent 自己画、通过输出回来——把输出压住，那些帧就一路憋到 pointerup 才刷出来，症状正是"松手之后才出现选择区域"。所以暂停之前要先判断这次按下形成的是谁的选区。

判据必须和 xterm 自己那道门一致（`CoreBrowserTerminal.bindMouse`：`!areMouseEventsActive || shouldForceSelection(ev)`），判错一边就会压错对象；`dragMakesLocalSelection` 就是这道门的镜像：

- `areMouseEventsActive` 等价于 `mouseTrackingMode ∈ {vt200, drag, any}`。**`x10` 不算**——它的 events 只有 DOWN，`SelectionService` 照常工作，是本地选区。
- `shouldForceSelection` 在 macOS 上是 `altKey`（配合 `initTerminal` 已开的 `macOptionClickForcesSelection`），其余平台是 `shiftKey`。所以 ⌥Option 拖动仍然要压。
- 读不到 `modes` 时按本地选区处理。这是修复前的行为，压错方向最坏只是拖动期间画面不动；反过来猜错会把本地选区抖成一团。

`textarea.disabled` 那层保护跟选区归属无关，两条路径都要保留。

## 权衡

| 鼠标上报 | 滚轮 | 框选 |
| --- | --- | --- |
| 开 | agent 自己逐行滚，手感最好 | 拖动被转发给 agent，选区由 agent 画（**不能压住输出**，见上节）；原生选区要按 ⌥Option（macOS，`initTerminal` 已开 `macOptionClickForcesSelection`）或 Shift（Win/Linux） |
| 关 | 需要前端把滚轮翻译成 PageUp/PageDown 才能翻看（页粒度） | 拖动即选，无需修饰键 |

`d18976871` 选了"保框选"，2026-08-22 改成"保滚动"。要改回去就是在 `build_claude_cmd` 重新设 `CLAUDE_CODE_DISABLE_MOUSE=1` + 在兜底里发 `ESC[5~` / `ESC[6~`。
