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
