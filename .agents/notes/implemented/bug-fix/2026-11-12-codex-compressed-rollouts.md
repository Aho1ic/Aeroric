# Agent Note: Codex 压缩旧会话记录后，Aeroric 照样能读

Status: implemented

## Problem

Codex 会把不再写入的会话记录压缩成 `rollout-….jsonl.zst`，再删掉原来的 `.jsonl`。2026-11-11 21:27 前后，用户 Mac 上的一批旧记录被这样压缩了。

Aeroric 的任务记录里存的是会话运行时 Codex 给的 `.jsonl` 路径，读的时候按这个路径打开，结果是：

- 会话页的历史是空的（Codex 目检 R5-1 时 undo-demo 会话 20:47 还能看到 3 轮，21:26 再看就空了）；
- 回合账单、"产品失败 · 已撤销"这些按会话记录算的标记一起消失；
- 导入列表里这些会话不见了，发现只认 `rollout-*.jsonl`；
- 换模型接着跑时复制会话记录失败，退回交接文件。

## Decision

- **一个入口**：core 新增 `sessions::transcript_file`，所有读 Agent 会话记录的地方都走它：
  - `resolve(path)`：先找记录的路径，找不到就找旁边的 `path.zst`；
  - `open` / `read_to_string`：压缩的用 zstd 边读边解；
  - `seekable` / `tail`：分页和尾部统计需要按字节偏移读，压缩的先整份解到内存再读。压缩的都是 Codex 已经写完的会话，不会再变。
  - `copy_plain`：换模型接着跑时，复制出来的是解压后的普通 `.jsonl`，新的 Harness 要往里追加。
- **磁盘上什么都不改**：不解压回去，不改任务记录里的路径，不碰 Codex 的文件。
- **路径还用原名**：
  - 任务记录、导入索引的 id 和来源键、发现结果，都还是 `.jsonl` 的名字，旧数据不用迁移。
  - 发现时 `rollout-x.jsonl.zst` 按 `rollout-x.jsonl` 列出；两个都在时只列一次。
  - 宿主收到客户端传来的 `sessionPath` 时，用 `canonical_session_path` 找到实际文件，再按原名交给 core。
- **接入的地方**：
  - core：`history`（分页、尾部）、`resume`（id、能否原生恢复、时间）、`import`（读全文、读开头、大小、变更标记）、`discovery`、`export`；
  - 宿主：`session.read_session_id`、`session.session_supports_native_resume`、`session.read_session_message_page`、会话历史、尾部统计、重试和改动代价、上下文用量、换模型接着跑、重跑比较。
- **不支持**：远端（SSH）主机上的 `.zst`。远端读会话走另一条路，等真遇到再做。

## Alternatives considered

- **发现压缩后就解压回 `.jsonl`**：之后所有代码都不用改。没选：这是在改 Codex 自己的目录，Codex 下次整理时会不会又压回去、会不会把解出来的当成新会话，都说不准。
- **把任务记录里的路径改成 `.zst`**：一次迁移，读的地方只要会解压。没选：Codex 什么时候压缩由它自己决定，迁移完还会有新压缩的；而且导入索引的 id 是按路径算的，改路径会让同一个会话在导入列表里变成新的一条。

## Consequences

- **收益**：被压缩的会话和以前一样能看历史、算账单、导入、导出、换模型接着跑、重跑比较。
- **代价**：
  - 压缩的会话每次分页、算尾部都要整份解压一次。Codex 只压缩写完的旧会话，通常不大，目前没做缓存。
  - 导入时判断"会话太大，只读尾部"用的是压缩后的大小，解压后可能远大于 32 MB 的门槛，这种会话会整份读进内存。
- **验证**：
  - core 单测：压缩的会话按原名能读到全文、尾部、逐行，能复制成普通文件；找不到时报 NotFound；发现时压缩的按原名列出，两个都在时只列一次。
  - 目检：undo-demo 会话（记录已被压缩）重新显示 3 轮历史和"产品失败 · 已撤销"。
