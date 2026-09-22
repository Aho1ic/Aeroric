# Module review — protocol (Phase 2, Task P2-01)

**Gate status: `OPEN` — 不可宣告 accepted。** 契约与三方解码器已落成并全绿（见「证据」），但 P2-01 的计划验收还差三件实事：ADR-006 的 109 条孪生收敛、ADR-007 的事件 topic 映射表、G16 剩余 **17 条方法的逐条定性**（用户在门禁中明确选择「逐条现在定性」，尚未做完）。下面每一条都写了「做到哪 / 没做到哪」，不得把未回答的问题读成已通过（计划 §5.4）。

## 交付物（已完成，均已进门禁）

| 工件 | 内容 |
| --- | --- |
| `core/crates/aeroric-protocol/src/{envelope,error,project,task,capability,session,event,terminal}.rs` | v1 DTO：帧与 `Quantity`（十进制字符串）、错误码、项目 target、任务快照与 resume 规则、会话归属与不透明内容、能力报告、终端平面（含新增 `TerminalGap`：终端重置与控制流 gap 不是一回事） |
| `core/crates/aeroric-protocol/src/bin/export-schema.rs` | 线协议的**唯一来源**：独立重算 ADR-002 命名规则、读/改分类、能力门控、G16 出处，并生成 schema 与 fixtures；不写 `--out` 就退出 78 |
| `contracts/native/v1/{envelope,methods,events}.schema.json` + `fixtures/{success,error,unknown,terminal,session,legacy-task}.json` | 发布面：641 方法（503 改 / 138 读 / 60 wsl 门控 / 19 条 `host_local_os_ui` 无线名）、38 事件、6 个 fixture |
| `scripts/native/contracts.mjs` | `--check`（默认）生成到临时目录并**逐字节**比对，永不误写仓库；`--write` 才落盘；拒绝含本机路径的产物 |
| `src/lib/nativeHostProtocol.ts` + `src/test/native-host-protocol.test.ts` | TypeScript 读侧（17 用例） |
| `macos/AeroricKit/Sources/AeroricProtocol/{ProtocolV1,ProtocolModels}.swift` + `Tests/AeroricProtocolTests/ProtocolGoldenTests.swift` | Swift 读侧（16 用例） |
| `docs/native/adr/006-prefix-twin-collapse.md`、`007-event-topic-namespace.md` | 门禁拍板记录 |

## 证据（全部为实测输出，非推断）

- `node scripts/native/check.mjs protocol` → **5 case passed，80 test cases executed**：
  `protocol-rust` 47（min 47）· `protocol-swift-golden` 16（min 16）· `protocol-ts-golden` 17（min 17）· `contracts-current` · `contracts-free-of-local-state`。
- `node scripts/native/check.mjs skeleton` → 20 case passed，**106** 用例（此前 100；`gate-runner-unit` 18→24）。
- `pnpm native:doctor` → 18/18。
- `pnpm exec tsc --noEmit` → 无输出（新 TS 模块与测试均通过类型检查）。
- `cargo clippy --locked --all-targets -- -D warnings` → exit 0；`cargo fmt --check` → 干净。
- 写者安全性：`contracts.mjs --write` 连跑两次，9 个文件 sha256 全等（幂等）；在产物尾部追加一个换行 → `--check` **exit 1** 并指到具体行（`changed v1/envelope.schema.json: line 249`），还原后 exit 0。
- `checks.json`：`protocol` 模块注册并从 `plannedModules` 移除；新增 3 条地板；`swift-package-smoke` 加 `--filter PackageSmokeTests`，使 P1 冒烟地板不被 P2 套件涨跌掩盖。

## 三方比对真正抓到的缺陷（不是「顺手改的小 bug」，都是同一条契约上的分歧）

1. **`idempotencyKey` 到了 Rust 就消失**：`Request` 缺 `rename_all = "camelCase"`，于是 `idempotencyKey` 落进 `extra`，重试去重的主键在读侧**静默失效**。`extra` 兜底未知键，所以没有任何解析错误——只有把键绑死的断言能发现（`idempotency_key_binds_to_its_field_not_to_extra`）。
2. **`can_resume` 与线上客户端的规则不一致**：原实现只看「非终态 + 工作树未丢弃」。比对 `TaskList.tsx:184 canResumeTask` 后确认，旧规则要求 `!todo && !isActiveTaskStatus && !worktreeDiscarded && hasTaskSession`（`src/taskSession.ts:41`：id 或 path 任一即算有会话）。新实现照此改写，并把整张判定表放进 `fixtures/legacy-task.json.resumeRule`，Rust/Swift/TS **共用同一张表**断言。少这一条，无 transcript 的任务会被「恢复」成一个全新会话。
3. **空白超长行的裁决三方不一致**：TS 先查字节上限，Rust/Swift 先判「全空白」。1 MiB 的空白行在 TS 是 `TOO_LARGE`、在 Rust 是 `EMPTY`——即「什么都没发生」。改为三方**先判尺寸**，并各留一条断言（`a_blank_oversized_line_is_a_transport_fault`）。这条是 Swift 测试替我抓出来的：它照抄 TS 的期望然后失败。
4. **`BytesBase64` 的严格度**：TS 首版按 `body.length`（去填充后的长度）算输出长度，`YQ==` 解成空数组；运行时自带的 `atob` 会把 `"Y WJj"` 的空格吃掉、把 `"YWJ"` 截断成看着正常的字节。三方现在共用 `fixtures/terminal.json.base64` 的 valid/invalid 向量，`atob`/`Buffer` 的宽容被写成显式反例。
5. **未知错误码丢文本**：Rust 用 `#[serde(other)]` 降解为 `Unknown`，但注释写着「原文保留供展示」——**注释在撒谎**。改成 `Other(String)` + 手写 `Serialize/Deserialize`，原文既保留也能回显；`known_name()` 的穷尽 match 反过来成了「新增码必须同步 `ALL` 与契约」的编译期绊线。
6. **fixture 里出现不存在的方法名**：`task.start` 来自计划 §2.3 的示例，但 ADR-002 推导出的真实方法是 `terminal.run_task`。fixtures 是给三方解码器**执行**的输入，教客户端发一个必然被拒的调用不可接受 → 生成器新增 `fixtures_are_sourced`：fixture 里每个 `method`/`event` 必须 (a) 在发布面上，(b) 在 `PLAN_SOURCED_EVENTS` 里注明出处，或 (c) 在 `NEGATIVE_NAMES` 里且**确实**是负例；三份名单里任何没人用的条目都算失败。这条上线时立刻抓到一个随口写的 `"event": "e"`。
7. **读/改分类的假精确**：`READ_VERBS` 原列表 12 个动词**一个命令都不匹配**（装饰性规则），而按「首 token」判读会把 `remote_git_list_branches`（域名前缀）判成改、把 `notebook_export_site_write` 判成读。改成「第一个动作 token 决定」+ `git`/`remote` 这类前缀自然被跳过 + `READ_VERB_IS_A_NOUN`（`dbx_redis_list_*` 里的 `list` 是 Redis 键类型）；分类数 138 读 / 503 改钉住，动词表死条目直接失败。
8. **能力门控差点一刀切**：62 条平台命令里有 **2 条跨平台**（`platform.get_platform_runtime_info`、`platform.build_runnable_file_command`）。若照 `category == platform_specific` 一律挂 `wsl`，macOS 上这两个必然能用的调用会被自己禁掉。规则改为按旧名是否含 `wsl` 判定，60/2 的划分与 19 条无线名都是生成期硬断言。
9. **词表完整性**：四个枚举的 `ALL` 列表是契约里发布的词表本身。每个都配「穷尽 match + 长度钉死 + 稠密无重」的断言（`every_vocabulary_lists_the_complete_enum`），加一条反向检查：发布的每个名字必须真能被 DTO 解出来，且不会退化成 `Other`（`published_names_are_the_ones_the_dto_accepts`）。
10. **计数与数组自相矛盾**：`methods.count` / `readCount + mutatingCount == count` / `events.count == 数组长度` / `capabilityGatedCount == 60` / `byService` 求和 == 641 全部逐条断言，三方各写一遍——数字对不上数组时，报错的是测试而不是读者。

## 反例验证（断言不是摆设）

按 `repo-measurement-traps` 的既有教训，改坏生产逻辑必须让测试变红：去掉 `Request` 的 `rename_all` → 1 条失败；把 `can_resume` 改回旧规则 → 1 条失败（`record("done", session).can_resume()` 为假）；已确认还原后 47 用例全绿。

## 未决与后续（P2-01 收口前必须清零）

| 编号 | 事项 | 状态 |
| --- | --- | --- |
| Q8 | §2.3 示例名 vs ADR-002 推导面谁为权威 | **已答**：产物为准，§2.3 加注（见计划补记）；生成器 `fixtures_are_sourced` 已把口径钉成机械检查 |
| Q9 | 109 条前缀孪生 | **已答并已实现（契约侧）**：`inventory.mjs` 现冻结 `params`；面 641→**532**，`twinAliases` 109 条带 `paramMapping`/`targetKind`/`requiresCapability`；**77 proven / 32 pending**（pending 的样本：`remote_lsp_hover` 带一个父方法没有的 `remote_project_path`）。**未做**：宿主 target 路由（P3-06/P7-02）与 32 条 mapping 的人工决定 |
| Q10 | 17 条既无字面调用方、也非孪生、也无 ADR-005 证据的方法 | **已答并已做完**：17 条全部逐条落成 `pending: P6-03/P7-02/P5-05/P8-01/P8-04/P7-06`（测试簇 4、WSL 2、Agent 设置 2、扩展/市场 4、更新 2、笔记同步 3）。方法面 `uncalled-no-evidence` 从 17 → **0** 并被生成器与 `inventory --check` 双向钉住；旗标数仍是 58（17+41），语义变成"到期要交代的账"而不是"来历不明" |
| Q11 | v1 事件 topic 命名 | **已答**：v1 自定 topic + 38 条旧通道映射表（ADR-007）。**未做**：`docs/native/event-topics.json` 与生成的 `event-topics.schema.json` |
| Q12 | `RpcError.detail` 的密钥投影 | **未问**：`mustNotContain` 目前只是契约里的**书面禁止**（列表已发布并三方断言），宿主是否要在写出前主动剥离这些键尚未决定；留给门禁第二轮 |
| Q13 | 本地/远程版本边界 | **未问**：`protocolVersion` 只有常量与握手字段，跨版本策略（拒绝 / 降级 / 双栈）未成文；留给门禁第二轮 |

## 回滚

全部为新增文件 + `checks.json` 注册 + 计划补记，不改旧命令、不改 Tauri 客户端行为。回滚 = 删除 `contracts/`、`core/crates/aeroric-protocol/src/*`（除 `lib.rs` 的模块声明）、两个 Swift 源与测试、TS 模块与测试，并把 `checks.json` 的 `protocol` 模块与相关地板还原；`pnpm native:check skeleton` 与 `native:doctor` 是回滚后的验证入口。

## 第二轮追加的实测（Q9/Q10 落地时新发现，2026-09-22）

1. **`params` 抽取的第一版被自己写的注释坑了一次**：生命周期 `'` 被当成引号起点，扫描器把整条签名吞掉 → 21 行 `params` 静默为空。现在的规则是"抽取失败必须写 `params: null` 并在 `--check` 里拒绝缺键"，另有 4 条形状断言（多行、限定路径 `tauri::State<'_, X>`、泛型里的逗号、目标函数名不符）钉住这几种写法。
2. **G10 与参数比对原先读 `repoRoot`**，而仓库里所有 live 抽取都读 `sourceRoot ?? repoRoot`。测试用临时树 + 真实源码根是既有约定，跟着改过来；否则"漂移"在测试里根本触发不了（改坏一个父方法参数类型，`--write` 会拒绝发布：`alias mappings moved: 76 proven / 33 pending`）。
3. **INV-02 源码扫描拦住了一句注释**：注释里写了 `AppHandle` 字面量，`inv02-core-source-scan` 就红了。选择改注释而不是放宽扫描，并在注释里写明为什么用描述性说法——放宽一次，下一个人就会以为可以放宽第二次。
4. **能力门控的判定键变了**：60 条 wsl 门控里有 39 条本身是孪生、其父方法是 `domain_rpc`。收敛后方法面只剩 21 条 `wsl` 门控，其余 39 条随别名走 `requiresCapability`。若继续按方法门控，`fs.read_dir_entries` 会在 macOS 上被整体禁掉。
5. **`surfaceProvenance` 现在是四组之和**：caller-literal 450 + stated-evidence 82 + uncalled-no-evidence 0 = 532 方法，另有 109 条别名；`prefixDerivedAliases` 单独成项，三方各断言一次。
