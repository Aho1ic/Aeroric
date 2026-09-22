# ADR-006：109 条 `remote_`/`wsl_` 前缀孪生命令并入父方法 + `target` 参数

- 状态：**accepted（用户在 P2-01 协议门禁的 frontier 追问中拍板，2026-09-22）**
- 实施进度（同日）：**契约侧已完成**——`inventory.mjs` 现在冻结每条命令的参数表（`params`，`!` 标记 Rust 注入句柄），`export-schema` 发布 **532 方法 + 109 别名**（`twinAliases`），别名映射 **77 proven / 32 pending**，全部数字钉死并由 Rust/Swift/TS 三方各断言一次；`inventory --check` 新增"名字既无调用方又无 `caller_evidence` 即失败"的规则（ADR-006/Q10）。**宿主侧 target 路由仍未实现**（P3-06 / P7-02），且 32 条 `pending` 别名在宿主里必须被拒绝——它们参数形状与父方法不齐（例如 `remote_lsp_hover` 多出一个父方法没有的 `remote_project_path`），这不是改名问题，是一次真实的入参设计决定。
- 日期：2026-09-22
- 关联：ADR-002（命名口径，本 ADR 收窄其适用范围）、ADR-003（平台面能力门控）、`MIGRATION_PLAN.md` §G16/§5.4、P2-01（方法面）、P3-06/P7-02（target 路由落地）

## 背景：一个行为在契约里出现三次

`command-map.json` 的可达性证据（`reachability.prefix_derived_of` + `prefix_derived_parent_reachable`）实测：

| 量 | 值 |
| --- | --- |
| 方法面（有 `rpc_method` 的命令） | 641 |
| 前缀孪生（父命令可达，自身是 `remote_*` / `wsl_*`） | **109**（`remote_` 70 + `wsl_` 39） |
| 去重后的父方法 | **70** |
| 孪生自身在活代码里查无调用方 | 34（其余 75 条经 `src/lib/invokeFacade.ts` 的 `prefixCommand` 运行时拼接而被调用） |
| 孪生的父命令是否都已在方法面里发布 | **是，缺失 0 条** |

也就是说 `git.git_list_branches`、`git.remote_git_list_branches`、`git.wsl_git_list_branches` 是**同一个行为的三种传输目标**，却在 v1 里是三个方法名。计划的 G16 结论本来就写着「必须收敛成 1 个方法 + target 参数，否则同一行为会在契约里出现 34 次」；本 ADR 就是那条结论的落点。`ProjectTarget`（`local` / `ssh` / `wsl` + `identity_key()`）在 P2-01 的 `project.rs` 里已经存在，所以收敛不需要新造概念，只需要停止发布重复名字。

## 决策

1. **109 条孪生不再作为独立方法进入本地协议 v1 的方法面**。v1 方法面从 641 收敛为 **532**。
2. 一个行为的传输目标由**参数**表达：父方法的 `params.target: ProjectTarget`。`target.kind = local` 时行为等于旧本地命令；`ssh` / `wsl` 时等于旧 `remote_*` / `wsl_*`。
3. 旧命令名**不消失**，但降级为**别名表**（`twinAliases`）：`legacy command → { method, targetKind }`。它是客户端适配层与旧数据目录的翻译表，不是协议方法面的一部分，因此改它不是协议变更——**新增/删除 v1 方法才是**。
4. ADR-002 的「叶子 = 旧命令名」规则**只对进入方法面的名字生效**；本 ADR 不推翻 ADR-002，只是把它的适用面从「所有 rpc_method」收窄到「方法面方法」。
5. 收敛**不以牺牲参数真实性为代价**：别名表每条都带 `paramMapping` 状态。已核实父子参数一致的才允许 `proven`；未核实的必须是 `pending: <task>`，且该别名**不得被宿主接受**（宿主对 `pending` 别名返回 `INVALID_ARGUMENT` + 原因），避免出现「一个方法两种参数形状」这种只能在运行时炸的债。

## 落地前必须完成的前置（本 ADR 的验收清单）

1. **参数形状核实**（P2-01）：`inventory.mjs` 目前不记录命令签名，需新增从定义点抽取 `async fn xxx(...)` 参数表，并对 109 条孪生逐条比对父命令：多出的目标参数（`connectionId` / `distro` 等）归入 `target`，其余必须严格一致。
2. **生成器改造**（P2-01）：`export-schema.rs` 发布 532 方法 + `twinAliases`，并把这些数量钉死（漂移即失败），与已有的 641→532 一样有 pin。
3. **宿主路由**（P3-06 / P7-02）：单命令入口按 `params.target` 分派到本地 / SSH / WSL 实现；`wsl` 分支受 ADR-003 的能力门控（macOS 上报 `wsl=false`）。
4. **客户端适配层**（P4/P5）：现有 `prefixCommand` 拼接出来的旧名字必须经别名表落到同一个方法 + target，React 侧行为不变（计划 §6.1）。

## 备选与被否决的理由

- **B：本轮先挂 alias 表，P3-06 再收敛**（保留 641 个名字）：被否决。重复行为会一路带进交付面，而删除方法是协议变更，等于把「现在能做的清理」变成「将来必须走 ADR 的破坏」。
- **C：只在 `methods.schema.json` 留证据不改面**：被否决。计划 §5.4 明确禁止把「看起来通过」当验收，且 §G16 的结论就是收敛。
- **反过来给 v1 补 34 条从未被调用的孪生名**（因为其中有 34 条自身查无调用方）：被否决。那正是计划禁止的「把无人调用的名字原样提升进三方契约」。

## 后果

实测收敛后的面（`command-map.json` 现算，不是估计）：

| 量 | 收敛前 | 收敛后 |
| --- | --- | --- |
| 方法面 | 641 | **532** |
| `platform_specific` 方法 | 62 | **23** |
| 需要 `wsl` 能力的方法 | 60 | **21** |
| 孪生 | 109 | 0（降级为别名） |

关键的一条：**60 条 wsl 门控里有 39 条本身是孪生，其父方法是 `domain_rpc`**（例如 `wsl_read_dir_entries` 的父是 `fs.read_dir_entries`）。因此能力门控必须变成**按 `(方法, target)` 判定**，而不是按方法判定：

- 方法的 `requiresCapability` 只在「无论什么 target 都需要该能力」时出现（剩下 21 条真 WSL 专属命令）。
- 39 条孪生的能力要求随**别名**走：`twinAliases[x].requiresCapability = "wsl"`，宿主在 `params.target.kind = wsl` 且 host 未上报 `wsl=true` 时返回 `UNSUPPORTED_CAPABILITY` + 原因（ADR-003 第 3 条不变）。
- 若照旧按方法门控，`fs.read_dir_entries` 会在 macOS 上被整体禁掉——本地读目录跟着 WSL 一起死，这是把清理做成功能收缩。

ADR-003 的「能力由 host 声明、client 不得用自己 OS 推断」继续生效，本 ADR 只把判定键从方法名扩展到 `(方法, target)`。

其他：

- `docs/native/command-map.json` 仍是 660 条命令的完整清单（**旧命令一个都没删**，Tauri 客户端继续可用）；变化的只是「哪些名字进入线协议」。
- 事件面（38 条）不受影响：事件名不是方法名，孪生只出现在命令侧。
- 「删除/合并方法即协议变更」这条从此有了反例可依赖：**别名表内的一切改动都不是协议变更**，这也是把孪生放进 alias 而不是 method 的根本理由。
