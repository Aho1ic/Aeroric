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

`term.onWriteParsed` 是这里的闭环信号：每帧最多触发一次、在解析完成后。发一批就等它，超过 `WHEEL_REPAINT_GRACE_MS`（70ms）无条件推进。**超时兜底不能省**：agent 滚到顶/底时一个字节都不回吐，只等信号会把队列锁死，症状是"滚到顶之后再往回滚要卡一下"。

第二层是缓动：每帧条数改成 `ceil(剩余 / 3)` 再夹进 `1..4`，尾部自然收窄到 1 行/帧。固定 4 行/帧是匀速直线，每帧跳约 64px 然后停住，台阶感本身就读作"顿"。

第三处是**上报不能算用户输入**。上报走 `term.onData` → `sendInput` → `writer.pauseForUserInput()`：48ms 输出挂起 + 两次 `refreshTerminalCursorLine`。滚动期间每帧好几条，等于反复把 agent 的重绘往后推。`resumeOnAnyOutput` 会在下一次输出时撤销挂起所以很少真停死，但那些光标行重绘和写队列的 hold/cancel 抖动是白付的。判据用 `isReplayingWheel()`——合成事件是同步 dispatch 的，`onData` 就发生在 `beginReplay`/`endReplay` 之间，这个标志天然精确，比正则匹配 `\x1b[<…M` 稳得多。

写测试时注意：**"最后一帧比第一帧小"证明不了减速**，固定速率跑完最后也会剩个零头。要断言的是渐进的尾巴（末三帧都 ≤ 2 且总帧数多于 `ceil(总行数 / 每帧上限)`）。改完拿"把预算换回固定值"跑一遍，测试必须真的红。

## 能做到真平滑的只有本地滚动

`smoothScrollDuration`（`initTerminal`，100ms）只作用于 xterm 亲自滚 viewport 的场景：shell / SSH / WSL 面板，以及 agent 终端**不在 alt screen** 的时候。开了鼠标上报的 alt screen 由 agent 重绘，这个选项完全管不到——那条路径最细的单位就是一行，不存在亚像素。要让 agent 终端真的浏览器式平滑，只能关掉鼠标上报并自己维护本地 scrollback 视图，代价见文末那张表。

## 滚轮兜底只装 agent 终端

`attachTerminalWheelScroll` 只在 `TerminalView` 里装，不要放回 `initTerminal`：shell / SSH / WSL 面板共用 `initTerminal`，那边的 `less`、`man`、`git log`、`vim`、`htop` 正是靠这条 alternate-scroll 方向键滚动的，吞掉等于它们也滚不动，而 shell 里没有会被方向键顶掉的输入框。

## 怎么验证上报真的开了

扫最新 transcript（`~/.aeroric/terminal-history/*.log`）里的 DECSET 参数，**只数 `h`（开启）不要数 `l`**：agent 退出时会无条件复位 `1000/1002/1003/1006`，即使全程没开过。混数 `h` 和 `l` 会得出"24 个会话开了鼠标"的错误结论，实际是 0 个。`?1049h` = alt screen，`?1002h`/`?1006h` = 鼠标上报真的开了。

## 权衡

| 鼠标上报 | 滚轮 | 框选 |
| --- | --- | --- |
| 开 | agent 自己逐行滚，手感最好 | 拖动被转发给 agent，原生选区要按 ⌥Option（macOS，`initTerminal` 已开 `macOptionClickForcesSelection`）或 Shift（Win/Linux） |
| 关 | 需要前端把滚轮翻译成 PageUp/PageDown 才能翻看（页粒度） | 拖动即选，无需修饰键 |

`d18976871` 选了"保框选"，2026-08-22 改成"保滚动"。要改回去就是在 `build_claude_cmd` 重新设 `CLAUDE_CODE_DISABLE_MOUSE=1` + 在兜底里发 `ESC[5~` / `ESC[6~`。
