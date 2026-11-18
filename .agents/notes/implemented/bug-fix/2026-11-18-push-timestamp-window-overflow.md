# Agent Note: 推送发送端的时间戳窗口不再溢出

Status: implemented

## Problem

2026-11-17 全量审查交接单 B4-1（Critical）：`remote-push/src/auth.rs` 的 `timestamp_in_window` 写的是 `(now_ms - issued_at_ms).abs() <= window_ms`，而 `issued_at_ms` 来自还没认证的请求。

- Debug 构建：`issued_at_ms = i64::MIN` 时减法溢出，进程 panic。任何人发一个请求就能打崩发送端。
- Release 构建：取 `now_ms.wrapping_add(i64::MIN)` 时差值回绕成 `i64::MIN`，`abs()` 仍是负数，于是"在窗口内"，时间窗被绕过。

## Decision

**非正的时间戳、非正的当前时间、负的窗口一律拒绝；差值用 `i64::abs_diff` 求（结果是 `u64`，不会溢出），再和窗口比较。**

- 测试 `timestamp_window_is_symmetric_and_bounded` 补了以下边界，都必须拒绝且不 panic：
  - `issued` 取 `i64::MIN`、`i64::MIN + 1`、`-1`、`0`、`i64::MAX`、`now.wrapping_add(i64::MIN)`；
  - 负窗口；
  - 当前时间为 `i64::MIN`。

## Alternatives considered

**用 `checked_sub`，溢出时拒绝。** 最强的理由是只改一行。没选：它挡住了溢出，但 0 和负的时间戳仍会当成"很久以前"参与比较；时间戳本来就该是正的 epoch 毫秒，直接拒绝更清楚。

## Consequences

- **收益**：未认证的请求打不崩发送端，也绕不过时间窗。
- **代价**：时钟回到 1970 年以前的机器发出的请求会被拒绝，现实中不会出现。
- **验证**：2026-11-18 在 Mac 上：
  - `cargo test --manifest-path remote-push/Cargo.toml --locked`：26 + 8 个通过；
  - `cargo clippy --all-targets -D warnings` 通过。
