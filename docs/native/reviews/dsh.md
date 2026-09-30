# tauri → native 迁移评审记录:dsh 结构化宿主(Task P3-05)

> **数字口径**:本文件里的 case 数／用例数一律是**验收当天的实测**，之后模块被别的 Task 扩过时以现场 `node scripts/native/check.mjs <module>` 与 `checks.json` 的 `minCases` 为准；既不当回归证据复用，也不为对齐改带日期的记录（口径原文见 `reviews/agents.md` 顶部横幅）。

## §4.2 七字段

- **tool 来源/revision**：`mattpocock/skills` @ `c55ee46073ed923f86ce59a5eb3b6d895095d1b7`——`grill-me/SKILL.md` 是转交 `grilling` 的入口（委托链见 `docs/native/tooling.md` §2/§3）。本会话的 `skill://grilling` 解析失败（技能清单在册、设备 404），按 sessions 轮同族处理：**grill 轮以四题决策包（D1-D4，补记二百六十三）＋ask 工具真实答复（补记二百六十四）＋收尾矩阵实测（本文件 §2）完成**，工具不可用不折算成「未评审」。
- **实际调用入口**：① `ask`（三题一批的真实答复轮：D1 transport／D2 端口簇／D3 G17 形态，2026-09-30，答复入账补记二百六十四，D1 否掉推荐项的案底写全）；② 本会话的收尾矩阵批跑（§2 表，当日退出码逐条单读）；③ 五轴自查（§1，每条事实当场可复验）。
- **审查 commit/diff hash**（G31 新规矩，**当场记**）：`git rev-parse HEAD`＝`bf214fe790dd95bf1846e4a84a38861f52487ebb`（bf214fe，2026-09-30）；`git status --porcelain`＝1 条目（`?? .zcodeignore`，用户文件、非本任务、不入 diff）；`git diff | git hash-object --stdin`＝`e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`（＝空 diff：五片实现（4769654 方法群／05b87db kernel 契约／00247da 接线／36a1ee3 fake server／06c7a1a typed 方法）＋两处门禁修复（2bb7bea/cb885c0/1c23d63）全部先 commit 再评审，审查树即最终树）。porcelain 与 diff hash 并列读（空 diff 不含未跟踪件）。
- **问答**：D1-D4（补记二百六十三决策包→二百六十四答复）＋§1 frontier 三条（Q83-Q85）。
- **证据**：§1 五轴的当日读数与测试名；§2 验收对照表；补记二百六十六～二百七十二的逐片实测段。
- **问题级别**：**两处中风险发现，当场修复**——① contracts 扫描清单漏登 approvals.rs（第八片埋、266 起暴露为 emitters 假零＋orphan 翻转，扫描器瞎而非契约变，修复后 `--write` 与 committed 逐字节相同）；② TS 快照测试读壳（252 埋、潜伏 9 小时，6/9 红）。两者同根：**切片验证的「受影响模块」判断漏面**——升格为纪律：宿主/清单/契约形状有变的片，收尾批必含 `contracts --check`＋`pnpm test`，或显式记「未跑面＋理由」（本条随 Q6/Q20 同批进 P10-05 门禁评审）。零缺陷级残留。
- **修复后复验**：本文写毕前当日矩阵全绿（§2 表），全部为当日实测，无引用旧读数。

## 1. 收尾拷问轮（2026-09-30，五轴）

**轮次输入**：五片全部落地——方法群（`dsh_host/{api,remote_mux,dto}.rs`，61 方法＋991 行 mux＋27 类型，宿主 4182→3074，补记二百六十六）、kernel 契约（`ports.rs` 16 端口＋`executor.rs` 编排 verbatim 上收，补记二百六十七）、kernel 接线（`run_dsh_task_core` 删除、legacy dsh 分支分派 core、`OutputSink` 全链改型，补记二百六十八）、fake server 端到端（`dsh_approvals` 6＋`dsh_reconnect` 5，补记二百六十九）、typed 错误面与验收行 2（`ActivateError` 四变体＋`take_waterfall_answer` headless，补记二百七十/二百七十二）。D1-D4 产品决策已由用户答复（二百六十四）。

**五轴自查结论（事实，非问题——逐条当场可复验）**：

1. **唯一 owner**：dsh 的 transport（HTTP RPC＋`/api/remote.mux` WS）、方法群、dto 线形状、启动编排、审批本地判定在 core `aeroric_core::agents::dsh_host`（11 文件）；**决策面留在宿主的**各有具名理由——六张状态 map＋`ActiveDshSession`（端口簇 D2 的宿主地盘）、stream 消费环 `consume_task_session_stream`（projection/emit/审批分发耦合 AppHandle，Q84 具名）、`ensure_dsh_webui`（进程 spawn/端口/config 写）、api 路由 fallback 链（读活跃表）、command 层 66 wrapper（tauri 入口）。嵌套方向单一：宿主 impl `DshHostPorts`→core 编排调用，core 不经端口回调宿主具体类型（`OutputSink` 擦除 `ipc::Channel`）。并发/停机序：`lifecycle_lock` 端口交 `Arc<tokio::sync::Mutex>`，guard 由 core 编排持有跨 `.await`；`retire_runs` 端口承接 ADR-025 任务级结算，失败路径与原文逐字同序（retire→failed(reason)）。
2. **反例**：fake server 端到端 11 条（wire 契约含 requestId 铸造/permission null 结算含 expect→clear-ic 顺序断言/斜杠 CRLF 包边/空 prompt 不经 pending/图片拒绝在请求前/ensure 短路不进结算块/续跑绝不 truncate/provider 强度门口丢弃/目录组解析与回退/effort 白名单门）＋headless 审批 3 条（回答即消费/重放不复生/坏输入不吃等待态）＋`dsh_host.rs` 26 条（registry take-and-consume/waterfall 判定/URL 闸门/echo 机）；形状反例由 `inv02-core-source-scan`＋`core-discovers-no-real-home` 承担且**当场变异实证过**（doc 注释里的 `AppHandle` 字面名与 `/Users/me` 假值都被扫红过——禁串不区分注释与假值，两处案底入二百六十八/二百六十六）。
3. **边界**：网络出口＝D1 用户显式批准（feature 集与宿主逐字一致，不造第二套传输；审计面扩大记全案底）；typed 错误＝`ActivateError` 四变体 Display 逐字透传（宿主 2452 条整体绿＝文案契约未动的证明）；不可信数据＝fake server 钉的 null/缺字段/未识别形状全部拒绝或回退（`is_null` 拒/目录查不到回退 current.provider/白名单外 effort 抹掉）；重放＝consume-once 双钉（registry 251＋headless 2）；token/日志＝本片零新增。
4. **回归**：旧 Tauri 客户端＝宿主 `--lib` 隔离 **2452/0**（13 条随迁 −0 新增，两侧对账严格相等；二进制 mtime 核验后测得）；前端/契约＝`contracts --check` 12 份逐字节、`pnpm typecheck` rc 0、全量 `pnpm test` **376 files/5483 tests rc 0**（含被修复的 snapshot 9/9）；mux 逃逸口＝`invoke_dsh_remote` 前端直连原样（D3 混合的 gateway 侧＝现状，零改动）；协议 v1 与存储＝零方法零格式变更（freeze 重切后 `inventory-freeze-is-noop` 绿）。
5. **回滚**：五片各一个 feat 提交＋随批件（lock/floor/锚点/extraction 同批），逐片可独立 revert；无落盘格式变化、无新建用户数据、不改权限；`ActiveDshSession.on_output` 改型是宿主内部字段（无外部持久化面）；回滚不涉及 checks.json 之外的面（floor 双写随批 revert）。**已修断点的回归风险＝零**（两处修复都收敛回原语义：contracts 逐字节相同、TS 测试读回真身后 9/9）。

**frontier 三条（产品决策，按提问轮格式；本轮用户不在场，处理照 P3-02/P3-04 先例＝按推荐项记账、异议可重开，不当作用户答复）**：

- ❓ **Q83** - **`dsh` 门禁模块装什么**：与答题纸 **Q6**（sessions 注册）同族——P3-05 验收行 `pnpm native:check dsh` 因注册未决而**不冒充跑过**；今天的机器形状是 `storage` 5/469（含 dsh 族 46 条：dsh_host 26＋approvals 9＋reconnect 5＋session_dsh_omp 7 的 dsh 半）＋`skeleton` 24/188。➡️ 推荐：Q6 答复时把 `dsh_host/dsh_approvals/dsh_reconnect` 与 sessions 五件**同批注册**（宿主子集不纳入，污染源 opt-in 先例）；在那之前这条验收记「由 `storage`/`skeleton` 当日读数代偿，缺口具名＝Q6」。**标注：用户未答，按推荐项记账。**
- ❓ **Q84** - **stream 消费环（`consume_task_session_stream`）要不要后续上收 core**：本片端口化后它的接触面已收敛为 `start_stream` 一个入口＋`dispatch_mux_frame` 的 projection（已在 core）＋emit 面（宿主）。上收需要审批 emit/terminal render/历史落盘的三个新端口簇。➡️ 推荐：**维持现状**——它没有「core 决策、宿主执行」的错位（纯宿主管道），上收属 P8-03（远程访问）射程的顺手事，届时与 mobile 载体一起要真需求。**标注：用户未答，按推荐项记账。**
- ❓ **Q85** - **D3 混合形态的 typed 壳退役清单**：G17 已答混合（二百六十四），typed 侧（executor/审批/credential-settings 命令）已在 core/宿主命令层就位；gateway 侧＝`invoke_dsh_remote` 现状零改动。**39 条零 caller 的 typed 壳（commands.rs 内）何时退役未定**——它们经 mux 与 typed 两条路可达，前端只走 mux。➡️ 推荐：**P8-03 开工时一次性对账退役**（与 remote 面的 mux 鉴权重做同批，避免两次动命令注册表）；P3-05 不阻塞。**标注：用户未答，按推荐项记账。**

**裁定（§4.2 四条判据对照）**：实际调用证据存在（ask 真实答复轮＋本文件 G31 三件套当场记）；必选测试通过（§2 当日矩阵，含验收行 2 的 headless 三用例与 fake server 五断言面中 core 可达的全部四条——webui 重启恢复的 reconnect 环在宿主 stream 侧，由宿主既有 recovery 测试承担，缺口在 Q84 具名）；无未解决的崩溃/数据丢失/安全越界/功能阻断问题（两处门禁盲点已当场修复并升格纪律）；中低风险明确由谁接受（Q83-Q85 按推荐项记账、具名可重开；`native:check dsh` 注册缺口＝Q6 同批，不静默）。**⇒ P3-05 标 accepted；`dsh` 门禁注册随 Q6 具名移交，typed 壳退役随 Q85/P8-03 具名移交，均不阻塞 Phase 3 内下游（P3-06 的输入不依赖它们）。**

## 2. 验收对照（2026-09-30 当日实测，退出码逐条单读）

| 命令 | 结果 |
|---|---|
| `cargo test -p aeroric-core`（core 全量） | **469/0 rc 0**（dsh 族 46 条在内） |
| `cargo test --lib -- --test-threads=1`（宿主隔离，HOME＝mktemp，二进制 mtime 核验） | **2452/0 rc 0（54.83 s）** |
| `pnpm native:check storage` | **5/469 rc 0**（floor 双写 438→451→462→463→466→469 逐批同commit） |
| `pnpm native:check skeleton` | **24/188 rc 0**（inv02/core-discovers/inventory-freeze 等门禁全过） |
| `pnpm native:check protocol` / `projects` / `tasks` | **6/98、5/21、7/151 全 rc 0**（contracts-current 修复后复绿） |
| `pnpm native:check runtime` / `agents` / `terminal-core` | **1/5、1/2、13/108 全 rc 0**（terminal-core 107→108＝typed 新用例） |
| `pnpm native:contracts --check` | rc 0，**12 份逐字节**（emitters/orphan 假零修复后与 committed 相同） |
| `pnpm typecheck` | rc 0 |
| 全量 `pnpm test` | **Test Files 376 passed／Tests 5483 passed rc 0（200.87 s）**（含 snapshot 9/9 修复） |
| `pnpm native:doctor` | 18/18 rc 0 |
| `inventory --freeze/--freeze-reachability/--check` | 全 rc 0（consistent；definition_resolved 14+38+5 锚三轮重锚；dynamicSites 归位） |
| `extraction --write/--check`（dsh_webui 清单） | 9 行／66 wrapper rc 0（三轮重切） |
| `digest --write/--check` | rc 0 |
| 两 workspace `cargo fmt --check`＋`clippy --all-targets -- -D warnings` | 全 rc 0（core 侧按新口径 `cd <ws> && cargo fmt --check -p <pkg>`，读输出不读 rc） |
| `git diff --check` | rc 0 |
| `pnpm native:check dsh` | **未运行**（模块未注册＝Q83/Q6 待答，不冒充） |
| Swift／XCTest UI | 未运行（P4-01 前不存在 target，口径见 `tooling.md` §10） |
