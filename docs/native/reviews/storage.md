# Module review — storage (Phase 2, Task P2-02)

> **数字口径（2026-09-29 加，审计出处 `MIGRATION_PLAN.md` 补记一百七十九）**：本文件里的 case 数／用例数／`minCases` 一律是**验收当天的快照**，只作证据留档，不是今天的地板值——模块在此之后被别的 Task 扩过。要看今天的数字：现场跑 `node scripts/native/check.mjs <module>`，或读 `scripts/native/checks.json` 各 case 的 `minCases`（`inventory --check` 会核对地板与冻结清单一致）。**既不要把这里的旧数字当成回归证据，也不要为了两边对齐而去改任何一份带日期的记录。**

**Gate status：`PASS` —— P2-02 accepted（2026-09-22）。** 验收第四条要求的 `grill-me [storage]` 已真实调用：第三轮 Q14～Q26 十三个答案，全部落成 ADR-010/011/012、CONTEXT 术语、代码与门禁断言（见「门禁第三轮」）。**两处语义按门禁结论下传给 P2-03/P2-04/P2-05/P3-06**，本模块不宣称它们已生效：`revision`/内容指纹的实际强制、以及真实 `statfs`/Windows ACE 的实现。

## 审查 commit 锚点（2026-09-29 反查补齐；§4.2 要求的这个字段当时没有记）

`MIGRATION_PLAN.md` §4.2 明文要求每份评审保存七项——**tool 来源/revision、实际调用入口、审查 commit/diff hash、问答、证据、问题级别、修复后复验结果**。本轮插入本节之前全量普查 `docs/native/reviews/`：**40 位十六进制串只有 2 处（插入之后本节自己会再引入锚点，那是预期结果），且两处都是外部技能 pin**（`c55ee46073ed923f86ce59a5eb3b6d895095d1b7`＝`mattpocock/skills` 的 grill-me，见 `docs/native/reference-provenance.md`）⇒ **九份载体没有一份记过被审查仓库的 commit**。这一节用 `git log -1 --until=<日期>T23:59:59` 反查补齐，**是事后重建，不是当时的记录**。

- Task **P2-02** accepted_on＝2026-09-22 ⇒ 当天最后一个提交＝`7e1f2957d8c1c477bc45718db248bbb041ad0ab2`（7e1f295，2026-09-22，「feat(protocol): P2-01 收口——ADR-008 闭合 error detail 白名单、AD」）

**局限（必须一起读）**：反查锚点只覆盖**当时已提交**的树；acceptance 常在脏树上完成，未提交部分**无法事后重建**，所以 §4.2 要的「diff hash」这一格今天仍然是空的。缺口已登记为 `G31`：今后的每一次 `grill-me` 必须**当场**写 `git rev-parse HEAD` ＋ `git status --porcelain` 的文件数与路径摘要（`docs/native/reviews/*` 的内容不被任何 case 解析，机器无法事后追补）。本文撰写时 HEAD＝`797e06c3c35ce9b28b53be3f0149894e6e038a8a`（797e06c 2026-09-28 11:04:54 +0800）。

## 交付物（Action 1–5 对应关系）

| Action | 落点 | 行数 |
| --- | --- | --- |
| 1 `paths/context/clock/storage_lock` | `core/crates/aeroric-core/src/{paths,clock,context,storage_lock}.rs` | 270 / 93 / 250 / 345 |
| 1 `storage/{mod,atomic,compat,projects,tasks,settings}.rs` | `core/crates/aeroric-core/src/storage/`（另含 Action 4 的 `runtime.rs`） | 75 / 495 / 191 / 88 / 125 / 82 / 250 |
| 2 默认路径不变、`--data-dir` 只搬管理数据 | `AppPaths{user_home,data_dir}` 两个根 + `data_dir_override_moves_managed_state_only` | — |
| 3 原子写 / 私有权限 / fsync / 失败清理 / 不只用内存 Mutex | `storage/atomic.rs` + `storage_lock.rs`（`File::try_lock`，零新依赖） | — |
| 4 v1 继续兼容既有 JSON、未知字段与备份、runtime metadata | `storage/{compat,projects,tasks,settings,runtime}.rs` | — |
| 5 `tests/storage_compat.rs`、`tests/storage_lock.rs`、src-tauri path 依赖 + 薄适配 | `core/.../tests/`（333 / 203 行）、`src-tauri/{Cargo.toml,src/core_bridge.rs}`（250 行） | — |

`Record` 保持 `#[serde(transparent)]`：核心不认识 `Project`/`Task` 结构体，是**故意的**——旧 `src-tauri/src/storage.rs:96-99` 的注释记录过「只存在于前端的字段被 serde 静默丢弃」这类事故，重新声明一遍等于再制造一次。

## 证据（全部为实测输出，非推断）

- `pnpm native:check storage` → **5 case／65 用例**，exit 0：`storage-core-rust 65/65`（lib 53 + compat 8 + lock 4）、`storage-core-clippy`、`core-discovers-no-real-home`、`lease-refusal-never-kills-a-foreign-pid`、`legacy-client-ignores-runtime-metadata`。
- 回归：`pnpm native:check skeleton` 20／113、`pnpm native:check protocol` 5／**88**（第三轮把三方映射断言加了进去：rust 49 / swift 19 / ts 20，三处地板与 `minCases` 同编辑抬齐）、`pnpm native:doctor` 18/18、`git diff --check` 干净、`cargo fmt --check`（core）exit 0、`cargo clippy --all-targets -D warnings`（core）exit 0。
- 旧客户端未破：`cargo test --manifest-path src-tauri/Cargo.toml --locked --lib` → **2412 passed / 0 failed**（HEAD 基线 2408 + 本任务 4 个桥接用例），`--test-threads=1` 与并发各跑均绿。
- **真实用户目录未被本任务的任何命令触碰**——这条被测量了两次，第二次的口径更严格，因为第一次的写法经不起追问：
  - 粗测（42 秒窗口）：门禁前后对 `~/.aeroric` 的 `projects.json`/`settings.json` 做 sha256+mtime、对 6 个 `projects/*/tasks.json` 做 (name,size,mtime) 摘要 → 零漂移。
  - **发现混淆并纠正**：几小时后同一份对照出现 `projects.json`（同尺寸、不同 sha）与 `settings.json`（+549 B）的漂移。原因不是测试——`/Applications/Aeroric.app`（pid 89834）**正在本机运行并写自己的 `~/.aeroric`**。因此"零漂移"必须写成"围着命令跑的那几秒"，而不是一句对全天的断言。
  - 严格重测：紧挨着连跑两次 `pnpm native:check storage`（第二次 exit 0），前后各取一次快照 → **drift = {}**；`runtime.json` 与 `.write.lock` 两次快照都不存在（只有 core 会创建它们）；`projects/*/tasks.json` 摘要一致。证据：`.artifacts/native/real-home-gate-window.json`（含窗口、退出码与混淆说明），旧文件保留为 `.artifacts/native/real-home-unchanged.json`。
  - 结构性佐证比时间戳更硬：core 里**不存在**任何 HOME/home_dir/USERPROFILE 发现路径（门禁 `core-discovers-no-real-home` 强制），所有路径由调用方注入，测试一律用 `std::env::temp_dir()`；`/tmp` 里没有 `aeroric-{compat,core,atomic,lease,ctx,legacy-dto,core-bridge}-*` 残留（0 个）。
- 跨进程而非跨线程：`tests/storage_lock.rs` 用 test 二进制 `--ignored` 把自己再 spawn 一次（`#[ignore]` 的那个用例就是持有者子进程，ready marker 里写的是它自己的 epoch，所以 marker 本身就是证据）。POSIX 记录锁在同一进程的两个 handle 之间不冲突，线程测试会假绿。

## 三条不可交换的实现规则

1. **活性来自 OS 锁，不来自 pid**：`File::try_lock` 失败即 `AlreadyInUse`；`.write.lock` 里那份 `HolderInfo` 只是报告（写失败也不撤销已拿到的锁），删除一个死持有者的文件不等于释放锁。
2. **`corrupt ≠ absent`**：`ReadOutcome{Found,Absent}` + `StorageError::Corrupt`。旧 `app_settings/load_save.rs:442-458` 把两者混为「用默认值」，随后任何一次保存就把用户唯一的配置覆盖掉——新层拒写不覆盖，且拒绝时连 `.bak` 都不留。
3. **拒绝不是击杀授权**：`AlreadyInUse` 携带持有者与「never kill」提示；`src/` 里出现任何 kill 调用会被门禁抓住（见下）。

## 反例验证（14 次改坏，每次只改一处）

| 改坏 | 应红 | 实测 |
| --- | --- | --- |
| `read_array` 的非 object 元素分支改为静默塞进空 `Record` | `a_non_array_document_is_refused...` | 红：`string element: [Record({})]` |
| `file.try_lock()` 的失败被忽略 | `a_second_process_cannot_write_the_same_directory` | 红：`.expect("another live process holds this directory")` |
| `canonical_decimal` 丢掉「规范十进制」检查 | `a_non_canonical_revision_is_a_refusal_not_a_reset` | 红（`"007"` 被读成 7） |
| `schema_version > RUNTIME_SCHEMA_VERSION` 判断被短路 | `a_newer_schema_is_refused_and_left_untouched` | 红，且红字显示它真的把 schema 2 改写成 1（正是拒绝降级的理由） |
| `paths.rs` 里注入 `std::env::var("HOME")` | `core-discovers-no-real-home` | 门禁 FAIL（forbidden matches present） |
| `paths.rs` 里注入 `Command::new("kill")` + `.kill()` | `lease-refusal-never-kills-a-foreign-pid` | 门禁 FAIL，同一轮两个 absent case 各自独立变红 |
| src-tauri 侧只留 legacy/新 reader 的其中一侧 | `the_legacy_dto_and_the_core_reader_round_trip_the_same_file` | 该用例断言 core 存回的字节与旧 DTO 写出的**逐字节相同**，任一侧改格式即红 |
| 临时把 src-tauri 三个文件退回 HEAD | 全量 lib 套件 | 2408 passed —— 用来判定下面的 remote 测试抖动归属 |
| `writer_eligible()` 改成永远 `Ok` | location + storage_lock 的 4 条拒绝用例 | 红（非本地卷、Unknown、无权限回执、云同步根各一条） |
| `verify_still_held()` 开头短路成 `Ok(())` | `a_replaced_lock_file_stops_the_writer` | 红（与前一行同批跑，共 8 条精确命中） |
| `runtime.json` 注入到 src-tauri 的读取 | `legacy-client-ignores-runtime-metadata` | 门禁 FAIL |
| TS 侧把 `locationUnfit` 的码改成 `NOT_FOUND` | `publishes the same three storage refusals...` | 红（deep-equal 失败）——证明"三方读同一份"不是装饰 |
| 在 `location.rs` 测试里写 `/Users/me/...` 夹具路径 | `core-discovers-no-real-home` | 门禁 FAIL —— 规则连自己新写的夹具都不放过，改判中性路径后复绿 |
| 给 `locationUnfit` 声明一个白名单外的 detail 键（`absolutePath`） | 契约生成器 | panic 并点名该键 "not in the ADR-008 allowlist"，`contracts.mjs --check` exit 1 —— ADR-008 的闭合由生成器把守，不靠评审眼力 |

变异一律用 `cp` 备份 + `cp` 还原，`diff` 确认逐字节回到原状；未使用 `git checkout/stash/reset`。

## Acceptance 第二条四类反例的证据位置（2026-09-29 审计补，见 MIGRATION_PLAN.md 补记一百七十八）

验收原文要求「symlink、权限错误、原子写中断、磁盘写失败有反例」。逐类回溯到具体测试名，是因为上面的变异表只覆盖了四类中的三类——**symlink 在这一份评审里一个字都没出现**（`grep -ci symlink docs/native/reviews/storage.md` 补记前为 0），不是缺测试，是缺记录：

- **symlink**：`a_symlink_to_the_same_directory_is_the_same_lease`（`core/crates/aeroric-core/tests/storage_lock.rs:200-226`，`#[cfg(unix)]`）。它不走"改坏生产码"这条路，而是双向争锁：先用真实目录 `AppPaths::with_data_dir(&root, Some(&real))` 拿到 `DataDirectoryLease`，再用指向同一目录的 `Some(&link)` 以另一个 owner 竞争，断言 `.err()` 必须是 `LeaseError::AlreadyInUse`，expect 文案即 "following a symlink must not grant a second writer"（`:221`）。实测 `cargo test --manifest-path core/Cargo.toml --locked -p aeroric-core --test storage_lock` → exit 0，`4 passed; 1 ignored`（ignored 是持有者子进程 `lease_holder_child`），该用例在 `storage-core-rust` 的 `minCases` 计数之内。
- **权限错误 / 磁盘写失败**：上表「`writer_eligible()` 改成永远 `Ok`」那一行，对应「非本地卷、Unknown、无权限回执、云同步根各一条」的 location + storage_lock 拒绝用例；另有一条目录形态错误的 `an_unusable_directory_is_a_distinct_refusal`（`:239`，文件占住目录位，断言 `UnusableDirectory | NotWritable`）。
- **原子写中断**：上表「`verify_still_held()` 开头短路成 `Ok(())`」那一行的 `a_replaced_lock_file_stops_the_writer`。

这条补齐后，Acceptance 的每一条都能从评审文本直接跳到测试名与行号；此前的缺口属于记录不完整，不影响结论。

## 本轮发现并处理的缺陷

1. **一条用例断言的对象根本不存在**：`a_non_array_document_is_refused_rather_than_read_as_empty` 只创建了 `projects/p2/` 目录，从未写入被断言的那份坏 `tasks.json`——缺文件会被读成 `Ok(vec![])`，断言的是它没制造的状态。补齐 fixture 后用上表第 1 行证明它现在真的会红。教训：**「测试通过」不等于「测试在测这件事」**，clippy 的 `err_expect` 改写只是恰好把它顶了出来。
2. `err_expect`×4、`needless_borrows_for_generic_args`×2、`unnecessary_to_owned`、`cloned_ref_to_slice_refs` 全部按建议改掉（core `--all-targets -D warnings` exit 0）。
3. **清单工具的形态问题（记账，不改行为）**：改动 `src-tauri` 任何源码都会使 `command-map.json` 里 pinned 的 `definition_resolved` 行号失效。`--freeze` **不会重定位**它们，而是把对应 6 条的 `params` 降级为 `null`（`command_params_annotated` 659→653），`--check` 报 16 条不一致。本轮人工改正 10 处行号 + 1 处动态站点后 `--check`/`--freeze-check` 复绿，diff 只含这 11 行。待议（不得顺手做）：freeze 是否可以按 `rust_path` 唯一命中来重定位行号，并把「重定位」与「人工确认」区分记录。
4. **legacy remote 测试在负载下抖动**：`remote::network_exposure_tests::failed_invite_after_widening_restores_loopback_listener` 在本机与并发构建同跑时红、独占 CPU 或 `--test-threads=1` 或单跑该组时绿（HEAD 内容同配置也如此）。机制定位在 `src-tauri/src/remote/mod.rs:875-881`：invite 失败后 `ensure_listener_scope` 恢复 loopback 成功，但紧接着一次 `handle.is_listening()` 若在重绑完成前被观察到 `false`，代码会 `shutdown_and_wait` 并把 `running` 留成 `None`——即「QR 生成失败可能顺手关掉本机远程服务」。**这不是存储层缺陷，也不归 P2-02 修**（远程监听是 Phase 7 的地盘，且属安全敏感路径），已记为 P7-01/P7-03 的输入。本轮未弱化、未跳过、未改断言。

## 与计划的一处口径更正

续跑指令第 1 条写的是「验收要求 `pnpm native:check database` 通过」，P2-02 的验收原文是 `pnpm native:check storage`（`database` 属 P7-03）。已按原文注册并跑 `storage` 模块，同时把该句更正。

## 未决与后续（不得被读成已完成）

- **`bump_revision` 目前没有生产调用方**：它被自己的 6 个用例驱动，但 INV-03 的「revision 单调」只有在 P2-03/P2-04 的写入路径真的调用它之后才成立。本轮只交付机制，不宣称语义已生效。
- `SecretStore`/`HookProvider`/`SettingsSource` 三个端口里只有前两个有 fake（`MemorySecrets`、`NoHooks`）；`SettingsSource` **至今没有任何实现**，它现在只是给 P2-05/P3-06 预留的形状——写在这里是为了不让"端口存在"被读成"注入已完成"。真 keychain 与 hook 安装留在 adapter 侧。
- Windows 的 ACE 收紧仍待 adapter 实现（`src-tauri/src/storage.rs:284-288` 的继承 ACE 语义要有人复刻）。Q21 之后它不再是"注释里的约定"：没有回执 → `private_permissions_enforced == false` → 直接拒当 writer。`both_writers_produce_the_same_file_bytes_and_mode` 的权限部分仍只在 `#[cfg(unix)]` 下断言。
- `tests/storage_compat.rs` 的 8 个用例只覆盖 legacy 三张表的形状；`events/`、`terminal-history/`、`agent-credentials/` 的读写迁移在 P3-03/P3-02 分别验收。
- **真实探针尚未接线**：资格判定的规则与拒绝路径已在 core 实现并测过（假探针注入 `NonLocal`/`Unknown`/无回执三种答案都拒），但**没有任何 adapter 真的去 `statfs`**。在 P2-05（Tauri）与 P3-06（原生宿主）各自接上之前，任何真实进程调用 `CoreContext::acquire` 都会因 `Unknown` 被拒——这是 fail-closed 的正确表现，不是可用状态。
- **锁文件身份检查在 Windows 上只到"存在"这一档**（std 无稳定 `(dev, ino)` 等价物）。`LockIdentity::checks_inode()` 把这个差别暴露出来，Q25 决定写死不假装，而不是偷偷两端都宣称成立。
- 排期账不变：32 pending 别名与 `params.target` 路由（P3-06/P7-02）、36 pending topic 接线（P3-04）、41 条 dsh `caller_evidence`（P3-05 出口）。
- **旧 writer 未被禁用，也未并行启用两套**：src-tauri 的 `save_*` 仍是产品写入路径，core 的 lease 只在注入的临时目录里被测试取得；P2-05 才是原子移交。

## 门禁第三轮的实际调用与落点（Q14～Q26，2026-09-22；反例第 9～14 次全部发生在这一轮）

`Skill(grillme-matt:grill-with-docs)`，参数 `[storage]`。一次一问，十三个答案；每个答案都指回具体代码或工件。

| 问 | 决定 | 落点 |
| --- | --- | --- |
| Q14 网络盘/云同步根怎么办 | 分级：非本地卷拒绝，云同步根允许+告警 | ADR-010、`location.rs::writer_eligible` |
| Q15 谁去 statfs | 端口在 core、实现留在 adapter（core 保持零平台 FFI） | `StorageLocationProbe`、`classify_by_path` |
| Q16 探针答不上来 | `Unknown` = 不可写（fail-closed） | `Ineligible::VolumeUnverified` + 用例 |
| Q17 revision 无调用方怎么记账 | 机制与语义拆开：P2-02 accepted，语义作为阻塞项下传 | ADR-012 决策 3、计划 P2-03/04 验收 |
| Q18 恢复权与备份代数 | 一代 `.bak`，core 永不自动恢复 | ADR-011 决策 3（现有行为即此，未改代码） |
| Q19 旧客户端/人工恢复改过数据 | 要检测：内容指纹是覆盖前置，实现下传 P2-03/04 | ADR-012 决策 2 |
| Q20 三类拒绝映射哪个码 | `BUSY` / `IO_ERROR`+reason token / `REVISION_CONFLICT`，不扩码 | ADR-011 决策 1 |
| Q22 映射算不算契约 | 算：进 `contracts/native/v1/`，三方各一条断言 | `constants.storageRejections`、TS/Swift 断言 |
| Q23 `runtime.json` 的地位 | 核心独占、可丢弃；登记清单 + 门禁证明旧客户端不读 | ADR-012 决策 1、`legacy-client-ignores-runtime-metadata`、baseline 清单表 |
| Q21 Windows 收紧归谁 | writer 资格的一部分：无回执即不可写 | `private_permissions_enforced`、`with_permissions` |
| Q24 本轮实现还是推后 | 本轮：资格检查进 `acquire` 前置 | `acquire(..., &StorageLocation)`、22 处调用点 |
| Q25 怎么确认还持锁 | 记 `(dev, ino)`，写前重验；Windows 差异写死不假装 | `LockIdentity`、`verify_still_held`、`LeaseError::IdentityLost` |
| Q26 遗留临时文件谁清 | 只有持有 lease 的 writer 扫一次 | `DataDirectoryLease::sweep_stale_temps`（非持有者拿不到该入口） |

一处**由门禁失败反过来改掉的设计**：`writer_eligible()` 原本先查权限再查卷类型，结果 NFS 上的错误信息在讲权限——用户会去 chmod 一个不是原因的东西。改成卷优先，并留了 `a_network_volume_is_refused_before_anything_is_created` 断言消息里出现 "network volume"。同时 `StorageLocation::new()` 的权限字段不再随卷类型自动为真：非 unix 平台必须显式 `with_permissions(..., true)`，否则一律拒绝（Q21 的 fail-closed 在构造函数层面成立）。


## 回滚

全部为新增文件 + 三处 src-tauri 机械改动（`Cargo.toml` 依赖、`Cargo.lock`、`storage.rs` 一处委托、`lib.rs` 一行 `mod`）与 `checks.json` 的 `storage` 模块注册；`docs/native/command-map.json` 只有 11 行行号。没有动过任何用户数据文件，也没有改签名基线（INV-09）或前端。回滚 = 删新增文件、还原上述 5 个改动、从 `checks.json` 撤下 `storage` 模块并把 `"storage": "P2-02"` 放回 `plannedModules`。

## §4.2 七字段落点（2026-09-29 反查补齐，出处 `MIGRATION_PLAN.md` 补记一百九十二；本节只回答「每个字段落在本文哪一处、哪一格当时是空的」，不重述内容）

- **tool 来源/revision**：无独立 provenance 小节；pin `c55ee46073ed923f86ce59a5eb3b6d895095d1b7` 见上面锚点节，权威出处 `docs/native/reference-provenance.md` ⇒ **当时未记**。
- **实际调用入口**：「门禁第三轮的实际调用与落点（Q14～Q26…）」一节写了调用动作与落点文件。
- **审查 commit/diff hash**：见上面「审查 commit 锚点」一节——commit 由 `git log -1 --until` **事后反查**，diff 句柄**当时没记、今天补不回来**（缺口＝`G31`）。
- **问答**：Q14～Q26 共 13 条，集中在上面那一节。
- **证据**：「证据（全部为实测输出，非推断）」；14 次改坏的对照表在「反例验证（14 次改坏，每次只改一处）」。
- **问题级别**：**未使用级别词汇**（事后判读）：最高一档是「本轮发现并处理的缺陷」第 1 条——**一条用例断言的对象根本不存在（测试通过 ≠ 测试在测这件事）**，属**证据有效性级**而非崩溃级；「与计划的一处口径更正」为记录级。**接受人**：「未决与后续（不得被读成已完成）」列出的项仍未接受，本文不代签。
- **修复后复验**：2026-09-29 同日 `native:check storage` **5／361 exit 0**；「Acceptance 第二条四类反例的证据位置」给出四类反例今天各落在哪条用例（补记一百七十八 审计补）。
