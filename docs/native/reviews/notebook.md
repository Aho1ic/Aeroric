# P7-05 评审记录 — notebook/quick notes 原生读写与内容安全（2026-10-03 收口）

## 1. 范围

P7-05 是 Phase 7 第五个 Task：notebook 纯业务上收 core（原样移动）＋宿主
re-export／44 原生服务＋Swift NotebookClient＋Notebook 十视图＋搬移既有
测试 428 条＋新契约测试 12 条。

## 2. 实现链（三刀）

- **刀一（core notebook 模块）**：`src-tauri/src/notebook/` 37 个纯业务
  文件＋20 个 import 文件（含 4 个测试文件）共 **17,457 行原样移入**
  `core/crates/aeroric-core/src/notebook/`。适配仅 4 处：snapshots → core
  `files::local_history`（API 名＋注入 now）、migrate 的 `home_dir`
  （本地 cfg helper）、apple_notes 的 `runtime::subprocess`、tests.rs 的
  local_history 路径。宿主 `notebook/mod.rs` 改为 re-export，命令体保留
  （rag/sync 留 P7-06）。core Cargo.toml 加 zip=8.6.0 / quick-xml 0.41 /
  md-5 0.11（与宿主同版本；锁文件仅 3 行新增）。
  **真发现（搬移测试当场抓回 P6-02 上收缺口两处）**：core
  `files/local_history.rs` 丢了 ①`min_interval_ms` 限流（NOTE_LAYOUT 的
  三分钟间隔——不限流则 30 条快照只覆盖二十几秒）；②同毫秒快照 `-N`
  后缀防覆盖（`create_new` 抢名＋list/prune 的 timestamp+sequence 排序）。
  已按宿主语义补齐，`same_millisecond_snapshots_do_not_overwrite_each_other`
  先红后绿（8/12 → 12/12）。
- **刀二（宿主服务＋客户端）**：`notebook_services.rs` 44 服务与 Tauri
  `notebook_*` 命令体逐一镜像（resolve → owning_vault → 业务调用）；
  `NotebookHandle`＝进程内共享 `Arc<NotebookState>`（vault allowlist＋指纹
  表，与 Tauri `.manage()` 同语义）。`NotebookClient.swift` 44 类型化调用；
  wire 结构 Codable 桥解码（`foundationObject` → JSONDecoder），与 core
  serde camelCase 对齐。
- **刀三（十视图）**：`AeroricUI/Notebook/{NotebookStore,NotebookViews}.swift`。
  Store：**path 即主键**（display title 只展示）；基线只在确认保存后前进；
  `flipTaskLine` 行级翻转且行中段 `[x]` 文本不动（前缀判定 `-`/`*`/`>`/
  数字）；重名标签大小写不敏感聚合（raw 保原样）；模板占位符**本地时区**
  展开（`{{date}}`/`{{time}}` 大小写不敏感，`{{title}}` 留到定稿——与
  React `fillDateTime`/`fillTitle` 同语义）；树懒加载合并。十视图全部落地；
  冲突弹窗复用 Editor `SaveConflictSheet`（覆盖/另存/放弃/取消，无默认
  覆盖）；TrashView 彻底删除二段确认；ImportExportWizard 失败也展示部分
  报告。

## 3. 验证实测（2026-10-03）

| 验收 | 结果 |
|---|---|
| `pnpm native:check notebook`（新注册） | 3 case / **449 用例** ok（contract 12＋搬移单测 428＋NotebookStoreTests 9；runner.floors 双写） |
| `cargo test -p aeroric-core` | 999 passed / 0 failed |
| 宿主 `cargo test --lib`（无代理环境） | 2011 passed / 0 failed（原 2465，454 条随业务入 core） |
| `swift test --package-path macos/AeroricKit` | 203 passed / 0 failed |
| `pnpm native:build` | OK（六步链＋内部→外签名＋验签 intact） |
| `smoke-headless.mjs` | rc 0 |
| `doctor` / `contracts --check` | 18/18 ／ 12 份逐字节 |
| `inventory --check` | consistent（660/660；定义 pin 重定位 48+17 处＋`--freeze` 重注 params） |
| 相邻 UI 门禁 | tasks-ui 10／files 17／editor 14 全绿 |
| core/host clippy `-D warnings`、fmt、typecheck | 干净 |

## 4. grill-me [notebook] 的落实（2026-10-03 真实调用，用户四问全答）

- **多窗口冲突**：`expectedRevision`（FileSig=mtime+hash）基线由客户端持有、
  宿主判定；`.conflict` 是正常分支——弹窗四选无默认覆盖，store 在用户决策
  前不改基线。契约测试 `a_stale_baseline_conflicts_instead_of_overwriting`
  覆盖（磁盘内容不被旧窗口覆盖＋force 才写）。
- **不可信数据边界**：ZIP `../`（`enclosed_name` + `sanitize_relative` 双闸，
  契约测试断言 vault 外无文件）、symlink 不跟随（walk 判文件类型**之前**
  处理）、解压上限（声明值+实际长度两道，Budget 总量 saturating_add 防
  绕回）、导入落点重名不覆盖（`unique_path` 加 `-2`）、HTML 永不明文执行
  （MarkdownPreview 原生 Text 渲染）。均为真实测试样本。
- **旧笔记/模板字节不变**：`migration_and_richtext_conversion_leave_
  unrelated_notes_and_templates_byte_identical`（plain 笔记/模板 hash 逐字节
  相等＋迁移幂等 skipped）。
- **用户裁决四项**（2026-10-03）：① 十视图推迟统一挂载（与 P6 同口径）✅；
  ② `attachment_read` base64 传输接受为已知边界并记账（见 §6）✅；
  ③ 本机 HTTP_PROXY 假失败只记录不改门禁（见 §6）✅；④ **接受并收口** ✅。

## 5. 验收原文逐条对照

- `tests/notebook_contract.rs`（12）＋`NotebookStoreTests.swift`（9）✅；
  `pnpm native:check notebook` ✅；**搬移后 import/archive 既有测试**＝428 条
  在 core 默认门禁中运行（原宿主 opt-in 门禁内 454 条中 454-26=428 条迁入，
  宿主地板 2439→2011 deliberate 双写）✅。
- Markdown/富文本往返、图片附件、重名标签、反向链接、任务复选框、trash
  restore、导入失败部分报告——全部有真实样本（contract 12 例逐项点名）✅。
- ZIP `../`、symlink、超大解压不写出测试 vault ✅（解压总量/单体上限由
  guards 单测覆盖边界，爆炸体构造不落盘）；旧笔记与模板内容 hash 未被无关
  格式化改变 ✅。
- `grill-me [notebook]`：已真实调用（grill-me → grilling 链；用户：接受并
  收口）✅。

## 6. 已记录的边界与挂账

- **`attachment_read` base64 传输**：JSON 通道下约 5 MB 图 → 6.7 MB 文本；
  Tauri 旧客户端走 `ipc::Response` 原始 body 不受影响。用户裁决接受为已知
  边界，协议 v1 演进（P8-03 远程面）时再议流式/二进制帧。
- **本机代理假失败**：`HTTP_PROXY=127.0.0.1:7890` 劫持宿主
  local_router/app_settings 的 localhost HTTP 测试（19 条假失败）；去代理后
  2011/0。用户裁决只记录不改 runner；CI 无代理不受影响。判别方法：
  `env -u HTTP_PROXY -u HTTPS_PROXY -u ALL_PROXY -u http_proxy … cargo test`。
- **十视图 App 挂载**：与 P6 系列同口径推迟统一接（owner＝shell 接线轮/
  P10-02）。
- rag/sync 仍在宿主（P7-06 scope）；其 `crate::notebook::{fs_ops,state,
  vault_index,trash}` 引用经 re-export 解析，P7-06 迁移时随行收口。
- 宿主 `local_history.rs` 的 `record_snapshot_in`/`force_snapshot_in`/
  `discard_history_in` 三个包装失去唯一调用方后删除（语义已在 core）；
  `definition_resolved` 48+17 处 pin 漂移按流程重定位（判据＝"该行即声明"）。
