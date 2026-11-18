# Agent Note: ACP 握手的诊断日志

Status: implemented

## Problem

2026-11-16 复测时，讨论"再来一轮"里 DSH 成员两次报"DSH 握手超时（30 秒没有回答）"；单独启动一个 DSH 会话 4 秒就回。没法判断是进程起得慢、`initialize` 没回，还是别的原因。

当时的代码（`main@9603b442`，`core/crates/aeroric-core/src/sessions/live/acp/driver.rs` 的 `process()`）：

- 只有 `initialize` 有 30 秒超时；超时或失败只返回一句话，不记任何耗时。
- `codex/rpc.rs` 的 `Transport::spawn` 把进程 stderr 的前 40 行写进宿主日志，但不带时间，也看不出是哪个会话的进程；握手失败时拿不到这个进程最后说了什么。

## Decision

**只加诊断，不改握手行为。** 超时时长、重试、给用户的报错文字都不变。

- **进程快照**：`Transport::spawn` 给每个进程建一个 `ProcessWatch`（进程号、启动时刻、stderr 最后 20 行，每行截到 300 字、前面带距启动的毫秒数），`ChildGuard::watch()` 取它。宿主日志里的 stderr 行也带上这个毫秒数（仍然只打前 40 行）。握手要跨 `await` 持有它，而 `ChildGuard` 因为装着清理闭包不是 `Sync`，所以单独拿出一个可共享的快照。
- **阶段耗时**：`process()` 记下启动进程、`initialize`、`session/new` 或 `session/resume`、`session/set_mode` 各自的耗时。成功时在宿主日志写一行，例如 `[acp] DSH task=t1 pid=4242 握手 4210ms：启动 12ms · initialize 3980ms · session/new 218ms`。
- **停在哪一步**：任何一步失败或超时（启动失败、`initialize` 失败或超时、协议版本不符、不支持恢复、恢复或新建失败、权限模式被拒），宿主日志写明停在哪一步、这一步等了多久、进程已运行多久、之前各步的耗时、报错原文，再附这个进程 stderr 的最后几行（没有输出时写"stderr 没有输出"）。
- 格式化是纯函数 `Handshake::summary` / `Handshake::failure`；日志由 `stopped()` 写出并把原报错原样交回调用方。

## Alternatives considered

**直接用宿主 RPC 反复点"再来一轮"复现。** 最强的理由是马上就能知道原因。没选它（owner 2026-11-16 定下）：复现要真实的 DSH 和讨论，一轮要几分钟，而且没有日志也只能再看到一次"超时"；先加日志，下次目检顺带复现就有据可查。

**把超时改长，或失败后自动重试一次。** 最强的理由是用户少看到一次失败。没选它：还不知道原因，改超时可能只是把问题藏起来；先有证据再决定。

## Consequences

- **收益**：下次 DSH 握手超时，宿主日志会说明卡在启动、`initialize` 还是建会话，进程那时跑了多久、stderr 最后说了什么。
- **代价**：每次 ACP 连接多一行宿主日志。stderr 可能含敏感信息：只进宿主日志（宿主日志本来就收 stderr 前 40 行），不进会话、不进界面。
- **验证**：`acp::driver::tests::a_handshake_is_logged_step_by_step` 测成功行、超时报告和启动失败报告的原文；`codex::rpc::tests::the_last_stderr_lines_of_a_process_are_kept` 用真实的 `sh` 子进程写 25 行 stderr，断言只留最后 20 行且都带毫秒数。原有的 ACP 驱动测试照常通过。超时原因本身还没查到，要等下次目检时用 DSH 复现，看宿主日志。
