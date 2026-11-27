# Agent Note: E9 第二步：真实 Codex 的历史升级测试，0.162.1 → 0.165.0 → 0.169.1 全部通过

Status: implemented

## Problem

[1118 调研](../../proposed/architecture/2026-11-19-new-addition-1118-research.md) 的 E9 是 Codex 升级纪律。[第一步](2026-11-23-e9-codex-contract-check.md)用仓库里的协议快照核对驱动发出的每条消息，第二步“历史升级测试”要真实的新旧二进制，一直 NOT RUN：
- 用旧版 Codex 生成两轮历史；
- 用新版恢复同一个线程，接着聊；
- 断言回合 id 和上下文都保留。

协议快照只说明新版“收不收”驱动发的消息，说明不了新版能不能读旧版写下的会话文件。Codex 升级后旧会话接不上，用户看到的是“无法恢复 Codex 会话”，而快照核对照样全绿。

2026-11-27 owner 同意跑这一步，并同意从 npm 下载 `@openai/codex@0.162.1-darwin-arm64` 和 `@openai/codex@0.169.1-darwin-arm64`。

## Decision

**测试：`codex/real_tests.rs` 的 `real_codex_history_survives_an_upgrade`**，和已有的真实 Codex 测试一样默认忽略，给了两个二进制才跑：

```text
AERORIC_REAL_CODEX_OLD=<旧版 codex> AERORIC_REAL_CODEX=<新版 codex> \
  cargo test -p aeroric-core --lib real_codex_history -- --ignored
```

- **模型：** 本地的假 Responses 服务，回答“ack <用户最后一句话>”。不花额度，不碰用户的 Codex 登录和配置，`CODEX_HOME` 是临时目录。
- **走的是 Aeroric 自己的驱动**（`CodexSession`），不是直接发 JSON-RPC：用户升级 Codex 后，恢复旧会话走的就是这条路（`thread/resume`）。
- **步骤：**
  1. 旧版：新建线程，跑两轮（`first ALPHA-1`、`second BETA-2`），记下驱动看到的两个回合 id 和线程 id，关掉；
  2. 新版：带着线程 id 启动，驱动发 `thread/resume`，再跑一轮（`third GAMMA-3`）；
  3. 新版直接调 `thread/read {includeTurns: true}`。
- **断言：**
  - 新版报回的线程 id 和旧版的相同，没有另开线程；
  - 第三轮发给模型的输入里有前两轮的用户消息和模型回答（上下文保留）；
  - `thread/read` 列出的回合 id 依次是旧版的两个、新版的一个，状态都是 `completed`（回合 id 保留）。
- 共用的假模型服务改成接收一个回答函数，子代理测试和升级测试各给各的。

**跑过的组合**（Mac mini，darwin-arm64 原生二进制）：

| 旧版 | 新版 | 结果 |
|---|---|---|
| 0.162.1（支持的最老版本） | 0.165.0（Mac mini 上装的） | 通过 |
| 0.165.0 | 0.169.1（npm 上最新） | 通过 |
| 0.162.1 | 0.169.1 | 通过 |

另外同一版本 0.165.0 → 0.165.0 先跑过一遍，确认测试本身能走通。

**新增协议快照 `contracts/codex/app-server-0.169.1`**，由 0.169.1 的 `codex app-server generate-json-schema` 生成，四个文件与旧快照一致。第一步的核对会自动纳入它：
- 驱动的 Codex 测试 65 个、执行器测试 20 个，发出的每条消息三份快照都收；
- `pnpm native:codex-diff 0.167.0 0.169.1` 以 0 退出，驱动在用的方法一个没少。

0.167.0 → 0.169.1 的变化：
- **请求：**
  - 多了 `account/securitySetup/read`、`modelProvider/config/write`、`modelProvider/list`；
  - `model/list` 多一个可选的 `modelProvider` 参数，驱动不发。
- **通知：**
  - 错误的 `codexErrorInfo` 多了 `inferenceHookDenied`，驱动不读这个字段；
  - `agentMessage` 条目多了 `moderation`（`moderated`、`display.action` 等），驱动不读；
  - `item/completed` 多了 `contentTokenCount`，驱动不读。

## Alternatives considered

**不经过驱动，直接用 JSON-RPC 走一遍 `thread/start` → `thread/resume`。** 最强的理由是测的只有 Codex 自己，失败时更好定位。没选：要护住的是“用户升级后在 Aeroric 里接着聊”，驱动发 `thread/resume` 时带的参数（`developerInstructions`、`config`）和读回的线程 id 都在这条路上。最后用 `thread/read` 核对回合 id，是因为驱动不保留恢复时拿到的回合列表。

**用真实模型跑。** 没选：要花额度、结果不确定，而这里要测的是会话文件的兼容，不是模型的回答。

**把 0.169.1 设为最低支持版本，删掉旧快照。** 没选：用户机器上的 Codex 不一定升级到最新；0.162.1 仍是 `version_supported` 的下限，三份快照都留着，驱动发的消息要每份都收。

## Consequences

- **收益：**
  - Codex 从支持的最老版本升级到 npm 上最新版本，旧会话在 Aeroric 里能接着聊，回合 id 不变；
  - 以后升级 Codex，按同样的命令跑一遍就知道历史还能不能用，不必等用户撞上。
- **局限：**
  - 只测了 darwin-arm64；
  - 只测了纯文本的两轮，没有工具调用、子代理、压缩过的历史；
  - 没测降级（新版写的会话交给旧版）：Aeroric 不会主动降级 Codex；
  - 测试要真实二进制，不在 `verify` 里跑，升级 Codex 时手动跑。下载的两个版本留在 Mac mini 的 `~/aeroric-codex-versions/`。
- **发现、未处理：** 0.169.1 的 `agentMessage` 有了 `moderation`，驱动不读这个字段。被审核拦下的回答在 Aeroric 里显示成什么样，没有核对；要不要处理，等真遇到再定。
- **验证：** 上表三组；`codex::contract` 的 `every_snapshot_is_found_oldest_first` 加上 0.169.1；Mac mini 全量验证通过。
