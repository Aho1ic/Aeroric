# ADR-011：存储层三类拒绝的协议映射，以及"恢复"永远不是自动的

- 状态：**accepted（用户在 P2-02 门禁第三轮 Q18/Q20/Q22 拍板，2026-09-22）**
- 日期：2026-09-22
- 关联：ADR-010（资格）、ADR-008（`detail` 白名单闭合）、ADR-009（v1 内不扩码，兼容性只走具名能力）、`contracts/native/v1/envelope.schema.json`、`docs/native/reviews/storage.md`

## 背景：拒绝已经存在，但它对外长什么样还没定

`storage_lock` / `storage::atomic` / `storage::runtime` 现在会给出三类互不相同的拒绝：目录被别的活进程持有、这个位置不该当 writer（含探针不确定的 `Unknown`）、要覆盖的内容不是自己上次写下的那一份（revision / 落后写入）。三者对用户意味着三种**不同的修法**——关掉另一个 Aeroric、把数据目录搬回本地盘、重新读取后再改——所以"随便回一个 `IO_ERROR`"会把三种修法压成一条无信息的报错。同时，坏 JSON 的恢复路径今天靠"人"：core 拒绝写、原件逐字节保留、每次替换只留一代 `.bak`；而**产品今天没有任何整目录回滚功能**（`~/.aeroric/backups` 无代码读写，`FileSnapshot` 只快照 agent home 的配置文件，不含 `projects.json`/`settings.json`）。

## 决策

1. **映射到三个已有码，不扩码**（v1 加错误码是协议变更，ADR-009 已定规则）：
   | 拒绝 | 码 | detail（ADR-008 白名单内） |
   | --- | --- | --- |
   | 另一进程持有目录 | `BUSY` | `lockHolder`、`epoch` |
   | 位置不具备 writer 资格（含 `Unknown`） | `IO_ERROR` | `reason`（固定 token：`storage-location-not-writer-eligible`） |
   | 落后写入 / 内容不是自己写的那份 | `REVISION_CONFLICT` | `expectedRevision`、`actualRevision`、`epoch` |
   位置不可写**不**用 `UNSUPPORTED_CAPABILITY`（那是 host 级事实，同一个 host 换一个 `--data-dir` 结论就变）、也**不**用 `INVALID_ARGUMENT`（默认 Home 就在 NFS 上时，用户没有传错任何参数）。
2. **映射本身进契约（Q22）**：`contracts/native/v1/` 新增固定块（`storageRejections`），由 Rust 侧从枚举**算出来**写盘，TS 与 Swift 各加一条断言读同一块。理由与 ADR-006/007/008 相同：客户端要渲染的就是这三条文案，一张只给人看的表在三种语言里各抄一遍必漏。
3. **一代 `.bak` 为上限，core 永不自动恢复（Q18）**：坏文件的处置是"拒绝写 + 原件保留 + 把路径与原因讲清楚"，覆盖它必须是一次显式的、由人发起的写（今天的测试正是这么做的）。不做恢复命令、不做多代备份环形、不做"检测到坏就试上一个"。理由：这一层没有任何办法区分"这份数据坏了"与"这份数据不是我期待的形状"，而自动选择覆盖的代价是用户唯一的副本。
4. **坏内容的路径经 `message` 传达，不为它扩 `detail` 白名单**：白名单的意义就是闭合，为便利性开一个 `path` 键等于承认"名单可以按需要长"。路径是人读的信息，不是程序分支的输入。

## 后果

- 客户端必须能区分"重试可能成功"（`BUSY`）与"重试永远不会成功"（资格、冲突），否则拒绝就退化成 loading。
- 未来若真需要恢复命令（P3-06 之后），它需要显式确认参数并带反例，而不是给 `write_atomic` 加一个 `force: bool`。
