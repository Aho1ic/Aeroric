# Agent Note: E9 第一步：驱动发给 Codex 的每条消息按协议快照核对

Status: implemented

## Problem

[1118 调研](../../proposed/architecture/2026-11-19-new-addition-1118-research.md) E9 是 Codex 升级纪律，分两步：
1. 对比新旧版本的请求方法表；
2. 用真实的旧版二进制生成历史，再用新版恢复，即“历史升级测试”。

第二步要真实二进制，在“测试 VM 暂停、不碰真实 Agent 配置”期间不能跑。第一步只用仓库里的快照就能做。

动手前（`main@8ef73abd`）核对的事实：
- `contracts/codex/` 有两份快照：`app-server-0.162.1` 是支持的最老版本，`app-server-0.167.0` 是核对过的最新版本。每份都是 `codex app-server generate-json-schema` 的四个文件。
- 已有的 `codex_methods_exist_in_the_snapshot` 只查方法名在不在：
  - 驱动调用的 10 个请求、要回答的 3 个请求、要听的 9 个通知；
  - 两份快照的目录名写死在测试里。
- 驱动发出的参数（`thread/start` 的 `sandbox`、`turn/start` 的 `input` 和 `sandboxPolicy` 等）是在 `driver.rs` 里就地拼的，没有任何检查对着快照核对字段名、必填项和取值。
  - Codex 不认识的字段会直接忽略，所以字段一改名，驱动发的值就悄悄丢了，测试照样通过。

## Decision

**一、假 Codex 收到的每条消息都按每份快照核对。** 新的测试支持模块 `sessions::live::codex::contract` 负责这件事：
- **找快照：** 读 `contracts/codex/` 下所有 `app-server-<版本>` 目录，按版本从老到新排。以后升级时新建一个目录，不用改任何测试就会被核对。
- **校验器：** 支持这些快照用到的 JSON Schema 子集：
  - `$ref`、`anyOf` / `oneOf` / `allOf`；
  - `type`、`enum`、`const`；
  - `properties`、`required`、`additionalProperties`、`items`。
- **比 JSON Schema 严一处，有意为之：** `properties` 没列出的字段算错，除非 `additionalProperties` 放行（例如 `config`）。这正是为了抓改名后被悄悄丢掉的参数。
- **字段写在分支旁边时：** 0.167.0 把音频输入的 `type` 写在 `anyOf` 旁边，`url` 和 `fileId` 写在两个分支里。分支只核对自己的字段，旁边的字段由外层核对，两边都不把对方的字段当成不认识。
- **核对哪些消息：**
  - 驱动发的请求：方法要在 `ClientRequest.json` 里，参数按该方法的 `params` 核对；
  - 驱动对 `item/commandExecution/requestApproval` 的回答：`decision` 按 `CommandExecutionApprovalDecision` 核对。
  - 其余回答和客户端通知（`initialized`）在快照里没有结构，不核对。
- **接入方式：** 测试用的假 Codex（`fake::Peer`）每收到一条消息就调 `assert_fits`。这条消息不合哪份快照，就当场失败，写明快照名、字段路径和原因。
  - Codex 驱动测试、模型目录测试、执行器测试都用这个假 Codex，所以都顺带成了协议核对。
  - 原来没有测试走到 `/compact` 和 `/review`，补了一个。
- `codex_methods_exist_in_the_snapshot` 改为遍历找到的所有快照。

**二、升级用的对比报告：`pnpm native:codex-diff [旧版本 新版本]`。**
- 不给参数时，对比最新的两份。
- **方法表：** 按三个文件列出新增和删除的方法。
- **参数变化：** 对驱动在用的方法，列出参数字段的增、删、改，以及必填项的变化。
  - “驱动在用”指驱动的非测试源码里出现过的方法名字符串。
- **退出码：** 驱动在用的方法在新版里没有了，就以 1 退出。
- **定位：** 这份报告是给做升级的人看的。真正拦住不兼容的，是上面那道 Rust 核对。

0.162.1 → 0.167.0 的报告：
- **请求：**
  - 多了 `feedback/save`；
  - `initialize` 的能力里多了 `requiredHumanApproval`；
  - `review/start` 的 `delivery` 只剩 `inline`，驱动不发这个字段；
  - `turn/start` 多了 `toolOutput.retain`；
  - 音频输入可以用 `fileId`，驱动只发文本输入。
- **通知：**
  - 多了 `thread/readState/changed` 和 `thread/section/changed`；
  - 错误的 `codexErrorInfo` 多了 `modelNotFound`、`policyCapacityUnavailable`，驱动不读这个字段；
  - 图片生成条目的结构变了，驱动也不读。
- **结论：** 驱动在用的方法一个没少，驱动发出的消息两份快照都收。

## Alternatives considered

**引入完整的 JSON Schema 校验库。** 没选：
- 要多一个依赖，而这些快照只用到很小的子集；
- 标准语义下多出来的字段是允许的，抓不到改名。

**只在一个专门的测试里走一遍流程、收集请求再核对。** 没选：只能覆盖这一个流程。挂在假 Codex 上，现有的几十个测试走到的每条路径都核对，以后新加的测试也自动核对。

**把对比报告做成 Rust 测试的输出。** 没选：升级时要看的是两份快照的差别，跟跑测试是两件事，报告单独一个命令更顺手。

## Consequences

- **收益：**
  - 新建一份快照目录，驱动测试就会核对驱动发出的每条消息新版收不收；
  - 字段改名、新增必填项、取值不再被接受，都会在测试里点名。
- **局限：**
  - 只核对驱动发出的消息。驱动读 Codex 的通知时取哪些字段，没有对着快照核对；对比报告会列出这些通知的结构变化，要人看。测试里扮演 Codex 的消息后来也按快照核对了，见 [补一步笔记](2026-11-23-e9-codex-stand-ins-checked.md)。
  - 除了命令审批，其余审批回答在快照里没有结构，核对不了。
  - 第二步“历史升级测试”要真实的新旧二进制，当时 NOT RUN；2026-11-27 已跑，0.162.1 → 0.165.0 → 0.169.1 都通过，见 [E9 第二步](2026-11-27-e9-codex-history-upgrade.md)。
- **验证：**
  - core 新增 5 个校验器测试：
    - 快照按版本排；
    - 合格的请求和回答不报错；
    - 改名、缺必填、取值不对、方法没了、审批回答不对，各报一条点名的错误；
    - 分支旁的字段两边都认；
    - `config` 里的任意键放行。
  - core 新增 1 个驱动测试：`/compact`、`/review` 的请求合两份快照。
  - MacBook 上：
    - Codex 驱动、模型目录、执行器的测试共 84 个，全部经过这道核对，都过；
    - 变异验证：临时把 `/compact` 发的 `threadId` 改成 `thread`，测试当场失败，报“`threadId` 必填没发”和“`thread` 这版 Codex 不认识”，之后改回；
    - `cargo fmt --check` 过。
  - 对比报告有 5 个 node 测试，都过，登记为门禁 `codex-contract-diff-unit`。
