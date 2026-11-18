# Agent Note: 宿主方法与契约对账——先修扫描，再分批回填

Status: proposed

## Problem

2026-11-17 全量审查交接单 B5-1：宿主现在注册的方法有 350 多个，没有进 `docs/native/command-map.json`，也没有进 `contracts/native/v1/**`。涉及 `session_live_services`、`imports`、`canvas`、`agents`、`agentConnections`、`builtinSkills`、`remote` 等服务。

- A5 那次已经发现：契约文件和 command-map 漏了 238 个现行方法，于是客户端方法名检查（`ClientMethodNamesTests`）改为对照 Rust 源码里的字符串。
- `envelope.schema.json` 里 `method` 的正则只认单个点号和小写服务前缀，像 `agentConnections.list` 这样的现行方法名本身就不合规。
- `ClientMethodNamesTests` 只扫 `AeroricKit`，没扫 `macos/AeroricApp/Sources` 和 iOS 伴随端，也认不出 `request(method: "...")` 这类带参数标签的写法。
- `event-topics.json` 里的 `notebook.sync_updated`、`checkpointChanged` 已经接线，还标着 pending。

这件事改的是契约体系本身（`export-schema` 的方法面钉了"532 = 641 - 109"这类常数），按用户 2026-11-17 的决定，先写方案、批了再做。

## Proposal

按风险从低到高分三步：

1. **先修扫描和正则**（不改方法面）：
   - `ClientMethodNamesTests` 加扫 `AeroricApp/Sources` 和 iOS，认出带标签的调用；Rust 侧只认真实的注册点（`register("...")`、`service_ctx!("...")`、`*_METHOD = "..."`）；
   - `envelope` 的 `method` 正则放宽为 `^[A-Za-z0-9_]+(\.[A-Za-z0-9_]+)+$`；
   - 已接线的 topic 去掉 pending。
   - 这一步会改 `envelope.schema.json`，是一次契约文件变更，但不改任何方法。
2. **生成回填清单**：写一个只读脚本，从宿主注册点导出全部方法和能力门控，与 `command-map.json` 做差，输出"缺失 / 多余 / 名字不一致"三张表，交给用户过目。
3. **分服务回填**：每个服务一批，补 `command-map.json` 行、按需补 DTO 的 schema，重新生成契约，同步 Swift 解码器和金样。`export-schema` 里钉死的计数随每批更新，并在笔记里写明新值。

## Alternatives considered

**一次性全量回填。** 最强的理由是一步到位，门禁马上就完整。没选：350 个方法里有不少是遗留别名和待退役的面，一次性回填等于把它们全部固化成契约；分批时每批都能先问"这个方法还该不该留"。

**放弃 command-map，契约只从 Rust 注册点自动生成。** 最强的理由是不会再漂移。没选：command-map 里记的能力门控、`mutating`、别名映射这些语义，注册点上没有；全部自动化要先给注册点加注解，改动更大。可以作为第 3 步之后的方向。

## Acceptance criteria

- 第 1 步：`ClientMethodNamesTests` 覆盖 App 和 iOS，并有正控；放宽后的正则能接受全部现行方法名；`pnpm native:contracts --check` 通过。
- 第 2 步：回填清单能复现；每个"缺失"都能指到宿主的注册点。
- 第 3 步：每批回填后契约检查、Swift 解码与金样测试全部通过。

## Risks

- 回填会把待退役的方法固化成契约，所以每批先问"这个方法还该不该留"。
- 放宽 `method` 正则后，原来会被拒的拼写错误也能进契约，要靠回填清单里的"名字不一致"表兜住。

## Consequences

- 需要用户批准第 1 步（会改 `envelope.schema.json`），并决定第 3 步按什么顺序回填。
- 在此之前，客户端方法名仍由 A5 那套"对照 Rust 源码"的检查兜底。
