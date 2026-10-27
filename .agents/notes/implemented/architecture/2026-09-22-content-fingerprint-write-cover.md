# Agent Note: 写入基线是 core 上一次发布的字节，runtime.json 可丢弃

Status: implemented

## Problem

`runtime.json` 记 `schema/epoch/revision`，计数器只在参与它的写者之间有意义：外部进程或人工把 `projects.json` 换回旧内容不会让计数器后退，所以"revision 更高"不等于"数据更新"。要判断一次写是否在覆盖别人的改动，需要一个与计数器无关的基线。

## Decision

- `runtime.json` 是 core 独占、可丢弃状态：缺失即以 `revision: 0` 重建，不影响任何用户数据；它只描述"谁在写、写到哪一步"。任何"`runtime.json` 不在就说不出话"的设计都是错的。
- 跨写者检测靠内容指纹：每个受管文件在 `runtime.json` 的 `contentFingerprints` 表里记 `(sha256, size)`。每次写只允许覆盖"core 上一次发布的字节"；磁盘内容不符时返回 `StorageError::ContentDiverged`，映射为 `REVISION_CONFLICT`（不可重试，调用方应重读）。**绝不**静默覆盖。
- core 从未发布过的文件，第一次写采纳磁盘现状并记账，之后进入受保护状态。
- 写入顺序固定为**先数据文件、后 `runtime.json`**。中途崩溃只会让表落后磁盘一代，表现为下一次写被拒，重新 load 即消除。
- revision 只负责检测同一目录内的并发覆盖，不承担"数据新旧"判断。
- 摘要用 `sha2` crate（`Fingerprint::of(bytes)` 是唯一调用点），不手写 SHA-256。
- 项目导出/导入只搬 `projects.json` 与各 `tasks.json`，不搬 `runtime.json`；导入后的第一次写是一次采纳，这是预期行为而非缺陷。

## Alternatives considered

- **以本次写之前刚读到的字节为基线**：实现最直观。否掉是因为每条写路径都先 load，"上次看到的"恒等于"现在盘上的"，检查永久失效。
- **没有基线时一律拒绝**：最严格。否掉是因为 core 面对继承来的目录时第一次写就被自己挡住。
- **手写约 70 行 SHA-256 以免增加依赖**：锁文件不变。否掉是因为这段代码位于安全边界上，自己维护标准算法得不偿失；`Fingerprint::of` 是唯一调用点，将来换实现不改变已存指纹。依赖原则因此是"倾向少依赖"而非"禁止依赖"：先问它是否已在产品依赖图里。

## Consequences

- **收益**：外部改动、人工回滚、并发写都会表现为显式冲突而不是静默覆盖；持久化失败时不会启动任何进程。
- **代价与已知上限**：人工把文件换回旧内容会被判为冲突，必须由调用方重读再改；指纹覆盖的文件集合必须与受管文件清单一致，漏一个文件就等于给它开了静默覆盖的后门。
