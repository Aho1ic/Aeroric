# Module review — projects (Phase 2, Task P2-03)

> **数字口径（2026-09-29 加，审计出处 `MIGRATION_PLAN.md` 补记一百七十九）**：本文件里的 case 数／用例数／`minCases` 一律是**验收当天的快照**，只作证据留档，不是今天的地板值——模块在此之后被别的 Task 扩过。要看今天的数字：现场跑 `node scripts/native/check.mjs <module>`，或读 `scripts/native/checks.json` 各 case 的 `minCases`（`inventory --check` 会核对地板与冻结清单一致）。**既不要把这里的旧数字当成回归证据，也不要为了两边对齐而去改任何一份带日期的记录。**

**Gate status：`PASS` —— P2-03 accepted（2026-09-22）。** 验收第四条已真实执行：`grillme-matt:grill-with-docs [projects]`，Q27～Q34 八个答案全部落成 ADR-013 + 代码 + 两侧断言（见「门禁第三轮」）。**一处遗留**：分组清单与 localStorage 的合并规则按决定留给 P2-05 验收。

## 审查 commit 锚点（2026-09-29 反查补齐；§4.2 要求的这个字段当时没有记）

`MIGRATION_PLAN.md` §4.2 明文要求每份评审保存七项——**tool 来源/revision、实际调用入口、审查 commit/diff hash、问答、证据、问题级别、修复后复验结果**。本轮插入本节之前全量普查 `docs/native/reviews/`：**40 位十六进制串只有 2 处（插入之后本节自己会再引入锚点，那是预期结果），且两处都是外部技能 pin**（`c55ee46073ed923f86ce59a5eb3b6d895095d1b7`＝`mattpocock/skills` 的 grill-me，见 `docs/native/reference-provenance.md`）⇒ **九份载体没有一份记过被审查仓库的 commit**。这一节用 `git log -1 --until=<日期>T23:59:59` 反查补齐，**是事后重建，不是当时的记录**。

- Task **P2-03** accepted_on＝2026-09-22 ⇒ 当天最后一个提交＝`7e1f2957d8c1c477bc45718db248bbb041ad0ab2`（7e1f295，2026-09-22，「feat(protocol): P2-01 收口——ADR-008 闭合 error detail 白名单、AD」）

**局限（必须一起读）**：反查锚点只覆盖**当时已提交**的树；acceptance 常在脏树上完成，未提交部分**无法事后重建**，所以 §4.2 要的「diff hash」这一格今天仍然是空的。缺口已登记为 `G31`：今后的每一次 `grill-me` 必须**当场**写 `git rev-parse HEAD` ＋ `git status --porcelain` 的文件数与路径摘要（`docs/native/reviews/*` 的内容不被任何 case 解析，机器无法事后追补）。本文撰写时 HEAD＝`797e06c3c35ce9b28b53be3f0149894e6e038a8a`（797e06c 2026-09-28 11:04:54 +0800）。

## 交付物

| 落点 | 行数 | 职责 |
| --- | --- | --- |
| `core/crates/aeroric-core/src/projects/mutations.rs` | 763 | 纯函数：与 `src/projectOrder.ts`、`src/projectGroups.ts`、`src/appProjectState.ts`、`src/state/app/projectMutations.ts` 一一对应 |
| `.../projects/service.rs` | 331 | 唯一读写路径：list / rail / open(local·ssh·wsl) / touch / rename / avatar / hide / pin / group / reorder / remove + revision 记账 |
| `.../projects/mod.rs` | 25 | 两层分离的说明与导出 |
| `core/crates/aeroric-core/tests/projects.rs` | ~830 | 42 条 parity 重放 + 15 条服务层/身份/分组用例 |
| `core/crates/aeroric-protocol/knowledge/project-parity-cases.json` | 1383 | **期望值的权威存储**，由旧客户端 TypeScript 亲自算出 |
| `src/test/project-core-parity.test.ts` | 416 | 用真实 TS 函数重放知识表；重派生需要显式环境变量 |
| `contracts/native/v1/fixtures/projects.json` | — | 知识表发布成的契约产物（生成器内嵌，`contracts.mjs --check` 逐字节比对） |
| `scripts/native/checks.json` → `projects` 模块 | 4 case／**18** 用例 | 注册即同写 `minCases` 与地板；`plannedModules.projects` 已摘除 |
| `src/App.tsx`、`src/projectOrder.ts`（旧客户端） | 2 处 | ADR-013 Q29/Q30：打开项目只用一个时间戳；reorder 的重复 id 先去重 |
| `src/test/project-open-single-timestamp.test.ts` | 55 | 上面那条的守卫（源码形状检查，见「教训」） |

派生链是这条验收的全部意义：

```
真实 TS 函数 --(env AERORIC_WRITE_PROJECT_EXPECTATIONS=1)--> 知识表 --(include_str!)--> 生成器 --> fixtures/projects.json
      |                                                        |                                  |
      +-- projects-parity-is-derived-from-client（TS 侧断言） ---+                                  +-- projects-core-rust（Rust 侧重放）
```

任何一环自己变了而别环没变，红的是**那两环之一**，而不是等某次人工对比发现"哦行为不一样了"。

## 证据（实测）

- `pnpm native:check projects` → **4 case／12 用例**，exit 0。
- `pnpm native:check storage` → 5 case／**75** 用例（`bump_revision` 与内容指纹的写入路径用例并进同一条 `cargo test -p aeroric-core`，两条地板不能各持一半真相）；`protocol` 5／88、`skeleton` 20／113、`doctor` 18/18、`pnpm typecheck` exit 0、`cargo clippy --all-targets -D warnings` 与 `cargo fmt --check` exit 0、`git diff --check` 干净。
- 真实用户目录：把 `~/.aeroric` 的 `projects.json`/`settings.json`（sha256+mtime+size）在门禁窗口前后各取一次 → **零漂移**，`runtime.json`/`.write.lock` 两次都不存在（`.artifacts/native/real-home-p203-window.json`）。产品 `Aeroric.app` 在本机运行，所以口径只在"窗口内"成立，不写成全天断言。
- 无 UI 耦合是**机械**检查：`projects-core-has-no-ui-coupling` 只拦代码形状（`tauri::`、`.emit(`、`window.`、`addEventListener`、`localStorage.setItem`、`useEffect`…），三条正控证明 rg 真读到该模块；注释里那句"没有 window"不会把自己判红。
- 「未跑窗口也能列出保存的项目」由 `the_rail_can_be_computed_without_any_window_state` 实测（pinned 在前、hidden 留在数据里、分组顺序确定）。

## 反例验证（P2-03 这一段 5 次）

| 改坏 | 应红 | 实测 |
| --- | --- | --- |
| Rust `merge_group_names` 直接用未归一化的 configured 列表 | `projects-core-rust` 的分组用例 | 红（**这是首轮自己抓到的移植缺陷**，见下） |
| 把 TS `normalizeProjectGroupName` 的 `trim()` 去掉 | `projects-parity-is-derived-from-client` | 红，且点名是哪条用例 |
| 知识表与产物不同步（改了 TS 行为不重派生） | TS 断言先红 | 红：`reproduces every stored expectation` 失败，重派生是显式动作 |
| 在契约夹具里使用 `/Users/me/...` 路径 | `contracts-free-of-local-state` | 门禁 FAIL（协议模块抓到项目夹具） |
| 删掉知识表任意一行而不改 `caseCount` | 两侧 | 红：Rust `caseCount == len`、TS `CASES.length == stored.length` |

一律 `cp` 备份 + `cp` 还原，`diff` 确认逐字节复原；未用 `git checkout`/`stash`/`reset`。

## 写代码时"顺手决定"过、后来交给门禁拍板的六件事

1. **本地项目身份 = `path` 字面串**。今天 `/repo/one/` 与 `/repo/one` 是两个项目，`/Repo/One` 与 `/repo/one` 也是两个（macOS 上大小写不敏感的盘尤其明显）。我已把这两种拼法各钉成 parity 用例：现状是**确定但不合并**。合并＝把用户竖条里的两条变一条，属于产品决定。
2. **`id = String(now_ms)`**。同一毫秒打开两个项目会得到同一个 id；后续 `find(|p| p.id == …)` 只会命中第一条，第二条形同覆盖。旧客户端今天就这样，我没改，但这是真实丢记录的窗口。
3. **`applyProjectOrder` 的重复 id 会复制项目**（实测钉成用例：`["a","a","b"]` → 三条记录，`a` 出现两次且 orderIndex 不同）。这是产品的既有缺陷，不是移植引入的。
4. **分组清单（`localStorage` 的 `aeroric:projectGroups`）在 core 里只是入参 `GroupConfig`，不落盘**。P2-03 没说它该归谁；不决定就会在 P3-06 变成"两边各存一份分组"。
5. **坏 `location`（kind 不认识）在 TS 里原样透传，在 core 里回落成 local**。这是我在两侧造出的**已知不一致**，测试没抓到是因为我没写这条用例——已补，需要门禁确认这是可接受的降级而不是必须报错。
6. **`rename` 只改 `name`，`renameProjectGroupInList` 返回 `groupsNext: []`**（TS 让调用方自己去刷新分组）。我逐字照抄了这个空数组，但"谁拥有分组清单"和 4 是同一个问题。

| 问 | 决定 | 落点 |
| --- | --- | --- |
| Q27 本地路径身份 | 只做无损归一（去尾分隔符、`\`≡`/`），不猜大小写、不自动合并 | `mutations::normalise_local_path` + `identity_of`；parity 用例标 `coreExpected` |
| Q28 项目 id | 新项目用调用方给的 uuidv7，旧毫秒 id 不动；写入前拒绝重复 id | `upsert_*` 新增 `new_id`；`ProjectError::DuplicateId` + `assert_unique_ids` |
| Q29 `handleOpen` 双调用 | 现在修：一个时间戳喂给两次 upsert（照 `handleOpenWslProject` 已有写法） | `src/App.tsx:378,397` + 守卫用例 |
| Q30 reorder 重复 id | 两侧都按首次出现去重 | `src/projectOrder.ts` + `mutations::apply_order`（原「会复制记录」的 parity 期望随客户端一起改） |
| Q31/Q34 分组清单 | 核心独占 `project-groups.json`；**不能**放 `settings.json`（`AppSettings` 强类型会抹掉未知键）；P2-05 之前两文件各一写入者 | `AppPaths::project_groups_file`、`stored_groups`/`write_groups`/`create_group`、门禁 `legacy-client-ignores-core-exclusive-files`、baseline 清单 |
| Q32 坏 `location` | 看得见但不可寻址：`open/touch/rename/assign_group/reorder` 拒绝并点名 kind | `mutations::is_addressable` + `ProjectError::Unaddressable` |
| Q33 坏记录能否删 | 能——删除不寻址 | `remove` 不查 addressability，`an_unaddressable_record_is_listed_and_removable_but_nothing_else` |
| Q36(追加) `groupsNext` | 不再是空数组，返回真实清单 | `Applied.groups_next` |

**教训（守卫自己也会骗人）**：`project-open-single-timestamp.test.ts` 第一版把 `Date.now()` 出现次数当判据，结果**注释里那句解释 bug 的话**让计数变成 2 —— 守卫红了但不是因为 bug。修法是剥掉注释再数，并在测试里写明"这条源码检查只证明单一时间戳，不证明对话框的其余流程"。同一类错误在 P2-02 也犯过一次（断言一个从未写入的文件）。

## 已按 P2-02 门禁下传的要求做到的部分

- `bump_revision` **真的被写入路径调用**：一次真实改动推进一个 revision（`opening_a_project_writes_once_and_moves_the_revision_exactly_one_step`）；无效改动**不消耗** revision（`a_no_op_mutation_does_not_consume_a_revision`）；被拒绝的删除同样不推进。
- 拒绝删除仍有活任务的项目，且拒绝不留副作用（`removing_a_project_with_live_tasks_is_refused_and_changes_nothing`）；"活"读的是契约里的 `TaskStatus::is_live()`，不是本地字符串表。
- 删除只删记录，不碰磁盘（`removal_keeps_the_project_directory_and_every_other_record`）。
- **仍缺**：内容指纹（ADR-012 决策 2）在 P2-04 与任务写入一起落地；P2-03 的写入靠 lease 排他，不宣称能发现旧客户端的旁路写入。

## 未决 / 排期

- 分组清单与 `localStorage` 的**合并规则**：P2-05 验收第一条（Q34 决定"两个文件、各自单一写入者，移交时定合并"）。
- 内容指纹（ADR-012 决策 2）仍按约定归 P2-04；P2-03 只有 lease 排他。
- SSH/WSL 项目记录里 `path` 与 `location` 两份拼法的**收敛**（P3-06 `params.target` 路由）；本模块只做 `identity_key` 比较，不发明第二套身份。
- 分组清单持久化归属（P3-06 宿主设置层）。
- 36 pending topic 接线（P3-04）、41 条 dsh 证据（P3-05）与本模块无关，按排期。

## 回滚

纯新增：`projects/` 两个模块 + 一个 test 文件 + 知识表 + 一个 TS 测试 + 一个产物 + `checks.json` 的 `projects` 模块与两条地板，以及 `node-builtins.d.ts` 补的两个 `node:fs` 声明。产品侧（`src/**` 运行时、`src-tauri/**`）**一行未改**：本轮只是把旧行为读成期望值，没有把写入责任移走，React 仍然拥有项目状态。回滚 = 删上述文件 + 从 `checks.json` 撤下 `projects` 并把 `"projects": "P2-03"` 放回 `plannedModules` + `contracts.mjs --write` 复原产物目录。

## §4.2 七字段落点（2026-09-29 反查补齐，出处 `MIGRATION_PLAN.md` 补记一百九十二；本节只回答「每个字段落在本文哪一处、哪一格当时是空的」，不重述内容）

- **tool 来源/revision**：无独立 provenance 小节；pin `c55ee46073ed923f86ce59a5eb3b6d895095d1b7` 见上面锚点节，权威出处 `docs/native/reference-provenance.md` ⇒ **当时未记**。
- **实际调用入口**：「写代码时"顺手决定"过、后来交给门禁拍板的六件事」一节逐条写了发问与交门禁的动作。
- **审查 commit/diff hash**：见上面「审查 commit 锚点」一节——commit 由 `git log -1 --until` **事后反查**，diff 句柄**当时没记、今天补不回来**（缺口＝`G31`）。
- **问答**：文中 Q27～Q34 与 Q36，共 9 个标号；六件事一节是问答的主体。
- **证据**：「证据（实测）」一节，另有「交付物」提供反查入口。
- **问题级别**：**未使用级别词汇**（事后判读）：最高一档是「已按 P2-02 门禁下传的要求做到的部分」里的**跨模块契约分歧级**（不阻断，但会被下游当真），无崩溃／数据丢失级；「反例验证（P2-03 这一段 5 次）」5 次全部当场翻红后修回。**接受人**：中低风险项留在「未决／排期」由后续 Task 认领，**本文不代用户接受**。
- **修复后复验**：2026-09-29 同日 `native:check projects` **5／21 exit 0**；本文自带的历次改坏—修回对账在「反例验证」一节。
