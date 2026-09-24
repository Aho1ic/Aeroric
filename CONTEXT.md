# CONTEXT — 术语表（原生迁移）

纯术语表。实现细节在 `MIGRATION_PLAN.md`，边界与决策在 `docs/native/adr/`，逐项映射在
`docs/native/command-map.json` / `docs/native/feature-matrix.json`。此文件只统一"同一个词只有一个意思"。

| 术语 | 定义（不是实现说明） | 不是什么 |
|---|---|---|
| 冻结清单 / frozen inventory | `docs/native/command-map.json` 里 660 条命令的逐项映射，是可比对的基线工件 | 不是随手更新的草稿；改动它等同于改协议 |
| 旧命令名 / legacy command name | `rust_path` 的尾段，即今天 Tauri `invoke("…")` 用的字符串 | 不是 RPC 线名（线名是 `rpc_method`） |
| RPC 线名 / `rpc_method` | 本地协议 v1 的方法名，口径 `<服务面>.<旧命令名>`（ADR-002） | 不是 relay 协议的方法名，两者版本互不借用 |
| 服务面 / service area | 22 个能力分组名（`database`、`notebook`、`git`、`dsh`、`ide` …），即 RPC 前缀的取值域 | 不是 Rust 模块名（56 个），也不等于源码目录 |
| `core_service` | 该命令由 `aeroric-core` 的哪个服务实现 | 不是 UI 归属，也不是 check 模块 |
| 类别 / `category` | `domain_rpc` / `platform_specific` / `host_local_os_ui` / `internal` 四选一，决定能否被 host 执行 | 不是"实现语言"；值必须由 `meta.category_resolution` 推导可复现 |
| 归属类字段 / ownership field | 决定"谁能执行、谁实现、走哪条路由"的字段（`category`、`rpc_method`、`core_service`、`check_module`） | 值不许停留在"当初手工定的"——必须有登记的推导 |
| check 模块 | `scripts/native/checks.json` 里一个验收入口的名字（如 `skeleton`） | 不是测试文件；不是 CI job |
| 门禁 / gate | `pnpm native:check <module>` 跑出的一组 case，全部通过才算该模块验收 | 绿灯不等于工件完好（见"正控"） |
| 正控 / positive control | 证明"扫描确实读到了内容"的断言（每个被扫路径至少一条） | 不是"没匹配上就算干净"——空语料与被忽略文件都长这样 |
| 地板 / floor | `runner.floors` 里冻结的最少用例数，与 `minCases` 同一次编辑才允许改，只升不降 | 不是可以顺手调低的阈值 |
| 反例 / counterexample | 能推翻当前实现的一个具体场景（含必须由真实测试覆盖的那类） | 不是"看起来能跑" |
| §5.1 展开表 / expansion | 一个目录级模块"一文件一条"的迁移行清单，可由 `scripts/native/extraction.mjs` 重导 | 不是授权改架构；不是手写后就准的清单 |
| 唯一写入者 / single writer | 一个数据目录同时只有一个业务写入进程（INV-03） | 不是"锁文件存在就代表它还活着" |
| PTY generation | 一次 PTY 生命周期的代号，用于丢弃迟到的退出/事件（INV-04） | 不是 taskId；不是 sessionId |
| 托管 home / managed agent home | `~/.aeroric/agent-homes/<family>` 下由 Aeroric 读写的目录 | 不是用户自己的 `~/.omp`、`~/.claude` 等外部历史 |
| 纯原生 / 高级(advanced) 模式 | 呈现方式偏好（`interactionMode`），切换不重启 Agent、不改协议 | 不是外观的 glass/classic，也不是权限档位 |
| 零证据 / zero-evidence | 在生产与测试源码里都找不到任何调用线索的命令 | 不是"确定死代码"；前缀派生与只被 mux 触达都另计 |
| 已退役 / RETIRED | `protocol_inventory.rs` 里显式标为退役的条目（48 项），不计入能力面 | 不是"待支持"，也不是 live 名字 |
| 能力门控 / capability gate | 方法在 schema 上声明 `requires_capability`；**能力由 host 声明，client 不得用自己所在 OS 推断 host 能力**（ADR-003） | 不是客户端本地开关；控件禁用之外服务端也必须拒绝 |
| 平台面 / platform surface | 62 条 `platform_specific` 命令（`wsl_git` 32、`wsl` 19、`wsl_fs` 9、`platform` 2）及其方法 | 不是"暂不迁移"的委婉说法；已进入协议 v1 |
| 方法面 / method surface | 协议 v1 冻结时包含的全部方法（ADR-003 后为 641） | 不是 660：其中 19 条 host_local_os_ui 有意无线名 |
| 权威来源 / authoritative source | 一个事实唯一的答案所在处（如事件通道以源码常量与调用点为准） | 文档转述、ADR 表格、记忆里的数字都不是 |
| 回滚点 / rollback point | 基线 commit `ca69a4b` + 可整块丢弃的新增目录 | 不是 `git reset --hard`；不是把旧全量内存快照写回去 |
| 未决 / open、deferred | 需要用户拍板或需单独审定才可关闭的事项（Q1、Q5、Q6、Q7） | 不许写成"已通过"；关闭需带实测复验 |

## 协议第二轮新增（P2-01，2026-09-22）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 详情白名单 | `RpcError.detail` 允许出现的 10 个键，由 `ErrorDetail` 结构体导出并写进契约；未登记即不上线（ADR-008） | 不是 `mustNotContain`——那只是说明为什么闭合 |
| 闭合内容 | `detail` 这类"宁可拒绝未知键"的字段，与帧级的开放内容相反 | 不是前向兼容的例外，是有意取舍 |
| 主版本严格相等 | 本地 `v` 必须等于 `PROTOCOL_VERSION`，不等即握手失败（ADR-009） | 不是版本区间协商；兼容性走具名能力 |
| 独立版本空间 | 远程 relay 协议与本地协议各自演进，桥接必须显式公布映射的版本对 | 不是同一版本号的两处使用 |
| legacyDirection / plannedDirection | 前者是旧通道 emitters/listeners 的证据，后者是 v1 的计划；两者不许互相冒充 | `unproven` 不是"没有事件"，是"证据里没有 Rust 侧发出方" |

## 存储层门禁新增（P2-02，2026-09-22）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 写入者资格 / writer eligibility | 某个数据目录**能不能**成为 writer（由所在文件系统决定，ADR-010）；资格 ≠ 权限，权限是 lease | 不是 `StorageWritePermission`（那是"没拿到 lease 就写"） |
| 持有者提示 / holder hint | `.write.lock` 里那份 `HolderInfo`，锁拿到之后才写，只是报告 | 不是锁本身；删掉它不会释放锁 |
| 写入世代 / writer epoch | 一次 writer 会话的身份（`runtime.json.epoch` + `HolderInfo.epoch`），用于识别"上一个写我的人" | 不是 `revision`（世代内递增的是 revision） |
| 云同步根 / cloud-synced root | 文件在本地盘上、后台被 iCloud/OneDrive/Dropbox 同步的目录：本机锁有效，跨机合并不可控 | 不是网络卷；不是"任何带同步字样的路径" |
| 非本地卷 / non-local volume | NFS/SMB/FUSE 等记录锁语义不保证的挂载，直接拒绝当 writer | 不是外部盘（本地 USB/Thunderbolt 仍算本地） |
| 位置探针 / storage location probe | core 定义的端口（`VolumeKind` + 前缀表 + 分级判定）；真实 `statvfs`/`GetDriveType` 在 adapter | 不是"core 自己去查环境"；判不出来是 Unknown，不是放行 |
| 权限回执 / permission receipt | adapter 对"该平台私有权限已强制"的显式确认（Windows ACE 语义）；无回执 = Unknown = 不可写 | 不是 `FileMode::Private`（那是 unix 侧 core 自己保证的部分） |
| 身份重验 / lease identity revalidation | 写入前比对锁文件 `(dev, ino)`；消失或换 inode 即停写报错，不自动重拿 | 不是 `metadata().is_ok()`（那只证明路径还在）；Windows 无稳定等价 API，差异写死不假装 |
| 内容指纹 / content fingerprint | 受管文件的 `(sha256, size)` 记录，跨 writer 与人工恢复的检测手段；不符即 `REVISION_CONFLICT` | 不是 revision：计数器只在参与它的 writer 之间有意义 |
| 可丢弃状态 / disposable state | `runtime.json`：删了即重建，不影响用户数据 | 不是备份；不是"必需文件"，任何"没它就报错"的设计都是错的 |
| 机制 / 语义生效 | 机制=能力已实现并有反例；语义生效=生产写入路径真的经过它。两者分开记账（ADR-012 决策 3） | "写完了"不等于"不变量成立了"；INV-03 在 P2-05 前不宣布全局成立 |
## 项目层门禁新增（P2-03，2026-09-22）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| parity 知识表 | `core/crates/aeroric-protocol/knowledge/project-parity-cases.json`：旧客户端 TypeScript 亲自算出的 input→expected 表，是行为的**权威存储** | 不是文档里的示例；不是手抄的期望值 |
| 派生链 | 真实 TS 函数 → 知识表 → 生成器 `include_str!` → 契约产物 → TS 与 Rust 各自断言 | 不是"两边各写一遍再人工比对" |
| 显示串 / display path | `path` 里那个 `ssh://conn/app`：给人看、给日志、不是身份也不是文件系统路径 | 不是 `identity_key`；不是可 join 的路径 |
| 项目身份 / identity key | `ProjectTarget::identity_key()`（`local:<path>` / `ssh:<conn>:<path>` / `wsl:<dist>:<path>`），唯一的"是不是同一个项目"答案 | 不是项目 id；不是 `path` 字面值 |
| 无效改动 / no-op mutation | 语义上什么都没变的调用（把 pin 设成原值、重命名成分组同名）：不写文件也不推进 revision | 不是失败；不是"写了同样的内容" |
| 活任务 | 状态属于契约里 `TaskStatus::is_live()`（pending/running/input_required/detached）的任务；有活任务就拒绝移除项目记录 | 不是"看起来在跑"；不用 ANSI 猜（INV-12） |
| 无损归一 / lossless normalisation | 只在"不会丢信息"的前提下统一路径拼法：去尾分隔符、`\\` 与 `/` 同义；**不**改大小写、不跨记录合并 | 不是 canonicalise（真实大小写、符号链接目标都可能改变含义）；不是自动合并 |
| 不可寻址记录 / unaddressable record | `location.kind` 认不出的项目：能列出、能删除，但不能被任何需要"它在哪"的操作使用 | 不是坏文件（文件整体仍可读）；不是"远程项目"（core 不猜） |
| 有意分歧 / intentional divergence | parity 表里 `expected`（旧客户端算出）与 `coreExpected`（新核心意图）并列的用例：两侧各自被断言，差异是写明并评审过的 | 不是"测试期望写错了"；不是静默改变行为 |


## 任务层门禁新增（P2-04，2026-09-23）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 认领 / claim | 一次尝试在 `task-operations.json` 里的登记；未结清期间同 key 的重试被拒 | 不是锁（它不阻止别的任务）；不是"进程存在"的证据 |
| 操作键 / operation key | 调用方为一次尝试自带的身份（= §2.3 `idempotencyKey`）。**同键=同一次尝试**，不同键=两次尝试，即便落在同一任务上 | 不是 generation（那是 core 的运行编号）；不是 taskId |
| 世代 / generation | core 每次真实尝试前进一格的运行号；事件靠它判新旧，且"已结清"才算陈旧 | 不是 revision（那是快照计数器）；不是重试身份（ADR-017） |
| 重叠运行 / overlapping runs | 同一任务允许多代同时在飞（Q41）；此时任何单代终止不写终态，任务回 `running` | 不是"最新一代赢"；不是双开的实现细节——legacy `TaskManager` 必须先改成 `(task_id, generation)` 键 |
| 写入基线 / cover | 一次写只允许覆盖"core 上一次发布的字节"（`runtime.json` 的 `(sha256,size)` 表） | 不是"我刚读到的内容"（每次写前都读，那样检查永不成立）；不是 revision |
| 首次采纳 / adoption | core 从未发布过的文件，第一次写直接接纳磁盘现状并记账；此后受保护 | 不是"无条件覆盖"；不是永久盲点 |
| 待确认 / awaiting acknowledgement | 被对账截断的运行，在有人看过之前不许 `resume`（只挡续跑，Q42/Q43） | 不是新状态（状态词表 v1 冻结）；不是"任务被锁住" |
| 乐观写 / optimistic write | 先改内存/磁盘、失败再回滚。core **不做**这种写：失败即什么都没发生，因此不存在"磁盘回滚" | 不是 `rollback_mutation`（那是宿主自己那份乐观列表的字段级回滚） |
| 远程通道限制 | 附件与 worktree 不能进入 ssh/wsl 运行：由服务层拒绝，不只靠表单隐藏 | 不是"产品不支持远程"；不是 UI 偏好 |

## 宿主层门禁新增（P2-05，2026-09-23）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 运行身份 / run identity | 一个任务某一次运行的唯一编号：core 的 per-task generation，经 `LaunchRequest`/`ResumeRequest` 传给宿主。全仓只有这一个（ADR-020） | 不是 `STARTUP_SIGNAL_GENERATION`（进程全局 `AtomicU64`，已决定删除）；不是 `DshStartAttempt.generation`（按 agent 不按任务）；不是 `Arc::ptr_eq` 的句柄身份 |
| 句柄身份 / handle identity | `Arc<...Child>` 的指针相等性，ADR-020 后**只**用作"同代被注册两次"的安全网断言 | 不再是"要不要清理 PTY"的判定依据（那是 `remove_pty_handles_if_current` 今天的职责） |
| 按任务 / 按运行 | 一个容器存的是"用户对任务的整体意图"（取消、手动完成）还是"某一次运行的资源"（pty 句柄、子进程、启动输入信号）。重键后两类仍不同，区别写明在容器上而不是靠猜 | "按任务"不等于"可以覆盖"；取消一个双开任务是两代都停 |
| 卷型探针 / volume probe | 宿主用 `statfs` 实测文件系统类型（macOS/BSD 读 `f_fstypename`，Linux 读 `f_type` 超级块魔数）；答不出就是 `Unknown`，不给 writer 资格 | 不是 `statvfs`（可移植的那个恰好不带类型字段）；不是从路径字符串猜网络盘 |
| 权限回执 / permission receipt | "这个目录确实只有本人可写"的平台证明。unix 由 0600/0700 构造成立；Windows 复用 `ensure_private_dir` 已在调的 `restrict_to_current_user` 的返回值 | 不是"ACL 一般会继承所以大概没事"（ADR-010 明确否决）；失败必须 `false`，不猜 |
| 否决表 / veto table | Linux 超级块魔数里认定"远程/不可信锁语义"的那一份。它与"本地表"必须逐值不相交，由测试钉住 | 不是"认识的 FS 白名单"——表外的值是"无意见"，不是"本地" |

## 宿主层门禁新增（P2-05 续，Q51～Q54）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 资源型 / 意图型操作 | 只带 taskId 时：资源型（写输入、resize、会话挂接、启动信号投递）寻址**已注册的最大 generation**；意图型（cancel、手动完成）作用于**全部在飞代**（ADR-025） | "已注册的最大代"不等于"账本计数器当前值"——后者在没有运行时会指向不存在的代 |
| 一次性幂等导入 | 分组名单从 localStorage 迁进 core 的动作：只在 core 文件不存在时发生，重复执行不追加、不改顺序 | 不是"每次启动都合并"；不是把 localStorage 继续当真相源 |
| 元数据重放 / 状态 op 不重放 | 被 `REVISION_CONFLICT` 拒绝时：无副作用意图重应用一次后重试，可能起进程的意图直接报错（ADR-026） | 不是"operation key 能兜住所以都可以自动重试"；重放上限**一次** |
| 只读实例 | 拿不到 lease 的第二个实例：能看不能写，不抢租约、不退临时目录，且告知必须**持续可见**（ADR-027） | 不是"降级到临时目录"（那会让两份数据分叉）；`StartupDegradation` 的 toast 会自动消失，不满足"持续可见" |
| 启动族 / launchFamily | 决定用哪套 CLI 参数起进程。宿主侧**委托**给 `get_agent_launch_spec()`，因为 `claude_gpt55` 要读用户脚本内容才能定（ADR-028） | 不是会话族；不是可以在宿主里查表写死的东西 |
| 会话族 / sessionFamily | 决定会话文件格式与配置 home。内置四个 + `claude_gpt55`（Claude 格式）走表；自定义 profile 用 `family`，空则回落 `codex_like` | 不是启动族；`src/agents.ts` 的 `agentOption` 给它的**不是**这个答案 |
| content-derived | 契约里的**非值**：表示"这个问题只有宿主（读了用户脚本之后）能答"。三方都断言它不可被解析成 `ProtocolFamily` | 不是一个族名；把它当族名用等于把 ADR-028 拆回一个字段 |

## 宿主层门禁新增（P2-05 续，Q59～）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 销账 / consume-on-settle | 一次性标记被某个退出**读走并清除**的动作。取消/手动完成的账按任务共享，只由**最后一个退出的代**销账（ADR-030） | 不是"谁先退出谁吃掉"（那是今天的实现，双开时会把后退出的一代误判 failed）；销账判断必须在自我 retire 之后 |
| 未 Minted 运行 / unminted run | `generation == 0` 的运行：所有 shell，以及 writer 未递交前由旧命令直接起的任务。0 是**保留常量**，不是第四个计数器（core 的编号从 1 起，故它永远抢不到寻址） | 不是"没有代"；不是宿主自己分配的号；`RunRegistry` 里它与真实代同等参与 `newest`/`live` 计算 |
| 存活代 / 最新代 | "这个任务还有进程活着吗"看**任意一代**（`has_live_run`）；"这次输入/resize 给谁"看**已注册的最大代**（`resource_run`，ADR-025） | 两者不可互换：用最新代判存活会把仍在跑的老代报成已死，用任意代做寻址会把输入喂给错误的窗口 |
| 条目版本戳 / entry version stamp | 只在一张表内部回答"我登记的那条还在不在"的序号。`STARTUP_SIGNAL_GENERATION` 本波降格为此（ADR-032），**不是**运行身份 | 不是 generation；除 signal 登记/清除两处外不得再被引用，这条进门禁 |
| 反查释放 / release-if-unreferenced | 路径 claim 的唯一释放规则：先删本次会话条目，再反查四张会话表，仍有人引用就不放（ADR-031） | 不是"删条目顺手删 claim"；不是持有者计数（那是第二份会漂移的账） |

## 发射面移交新增（P2-05 续，Q62～Q63，ADR-033/034）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 发射面 / emit surface | 某事件在宿主里的**全部 `.emit` 调用点**，以 `command-map.json` 的 `events[].emitters` 为准。`task-status` 23 个（1 个是 core 的 sink）、`task-session` 7 个（同上） | 不是"包装函数的调用点数"（那个数曾把 28 处算成 8 处）；不是 `event_names.rs` 里的常量个数 |
| 观察 / observation | 宿主把**已经发生的事实**（退出、hook、转录行、DSH 状态变化）报进 `TaskService::observe`，由 core 的 reducer 决定是否改变状态、是否发射。28 处移交的目标形态 | 不是"宿主直接调 sink"（那只是换个发射者，状态没过 reducer）；不是命令（命令是意图，观察是既成事实） |
| 迟到观察丢弃 / late-observation drop | core 规则：**来自观察**的状态变更，若该任务在本进程已无存活代，则 `Dropped`，不改记录也不发事件（ADR-034）。今天它是 `session.rs` 3 个发射点上的局部习惯 | 不是按代判定（必须按"任意存活代"，否则双开时迟到的老代观察会被误丢，那是行为变更）；不覆盖启动/退出/取消/手动完成/DSH 状态机那些宿主主动告知的变化 |
| 可见性决定权 / who decides visibility | "这条状态该不该让用户看到"的归属。移交后归 core；`is_task_active` 保留但只剩循环条件与流程守卫两种用途 | 不是"谁调用 `.emit`"（那是发射权）；宿主删掉 `session.rs:70` 那一处读取≠可以删掉 `is_task_active` 本身 |

## 决策/落盘解耦新增（P2-05 续，Q64，ADR-035）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 事件权威 / 数据权威 | 两件事，不按平台切：**事件权威永远是 core**（唯一决定"这条状态用户该不该看到"的地方）；**数据权威按平台递交**（ADR-021：目前只有 macOS）。非 writer 平台 core 只读磁盘行做决策、不提交 | 不是"core 是不是 writer"——那是数据权威；不是"谁调 `.emit`"——那是发射权，两者都收口在 core 才叫移交完成 |
| 只决策的观察 / announce-only observation | `observe` 的决策段（reducer + 两道守卫）不依赖写权，落盘是它后面可选的一步。非 writer 平台走"决策 + 发射 + 不提交" | 不是"观察失败就丢掉"（那是静默不发，ADR-033 §4 禁止）；不是"排队等切换后补发"（补发队列是第二个事件权威） |
| 准入即发射 / admitted means emitted | 被 reducer **准入**的观察必须发事件，即使 `changed == false`；只有 `EventOutcome::Dropped` 才不发。今天 28 处里 25 处无条件发射 | 不是"状态变了才发"（那是把宿主没做的折叠补上，属于夹带行为变更）；不是"发两次算重复" |
| 存活代登记 / host-reported liveness | 宿主把"某个代正在跑"报进 core 账本的动作。DSH 的 9 处必须有它才能删：dsh 从不走 core 的 `start`，`live_for(task)` 恒空 | 不是 core 铸造 generation（ADR-020 的唯一编号源不变）；不是 `RunRegistry` 的注册（那是宿主自己的寻址表，core 看不见） |

## 存活代来源新增（P2-05 续，Q65，ADR-036）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 问出来的存活 / asked liveness | core 通过 `LiveRuns` 端口**问**宿主"这个任务还有活着的代吗"，自己不存副本。宿主答的是 `TaskManager::runs`（真正握着进程句柄的那张表） | 不是 core 里的镜像表（漏一次 retire 就永久"活着"，失效方向是抑制闸静默停摆）；不是"宿主自己做决定"（决定仍在 core 的准入判据里） |
| 存活判据之并 / union of two sources | `is_task_live = 有 core 自己的 claim ∨ 端口答是`。两半各覆盖一类运行：claim 覆盖 core 起的，端口覆盖旧命令路径起的 | 不是"看最新一代"（ADR-025：存活看任意一代）；不是只看 claim（产品路径今天一条 claim 都不产生，只看它就会让 5 处 launch 的 `running` 全部静默） |

## 观察来源分类新增（P2-05 续，删 dsh 那 9 处时补上，ADR-034 更正二）

| 术语 | 含义 | 不要与混用 |
| --- | --- | --- |
| 观察来源 / observation origin | `TaskEvent::Status` 上的 `ObservationOrigin::{Transcript, Hook, Runtime}`。ADR-034 的迟到观察闸**只对前两者生效**；`Runtime` 是宿主自己改的（launch / cancel / 手动完成 / dsh 状态机），今天无条件发，移交后仍然无条件 | 不是状态词（九值词表 v1 冻结，没加任何东西）；不是"哪个进程"（那是 generation）；不是宿主的权限——它是事件自带的事实 |
| 有闸 / 无闸 / is_guarded | `Transcript \| Hook ⇒ 有闸`。这条边界在补记四十八之前**无法执行**（事件里没这个信息），于是 dsh 的 9 处非终态报告会被误判"无存活代"而九处全静默 | 不是"是否终态"（那是 `is_terminal`，另一道判据）；闸的判定仍是 `is_task_live`，origin 只决定要不要问它 |
