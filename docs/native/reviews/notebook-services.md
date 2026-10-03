# P7-06 评审记录 — 笔记同步、RAG 与存储后端（2026-10-03 收口）

## 1. 范围

P7-06 是 Phase 7 末个 Task：`notebook/{sync,rag}/` 上收 core＋
`storage_backends/`（18 协议连接/5 后端/conn 仓库）＋宿主 23 个原生服务＋
Swift 五视图＋SyncStoreTests＋**G03 关闭**（sync daemon 一半）＋**2 条 v1
topic 接线**。既有内联测试 ~350 条随业务移入 core（默认门禁）。

## 2. 实现链

- **storage_backends 上收**：`storage_backend/mod.rs`（trait/能力表/18 协议
  元信息/路径守卫）＋5 后端（opendal/box/baidu/smb/mount）＋`storage_conn.rs`
  → core `storage_backends/{mod,conn,opendal,box_backend,baidu,smb,mount}.rs`。
  路径参数化：conn 读写以 `data_dir` 注入（`StorageConnRepo`，凭据分文件
  0600 语义原样）；mount 的挂载点 `<data-dir>/mounts/<id>`；
  `build_backend(data_dir, connection)`。`box` 为 Rust 关键字 → 模块名
  `box_backend`。宿主只剩 `storage_backend.rs`/`storage_conn.rs` 两个 shim
  （5 条 Tauri 命令走 repo；旧签名保留，sftp/ssh 调用点零改动）。
- **sync 上收＋G03**：`sync/{diff,engine,git,local,manifest,remote,scan,
  schedule,store}.rs` 原样移动（crate 路径适配：storage_backends、git
  transport 适配器）。**`daemon.rs` 重写**：纯逻辑 verbatim（status_for/
  forget_pending/reconcile/classify/event_is_in_scope），`run_loop` 去 berth
  AppHandle——vaults 从注入的 `NotebookState` 取、状态变化走注入的
  `SyncEventSink`；**`SyncDaemonHandle`（AtomicBool 停止位＋JoinHandle，
  `stop()` 置位并 join，Drop 即停）取代裸 `thread::spawn`——G03 的
  daemon.rs:404 半边闭合**（usage_index.rs:1048 半边留 P8-04，用户裁决按
  计划）。宿主 `start(app)` 建 guard 存 Tauri state。
- **rag 上收**：`rag/{chunk,cjk,db,embed,graph,index,rerank,search,context}.rs`
  原样移动；`commands.rs`（13 条 Tauri 命令）留宿主。`embed::client` 参数化：
  `ProxyConfig { url, username, password, no_proxy }` 随 `EmbedConfig.proxy`
  携带（serde default 兼容旧形状），core 不再读宿主 app_settings 全局。
  `SearchOptions` 补 Deserialize（camelCase default）。
- **宿主 23 个原生服务**（notebook_services.rs，`service_ctx!` 持 CoreContext）：
  sync bind/unbind/remotes/run/set_auto/resolve/resolutions/status＋git.sync；
  rag probe/stats/cancel/clear/search/neighbors（`block_rag`＝current-thread
  runtime 独线程跑，与 `on_own_thread` 同语义）；embedding_key
  set/status/clear（凭据走 `CoreContext.secrets`，status 不回明文）；
  storage list/save/delete/touch/secret_keys（数据根 = `paths.data_dir()`）。
- **2 条 topic 接线**：sync 的 sink 双发 `notebook-sync-updated`（verbatim）＋
  `notebook.sync_updated`；rag 进度 EventSink 双发 `notebook-rag-index-progress`
  （verbatim；core 常量 `RAG_INDEX_PROGRESS_LEGACY`，与 `import/report.rs`
  同名不同值的符号表教训已写进常量表注释）＋`notebook.rag_index_progress`。
  常量表落 core（`SYNC_UPDATED_TOPIC`/`RAG_INDEX_PROGRESS_TOPIC`），宿主只
  剩 emit wrapper——与 P3-05 dsh 常量迁移同先例。

## 3. 验证实测（2026-10-03）

| 验收 | 结果 |
|---|---|
| `pnpm native:check notebook-services`（新注册） | 4 case / **21 用例** ok（notebook_sync 6＋rag_pipeline 4＋storage_backends 6＋SyncStoreTests 5；runner.floors 双写） |
| `cargo test -p aeroric-core` | **1560 passed / 0 failed**（含搬移内联测试 ~550＋三个契约文件 16） |
| 宿主 `cargo test --lib`（无代理环境） | 1466 passed / 0 failed（原 2011；~545 条随 sync/rag/storage 入 core） |
| `swift test --package-path macos/AeroricKit` | 208 cases / 0 failed |
| `pnpm native:build` | OK（验签 intact） |
| smoke / doctor / contracts | rc 0 ／ 18/18 ／ 12 份逐字节 |
| `inventory --check` | consistent（定义 pin 重定位 97 处＋`--freeze`＋`--freeze-reachability`——smb 后端随迁使 create_directory 的 internal ref 归零，属真实漂移） |
| core/host clippy `-D warnings`、fmt | 干净 |
| 真向量查询 | tmp SQLite 加载 sqlite-vec（链接进二进制），vec0 MATCH 最近邻命中 |

## 4. grill-me [notebook-services] 的落实（2026-10-03 真实调用，用户四问全答）

- **凭据边界**：两文件策略原样（公开连接文件断言不含 `AKIAEXAMPLE`/`s3cr3t`
  ＋无 secrets 键；secrets 文件 0600——unix 权限断言）；embedding key 只回
  「已设置」布尔，key 经 `SecretStore`（测试 fake 可注入）；SyncStore 类型面
  无凭据字段（Swift 测试以 Mirror 断言）。
- **取消/重复计费**：rag 取消协作式（CancelToken，已完成部分留库可续）；
  sync 空库差 → 空计划、零远端写入（`an_unchanged_vault_produces_an_empty_
  plan_and_no_double_charge`）；seq 只在全部落定时推进。
- **G03**：`SyncDaemonHandle.stop()` 在 ≤MAX_TICK 内 join（契约测试）；
  宿主 guard 入 Tauri state，退出 drop 即停。usage_index.rs:1048 留 P8-04
  （用户裁决）。
- **用户裁决四项**：① 索引任务互斥＝单客户端约定（跨进程锁不加，索引派生
  可重跑）✅；② 原生代理由客户端 config.proxy 携带（宿主 Tauri 命令层维持
  app_settings 注入）✅；③ G03 按计划关一半 ✅；④ **接受并收口** ✅。

## 5. 验收原文逐条对照

- `tests/{notebook_sync,rag_pipeline,storage_backends}.rs`（6/4/6）＋
  `SyncStoreTests.swift`（5）✅；`pnpm native:check notebook-services` ✅。
- fake storage/embedding：断线（整轮 Err → daemon Failed 退避）、冲突
  （ask 挂起→决定下一轮执行）、quota/401（fail_with 注入路径与断线同型，
  真实 401 语义在 embed::EmbedError::Rejected 分层——宿主单测）、取消
  （CancelToken＋SyncDaemonHandle）、重复 index 不重复计费 ✅；tmp SQLite
  真加载 vec0 并做向量查询 ✅。
- UI 切模式/关窗后 scheduler 不重复创建：core daemon 单线程单例 guard，
  `start()` 幂等由宿主 manage 语义保证（重复 start 会替换 state——guard drop
  即停旧线程，无泄漏）✅；凭据不出现在事件快照（sync payload 空、store 无
  凭据字段）✅。
- Keychain 错误可诊断不降级明文（SecretStore Result 上抛）✅；不支持持久
  钥匙串平台的限制说明保留在宿主 secrets.rs（未动）✅。
- **grill-me [notebook-services]**：已真实调用（grill-me→grilling 链；用户：
  接受并收口）✅。
- **Phase 7 Gate（F11–F16 全部映射闭合）**：F15（notebook）/F16（sync/RAG）
  随 P7-05/P7-06 accepted 转双 accepted；F11–F14 已于 P7-01～P7-04 闭合
  ——Gate 条件满足 ✅。

## 6. 已记录的边界与挂账

- **索引任务互斥**：桌面命令层与原生服务层各自持进程内任务表；同 vault 跨
  两客户端并发建索引只在同一 host 进程双客户端时存在——单客户端约定
  （用户裁决），索引派生可重跑。
- **原生代理注入**：原生路径代理取客户端 `config.proxy`（默认直连；本机
  Ollama 不受影响）；宿主 Tauri 命令层维持 app_settings 注入（用户裁决）。
- 本机 HTTP_PROXY 会劫持 localhost 测试（LAN roundtrip/rag rerank 的
  502/连接拒绝为环境假失败；去代理全绿）——沿用 P7-05 记录的判别方法。
- embedding key 的原生服务用 `SecretStore`（MemorySecrets 在无钥匙串宿主
  时由适配器决定实现）；macOS 桌面宿主已接真钥匙串。
