# 验收预算与实测（AP8 R-AP8-4 / 原 P10-01 Action 4/5）

> 载体:`scripts/native/stress.mjs`(固定口径压测)、门禁 `system-parity` 的 `stress-headless`。
> 需求源:`e3836e62:MIGRATION_PLAN.md` §Task P10-01 Action 4(负载量与机器口径)与 Action 5(初始预算)。

## 1. 固定口径

一个"随机器规模变化"的压测,两次的数字不可比,于是"这次怎么慢了一倍"永远答不出。
所以负载常量写在 `stress.mjs` 的 `LOAD` 里,**改动要附理由与新的基准**:

| 项 | 值 | 来源 |
|---|---|---|
| 单任务输出 | 5 MiB(5184 行 × 2000 B) | 计划 Action 4「每个 5 MiB 输出」 |
| 单行宽度 | 2000 B | 让 5184 行越过 8 MiB 历史保留线,顺带把压实路径压到 |
| 模式切换等价的 RPC | 100 次 | 计划 Action 4「100 次模式切换」 |
| 并发客户端 | 3 | 计划 Action 4「3 客户端」 |
| 重连 | 10 次 | 计划 Action 4「10 次重连」 |

计划里还有两项(10 000 条消息、100 000 文件索引样本)属于**规模**而非**延迟**,
它们各自有专门的载体(`task-events-log-rust` 的事件题、`notebook-rag-host-rust` 的索引),
不在这里重复造一遍——`stress.mjs` 只回答"切换与任务的延迟预算"。

## 2. 预算对照(2026-10-13 实测)

| 预算 | 来源 | 阈值 | 实测 | 结论 |
|---|---|---|---|---|
| 热切换 p95 | P10-01 Action 5 | < 150 ms | **5.8 ms**(p50 5.3 / p99 6.2) | 通过 |
| 磁盘终端历史保留 | `terminal/history.rs::HISTORY_MAX_BYTES` | 8 MiB + 4 MiB slack | 见下方缺口 | — |
| 100 次切换后子进程数回基线 | P10-01 Action 5 | = 基线 | 0 → 0 | 通过 |
| 宿主 RSS 不随负载无界增长 | 本任务新增(计划未给阈值) | 增长 < 64 MiB | +8.0 MiB(21.1 → 29.1) | 通过 |
| PTY replay 单连接队列 ≤ 4 MiB | P10-01 Action 5 | ≤ 4 MiB | **not-measured** | 见 §4 |
| 业务 replay ≤ 8 MiB | P10-01 Action 5 | ≤ 8 MiB | **not-measured** | 见 §4 |
| renderer / subscriber 数回基线 | P10-01 Action 5 | = 基线 | **not-measured** | 见 §4 |
| 离线打开 shell 不等模型网络 | P10-01 Action 5 | 立即可用 | **not-measured** | 见 §4 |

## 3. 实施中发现的真实缺口:`GAP-AP8-HISTORY-WRITER`(已修)

压测第一版把"历史保留上限"写成了一条**恒真**的断言:`history=0.00 MiB vs cap 8 MiB`
——因为文件根本没被创建。追下去是一件真事,不是压测写错了:

**终端历史的写侧此前只在待退休的 Tauri 桥里**。

| 面 | 位置(修前) | 现状 |
|---|---|---|
| 读 | `core/.../terminal/history.rs::read_terminal_history_tail_from_path` | 上收完成,有门禁 |
| 删 | `core/.../terminal/history.rs`(经 `services.rs:841`) | 上收完成,有门禁 |
| **写** | `src-tauri/src/pty.rs:607` → `storage::append_task_terminal_history` | **只在旧桥** |

后果:无头宿主(`aeroric-cli serve`)与原生 Mac 端跑完的任务,在 `terminal-history/`
下**没有日志**;`project.read_task_terminal_history` 对刚跑完的任务返回空。
core 的 `history.rs` 模块头当时如实写着"追加落盘机制与目录巡检仍是宿主的事"——
那句在 Tauri 是唯一宿主时成立,迁移到 Apple-only 之后**新宿主一个都没有**它。

**处置**(AP8 内修,不留给 AP10):

- core `terminal/history.rs` 新增 `append_terminal_history`(纯 append 快路径 + 超水位
  原子重写,策略与读侧共用同一个 `HistoryRetention`,不是第二份参数)。
- 宿主 PTY reader 落一份磁盘历史(`core/crates/aeroric-host/src/executor.rs`)。
- 实测:无头宿主跑 5 MiB 输出的任务后,`terminal-history/<task>.log` **9.9 MiB**
  (落在 8 MiB + 4 MiB slack 的设计窗口内),经 `project.read_task_terminal_history` 读得回来。

## 3b. 同一段压测暴露的第二个真缺陷:答复装不进一帧(已修)

修完写侧,读回 9.9 MiB 的历史时客户端报
`stream error: host closed the connection at .../host.sock`。

根因:`read_terminal_history_tail_from_path` 的上限是 `HISTORY_MAX_BYTES`(8 MiB),
而 `MAX_FRAME_BYTES` 是 **1 MiB**。把整个尾部塞进 `result` → `write_frame` 超限失败
→ 宿主断连。于是"小历史读得到、**大历史读不到**",而大历史正是最需要它的时候。

处置:窗口钳到帧预算之内(256 KiB,JSON 转义最坏 6× 仍有余量),支持 `maxBytes` 由
调用方收窄。两条回归用例钉住(`terminal_history_services`,floor 3→5):

- `a_history_larger_than_one_frame_is_read_back_bounded_not_by_dropping_the_connection`
  —— 4 MiB 历史读回来的字节数 ≤ `MAX_FRAME_BYTES`,**且连接仍可用**(原缺陷丢掉的正是后者)。
- `a_bounded_history_read_keeps_the_tail_not_the_head` —— 受限读取丢的是**开头**,
  读历史的人要的是刚发生的事。

## 4. 为什么有些预算不在这里测

`not-measured` 不是省略,是**如实标记**:把渲染器指标在无头环境里"测"出来比不测更糟。

| 预算 | 为什么这里测不了 | 归谁 |
|---|---|---|
| PTY replay 单连接队列 ≤ 4 MiB | 需要一条真实远程 WS 连接 + 慢消费者;这里只有本地 UDS。**相关的上限常量存在**(`remote/terminal_frames.rs::MAX_PAYLOAD_BYTES` = 512 KiB、`remote/server.rs::MAX_MESSAGE_BYTES` = 1 MiB、`OUTBOUND_QUEUE_CAPACITY` = 256),队列上限由它们共同约束 | AP8 的真机远控段 / AP9 |
| 业务 replay ≤ 8 MiB | 同上 | 同上 |
| renderer / subscriber 数回基线 | renderer 是原生端进程内对象,无头宿主里不存在 | `native:ui-test` / AP8 的原生段 |
| 离线打开 shell 不等模型网络 | 需要"网络不可达"的环境;本机无法可靠构造 | AP8 的原生段 |

## 5. 怎么跑

```bash
node scripts/native/stress.mjs
node scripts/native/check.mjs system-parity
```
