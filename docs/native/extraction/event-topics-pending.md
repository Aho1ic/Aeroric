# 36 条 pending v1 event topic 的归属审计 + gap 台账核对（P3-04 开工前查的，2026-09-28）

> 只读分析工件，**不是冻结清单、没有 `--check` 门禁**。生成方法：把 `docs/native/event-topics.json` 里
> `status == "pending: P3-04"` 的 36 行，逐条用 `legacyChannels` 到 `docs/native/command-map.json` 的 `events[]`
> 取 `emitters[]`/`listeners[]`/`disposition`/`note`，按**发射点所在模块**映射归属；gap 部分读同一文件的 `gaps[]`
> 与 `docs/native/task-status.json`。文末附完整探针脚本，任何人可原样重跑。

## 一、结论：P2-01 下传的目标 Task 是错的

进度总表 P3-04 行写着「具名前置：P2-01 下传的 36 条 pending topic 接线在**本 Task**闭合」（沿自 ADR-007 的
「36 条 pending → P3-04」）。按发射点实测：

| 按发射点模块定出的归属 | 条数 |
|---|---|
| `P3-05` | 11 |
| `待判` | 10 |
| `P8-03` | 5 |
| `P3-02（accepted）` | 2 |
| `P7-04` | 2 |
| `P7-05/P7-06` | 2 |
| `P6-03` | 1 |
| `P2-05（accepted）` | 1 |
| `P3-04` | 1 |
| `P8-04` | 1 |

> **2026-09-28 补记一百二十 就地更正**：本表里的 `待判` **10 条当日已定性并落归属**——8 条 `dsh.host_*` → `pending: P3-05`、2 条 `app.*_requested` → `pending: P3-06`（§进度总表 与 `docs/native/event-topics.json` 为准）。
> 原表保留，因为「扫描看不见发射点 ⇒ 不敢定归属」正是这张表当时的判断依据；而 §三 查明发射点一直都在，只是走的是变量发射。

**属于 sessions/usage 面的只有 1 条**（`usage.statistics_updated`，发射点 `usage_index.rs:1003`）。
> **2026-09-28 补记一百十三 就地更正**：实施 T1=A 时把 **2 条**留在 P3-04——上面这 1 条，加上 `task.session`（发射点 `core_adapter.rs:538`，属已 accepted 的 P2-05 模块，但 payload 是会话内容，改派给尚未开工的 Task 更诚实）。其余 34 条全部离队。原文保留，因为「当时只数到 1 条」正是这张表被重排的起因。其余 35 条的 Rust 发射点根本不在本 Task 的文件面：11 条在 `dsh_webui`（P3-05 的宿主本体尚未迁）、5 条在 `remote`（P8-03）、2 条 `database`（P7-04）、2 条 `notebook`（P7-05/06）、`report`/`lsp` 各 1 条，**3 条的发射点位于已经 accepted 的 Task 的模块里**（`agent_ops` ×2 → P3-02、`core_adapter` ×1 → P2-05），另有 **10 条在冻结清单里没有 Rust 发射点**（动态发射），必须先定性才谈得上接线。

好消息是这 36 条**全部**能在 `command-map.json` 的 38 条事件里找到条目（`<无冻结条目>` 命中 0）——缺的不是映射表，是归属决定。

## 二、逐条表

| topic | legacy 通道 | 方向 | 发射模块 | emitters/listeners | 按发射点的归属 |
|---|---|---|---|---|---|
| `aeroric.app_settings_changed` | `aeroric:app-settings-changed` | core-to-client | `remote` | 1/1 | P8-03 |
| `agent.operation_changed` | `agent-operation-changed` | core-to-client | `agent_ops` | 1/1 | P3-02（accepted） |
| `agent.tool_install_progress` | `agent-tool-install-progress` | core-to-client | `agent_tools` | 1/0 | P3-02（accepted） |
| `app.exit_requested` | `app-exit-requested` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `app.restart_requested` | `app-restart-requested` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `dbx.production_confirm_requested` | `dbx-production-confirm-requested` | core-to-client | `database` | 1/1 | P7-04 |
| `dbx.transfer_progress` | `dbx-transfer-progress` | core-to-client | `database` | 1/1 | P7-04 |
| `dsh.approval_requested` | `dsh-approval-requested` | core-to-client | `dsh_webui` | 1/1 | P3-05 |
| `dsh.approval_resolved` | `dsh-approval-resolved` | core-to-client | `dsh_webui` | 1/1 | P3-05 |
| `dsh.host_agent_error` | `dsh-host-agent-error` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `dsh.host_archived_sessions_changed` | `dsh-host-archived-sessions-changed` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `dsh.host_remote_event` | `dsh-host-remote-event` | core-to-client | `dsh_webui` | 1/0 | P3-05 |
| `dsh.host_session_added` | `dsh-host-session-added` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `dsh.host_session_removed` | `dsh-host-session-removed` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `dsh.host_session_status` | `dsh-host-session-status` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `dsh.host_stream_error` | `dsh-host-stream-error` | core-to-client | `dsh_webui` | 4/0 | P3-05 |
| `dsh.host_workspace_changed` | `dsh-host-workspace-changed` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `dsh.host_workspace_order_changed` | `dsh-host-workspace-order-changed` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `dsh.host_workspace_removed` | `dsh-host-workspace-removed` | unproven | `无 Rust 发射点` | 0/1 | 待判 |
| `dsh.question_requested` | `dsh-question-requested` | core-to-client | `dsh_webui` | 1/1 | P3-05 |
| `dsh.question_resolved` | `dsh-question-resolved` | core-to-client | `dsh_webui` | 1/1 | P3-05 |
| `dsh.session_event` | `dsh-session-event` | core-to-client | `dsh_webui` | 1/1 | P3-05 |
| `dsh.session_jobs` | `dsh-session-jobs` | core-to-client | `dsh_webui` | 1/1 | P3-05 |
| `dsh.session_projection` | `dsh-session-projection` | core-to-client | `dsh_webui` | 2/1 | P3-05 |
| `dsh.session_queue` | `dsh-session-queue` | core-to-client | `dsh_webui` | 1/1 | P3-05 |
| `dsh.session_subscribed` | `dsh-session-subscribed` | core-to-client | `dsh_webui` | 1/1 | P3-05 |
| `lsp.diagnostics` | `lsp://diagnostics` | core-to-client | `lsp` | 1/1 | P6-03 |
| `notebook.rag_index_progress` | `notebook-rag-index-progress` | core-to-client | `notebook` | 1/1 | P7-05/P7-06 |
| `notebook.sync_updated` | `notebook-sync-updated` | core-to-client | `notebook` | 1/1 | P7-05/P7-06 |
| `project.pinned_changed` | `project-pinned-changed` | core-to-client | `remote` | 1/1 | P8-03 |
| `remote.device_paired` | `remote-device-paired` | core-to-client | `remote` | 1/1 | P8-03 |
| `remote.task_request` | `remote-task-request` | core-to-client | `remote` | 1/1 | P8-03 |
| `remote.terminal_resized` | `remote-terminal-resized` | core-to-client | `remote` | 2/1 | P8-03 |
| `task.session` | `task-session` | core-to-client | `core_adapter` | 6/1 | P2-05（accepted） |
| `usage.statistics_updated` | `usage-statistics-updated` | core-to-client | `usage_index` | 1/1 | P3-04 |
| `weekly.report_progress` | `weekly-report-progress` | core-to-client | `report` | 2/1 | P8-04 |

> 表里 10 行 `unproven / 无 Rust 发射点 / 待判` 的**当天实况**（补记一百二十，逐条核到行号，详见 §三）：`发射模块` 应为 `dsh_webui(动态)` ×8、`lib(动态)` ×2；`方向` 属"发射方存在、订阅方存在"，只是发射名不是字面量；`归属` 已落 `P3-05` ×8 与 `P3-06` ×2。**这张表是按 09-28 早上的派生状态冻结的审计快照，不做回填**——回填会让「谁在什么时候不知道什么」这条链断掉。

## 三、那 11 条「无 Rust 发射点」的 —— 2026-09-28 已逐条定性：**全部都有 Rust 发射方**

补记一百十二 写这一节时按 `command-map.json` 的 `events[].emitters[]` 数出 10 条「无 Rust 发射点」，并标注「必须先逐条定性」。本轮把 11 条（原 10 条 + `shell-output`，它已随 `terminal.output` 改派故当时没列进这一节）逐条回源，结论是**这个标签是扫描口径，不是运行时事实**：`emitters[]` 只匹配**字面量**发射，而这三族都把事件名放在变量／返回值／结构体字段里，所以扫描看不见。真实发射点（本轮全部当场核到行号，并已回写 `command-map.json` 的 `events[].note`）：

| 族 | 通道 | 发射机制 | 声明处 | emit 处 | 前端订阅 |
|---|---|---|---|---|---|
| DSH 宿主帧 | 8 条 `dsh-host-*`（session-added / session-removed / session-status / agent-error / workspace-changed / workspace-removed / workspace-order-changed / archived-sessions-changed） | `dispatch_host_frame` 的 **10 臂** frame→event 映射 | `event_names.rs:21-28` | `dsh_webui.rs:3066-3075`（表）＋ `:3079`（`app.emit(event_name, payload)`，全族仅此一处） | `src/hooks/useDshHostEvents.ts:106-127` 订 8 条 |
| 退出授权 | `app-exit-requested`、`app-restart-requested` | `ExitAction::event_name()` 返回 `&'static str` | `lib.rs:177-178`（两条 match 臂） | `lib.rs:237` `app.emit(action.event_name(), ())` | `src/state/app/useAppShellHooks.ts:126/129` |
| 本地 shell 输出 | `shell-output`（v1 侧已更名 `terminal.output`） | `OutputSink::Event{event_name}` 结构体字段 | 名字绑在 `pty.rs:2786` | `pty.rs:462`（sink 分派在 `:455`） | `src/components/ShellTerminalPanel.tsx:155` |

**三点要下游知道的**：① 那张 10 臂表里另有两条（`dsh-host-remote-event`、`dsh-host-stream-error`）**没有任何订阅者**，它们不在这个名单里是因为它们的发射点在别处也有直接字面量发射、被登记者记到了那几行——这正是 `G01` 的三条孤儿，**和这张表同源**，所以 P3-05 处置时**至少**要动那张表；但这二条另有 5 处以常量名的直接 `app.emit(`（`remote-event` 在 `:3328`、`stream-error` 在 `:3151/:3277/:3355/:3372`），删除或改向都得一并交代——也正因如此它们的 `emitters[]` 非空而那 8 条为空：扫描认得常量名发射，认不得穿过 `match` 的变量发射。② 本节旧版的三个引用行号（`lib.rs:198`、`dsh_webui.rs:3007`、`pty.rs:504`）**全部已漂**，按符号找才靠得住。③ P2-01 登记的动态站点总数 **29 处**今天复算未变（`--freeze` 会原样重印这句：`29 dynamic emit site(s) recorded verbatim, not guessed`）。

**归属**：把派生本身改成「认得变量发射」（`inventory.mjs` 解析常量值 + `export-schema.rs` 允许由此产生的 `legacyDirection`）会改动契约产物，本轮**只记账不代改**。已按 §3.1 #1 写具名前置：**8 条归 P3-05**（DSH 宿主本体尚未迁）、**2 条 `app-*` 归 P3-06**、`shell-output` 归 P5-01（v1 侧 `terminal.output` 的形状改造）。表的当前状态由 `docs/native/event-topics.json` 的 `derivation.emittersZeroMeans` 一行说明，随 `native:contracts` 进契约产物。

## 四、三条最尖锐的：发射点在已 accepted 的 Task 里

| topic | 通道 | 发射点 | 冻结清单的 note | 该 Task 状态 |
|---|---|---|---|---|
| `agent.tool_install_progress` | `agent-tool-install-progress` | `agent_tools.rs:606` | `progress already reaches the UI via agent-operation-changed; **P3-02 must either give it a consumer or drop the emit**` | **accepted** 2026-09-27 |
| `agent.operation_changed` | `agent-operation-changed` | `agent_ops.rs:190` | — | **accepted** 2026-09-27 |
| `task.session` | `task-session` | `core_adapter.rs:538` | — | **accepted** 2026-09-26 |

第一条还有一处**工件互相矛盾**：事件级 note 说该由 **P3-02** 处置，而 gap 台账 `G01` 的 `task_ids` 写的是 **P2-01, P3-05**（owner `protocol+core:dsh_host`）。同一件事两个地方各指一个 Task，且 `P3-02` 不在 `G01` 的名单里。实况（2026-09-28 当场核）：`INSTALL_EVENT` 常量在 `agent_tools.rs:25`、发射在 `:606`，`listeners: []`、`orphan: emitted-without-listener`、`disposition: redundant-channel`，而 `grep -rn agent-tool-install-progress src/ mobile/ packages/` **0 命中** —— 后端仍在发、三方仍无人接。`task.session` 那条相对乐观：它的发射点已经是 ADR-023 的 core 唯一 sink（`core_adapter.rs:538`），缺的只是 topic 的 `payload`/`plannedDirection` 判定，不是实现。

## 五、顺带查出的结构性缺口：gap 台账没有「owner 已关门」这条线

`feature-matrix.json` 的 17 条 gap 里，**10 条**的 `task_ids` 至少点到一个**已经 accepted** 的 Task，而 gap 本身不是 `closed`。其中状态仍为 `open` 的 4 条：

| gap | status | 点到的 accepted Task | 全部 task_ids |
|---|---|---|---|
| `G01` | `triaged-in-inventory` | `P2-01` | `P2-01`、`P3-05` |
| `G02` | `triaged-in-inventory` | `P2-01` | `P2-01` |
| `G03` | `open` | `P3-01` | `P3-01` |
| `G05` | `open` | `P2-02`、`P2-05` | `P2-02`、`P2-05` |
| `G06` | `open` | `P2-02`、`P2-04` | `P2-02`、`P2-04`、`P8-04` |
| `G08` | `closed-by-tool` | `P1-04` | `P1-04` |
| `G09` | `open` | `P2-05` | `P2-05`、`P4-02` |
| `G10` | `closed-by-tool` | `P1-04`、`P2-01` | `P1-04`、`P2-01` |
| `G13` | `open` | `P3-02` | `P3-02`、`P3-04` |
| `G16` | `triaged-rev2-awaiting-phase-2` | `P2-01` | `P2-01`、`P8-03` |

其中两条**直接落在 P3-04 的文件面上**：`G03`（`open`，owner P3-01 已 accepted）记的是三个无停路径的 app-lifetime 线程，证据点含 `event_watcher.rs:66` 与 `usage_index.rs:1048`——正是 P3-04 Action 2 要接手的两个文件；`G13`（`open`，task_ids `P3-02, P3-04`，P3-02 已 accepted）是 `agent-homes/*` 全量放行 vs 注释承诺按 agent 校验的不一致，而 P3-04 的 OMP 发现根收窄（INV-06）就是它的主场。**结论：P3-04 真正该先闭合的是 `G03`/`G13` 加 `usage.statistics_updated` 这一条 topic，而不是 36 条。**

结构性原因和 F3 一模一样：**账目没有机器可读的 owner→状态链接**。gap 有 `task_ids`（人写）、Task 有状态（现在有了 `docs/native/task-status.json`），但没有任何一条门禁把两者对起来。一条可行的规则：「`task_ids` 里若只剩 accepted Task 而 gap 仍 `open`，`--check` 必须报错，除非显式改派或写明由谁接受」——这属门禁工具改动，需要新负例，**不在本轮顺手做**。

## 六、处置建议（等用户在场定，Agent 不代答）

- **建议 A（推荐）**：按发射点改 `event-topics.json` 的 `status`，把 35 条改派到各自 Task（`pending: P3-05` ×11、`pending: P8-03` ×5、`pending: P7-04` ×2、`pending: P7-05/P7-06` ×2、`pending: P8-04` ×1、`pending: P6-03` ×1、`pending: P3-02/P2-05 复议` ×3、动态那 10 条改成具名的 `pending: <Task>` 或写明 `disposition`），只留 `usage.statistics_updated` 给 P3-04。代价：`event-topics.json` 是人工维护、由 `export-schema` 校验并产出 `contracts/native/v1/event-topics.schema.json`，改完要 `native:contracts --write` + 三方断言复跑（12 份产物里含它）。校验器现禁止 pending 行提前装作已决（`plannedDirection`/`payload` 必须仍是 `pending`），所以改派不会伪装成闭合。
- **建议 B**：保留「36 条都归 P3-04」的字面账，把 P3-04 变成 36 条通道的搬运工。代价是 P3-04 要碰 dsh/remote/notebook/database 四个尚未迁完的面，等于把 P3-05/P8-03/P7 的工作提前，且这些通道今天的语义还没定——不推荐。
- 无论选哪个，`G01` 的 `task_ids` 与事件 note 的矛盾必须择一改掉（现在两份工件各说一个 Task）。

## 附录：生成本文件的一次性探针（原样重跑即可复现全部数字）

只读：仅 `json.load` 三个已提交工件并写出本文件，不改任何源码、不碰真实 `~/.aeroric`。

```python
import json, io, re
from collections import Counter

topics = json.load(io.open("docs/native/event-topics.json", encoding="utf-8"))["topics"]
cm = json.load(io.open("docs/native/command-map.json", encoding="utf-8"))
ev = {e["name"]: e for e in cm["events"]}
matrix = json.load(io.open("docs/native/feature-matrix.json", encoding="utf-8"))
statuses = {t["task"]: t["status"] for t in json.load(io.open("docs/native/task-status.json", encoding="utf-8"))["tasks"]}

OWNER = {
    "session": "P3-04", "usage": "P3-04", "usage_index": "P3-04", "analytics": "P3-04",
    "agent_usage": "P3-04", "event_watcher": "P3-04",
    "dsh_webui": "P3-05", "dsh_plugins": "P3-05",
    "remote": "P8-03", "notebook": "P7-05/P7-06", "database": "P7-04",
    "report": "P8-04", "notification": "P8-04", "lsp": "P6-03", "dap": "P6-03",
    "pty": "P3-03/P5-01", "shell": "P3-03/P5-01",
    "app_settings": "P5-05/P8-02", "agent_ops": "P3-02（accepted）",
    "agent_tools": "P3-02（accepted）", "core_adapter": "P2-05（accepted）",
    "skills": "P8-01", "mcp": "P8-01", "hooks": "P8-01", "git": "P6-04",
}

def module_of(site):
    return re.sub(r"^src-tauri/src/?", "", site.split(":")[0]).replace(".rs", "").split("/")[0]

rows, owners = [], Counter()
for t in topics:
    if t["status"] != "pending: P3-04":
        continue
    sites, mods, notes = [], set(), []
    for c in t["legacyChannels"]:
        e = ev.get(c)
        if e is None:
            mods.add("<无冻结条目>")
            continue
        ems = e.get("emitters") or []
        for s in ems:
            mods.add(module_of(s))
        if not ems:
            mods.add("无 Rust 发射点")
        if e.get("note"):
            notes.append("`%s`：%s" % (c, e["note"]))
    owner = OWNER.get(next(iter(mods)), "待判") if len(mods) == 1 else "待判"
    owners[owner] += 1
    rows.append({"topic": t["topic"], "channels": t["legacyChannels"], "direction": t["legacyDirection"],
                 "evidence": t["evidence"], "owner": owner, "mods": sorted(mods), "notes": notes})

L = []
A = L.append
A("# 36 条 pending v1 event topic 的归属审计 + gap 台账核对（P3-04 开工前查的，2026-09-28）")
A("")
A("> 只读分析工件，**不是冻结清单、没有 `--check` 门禁**。生成方法：把 `docs/native/event-topics.json` 里")
A("> `status == \"pending: P3-04\"` 的 36 行，逐条用 `legacyChannels` 到 `docs/native/command-map.json` 的 `events[]`")
A("> 取 `emitters[]`/`listeners[]`/`disposition`/`note`，按**发射点所在模块**映射归属；gap 部分读同一文件的 `gaps[]`")
A("> 与 `docs/native/task-status.json`。文末附完整探针脚本，任何人可原样重跑。")
A("")
A("## 一、结论：P2-01 下传的目标 Task 是错的")
A("")
A("进度总表 P3-04 行写着「具名前置：P2-01 下传的 36 条 pending topic 接线在**本 Task**闭合」（沿自 ADR-007 的")
A("「36 条 pending → P3-04」）。按发射点实测：")
A("")
A("| 按发射点模块定出的归属 | 条数 |")
A("|---|---|")
for owner, n in owners.most_common():
    A("| `%s` | %d |" % (owner, n))
A("")
one = owners.get("P3-04", 0)
A("**属于 sessions/usage 面的只有 %d 条**（`usage.statistics_updated`，发射点 `usage_index.rs:1003`）。其余 35 条的"
  "Rust 发射点根本不在本 Task 的文件面：11 条在 `dsh_webui`（P3-05 的宿主本体尚未迁）、5 条在 `remote`（P8-03）、"
  "2 条 `database`（P7-04）、2 条 `notebook`（P7-05/06）、`report`/`lsp` 各 1 条，"
  "**3 条的发射点位于已经 accepted 的 Task 的模块里**（`agent_ops` ×2 → P3-02、`core_adapter` ×1 → P2-05），"
  "另有 **10 条在冻结清单里没有 Rust 发射点**（动态发射），必须先定性才谈得上接线。" % one)
A("")
A("好消息是这 36 条**全部**能在 `command-map.json` 的 38 条事件里找到条目（`<无冻结条目>` 命中 0）——"
  "缺的不是映射表，是归属决定。")
A("")
A("## 二、逐条表")
A("")
A("| topic | legacy 通道 | 方向 | 发射模块 | emitters/listeners | 按发射点的归属 |")
A("|---|---|---|---|---|---|")
for r in rows:
    A("| `%s` | %s | %s | %s | %d/%d | %s |" % (
        r["topic"], "、".join("`%s`" % c for c in r["channels"]), r["direction"],
        "+".join("`%s`" % m for m in r["mods"]), r["evidence"].get("emitters", 0),
        r["evidence"].get("listeners", 0), r["owner"]))
A("")
A("## 三、那 10 条「无 Rust 发射点」的（必须先逐条定性）")
A("")
A("清单里它们 `disposition` 记为动态发射，`--check` 只断动态站点数量不漂移，因此**没人欠它们一个归属**。"
  "P2-01 当时登记的动态站点共 29 处，含 `lib.rs:198` 的 `action.event_name()`、`dsh_webui.rs:3007` 的 10 路帧映射、"
  "`pty.rs:504` 的 `OutputSink::Event`，以及 `AgentInstallStage::*` 那批实为进度回调的名字：")
A("")
for r in rows:
    if r["mods"] == ["无 Rust 发射点"]:
        A("- `%s` ← %s" % (r["topic"], "、".join("`%s`" % c for c in r["channels"])))
A("")
A("## 四、三条最尖锐的：发射点在已 accepted 的 Task 里")
A("")
A("| topic | 通道 | 发射点 | 冻结清单的 note | 该 Task 状态 |")
A("|---|---|---|---|---|")
A("| `agent.tool_install_progress` | `agent-tool-install-progress` | `agent_tools.rs:606` | "
  "`progress already reaches the UI via agent-operation-changed; **P3-02 must either give it a consumer or drop the emit**` | **accepted** 2026-09-27 |")
A("| `agent.operation_changed` | `agent-operation-changed` | `agent_ops.rs:190` | — | **accepted** 2026-09-27 |")
A("| `task.session` | `task-session` | `core_adapter.rs:538` | — | **accepted** 2026-09-26 |")
A("")
A("第一条还有一处**工件互相矛盾**：事件级 note 说该由 **P3-02** 处置，而 gap 台账 `G01` 的 `task_ids` 写的是 "
  "**P2-01, P3-05**（owner `protocol+core:dsh_host`）。同一件事两个地方各指一个 Task，且 `P3-02` 不在 `G01` 的名单里。"
  "实况（2026-09-28 当场核）：`INSTALL_EVENT` 常量在 `agent_tools.rs:25`、发射在 `:606`，"
  "`listeners: []`、`orphan: emitted-without-listener`、`disposition: redundant-channel`，"
  "而 `grep -rn agent-tool-install-progress src/ mobile/ packages/` **0 命中** —— 后端仍在发、三方仍无人接。"
  "`task.session` 那条相对乐观：它的发射点已经是 ADR-023 的 core 唯一 sink（`core_adapter.rs:538`），"
  "缺的只是 topic 的 `payload`/`plannedDirection` 判定，不是实现。")
A("")
A("## 五、顺带查出的结构性缺口：gap 台账没有「owner 已关门」这条线")
A("")
bad = []
for g in matrix["gaps"]:
    ids = re.findall(r"P\d+-\d+", g.get("task_ids") or "")
    acc = [i for i in ids if statuses.get(i) == "accepted"]
    if acc and g.get("status") != "closed":
        bad.append((g["id"], g.get("status"), acc, ids))
A("`feature-matrix.json` 的 17 条 gap 里，**%d 条**的 `task_ids` 至少点到一个**已经 accepted** 的 Task，而 gap 本身不是 "
  "`closed`。其中状态仍为 `open` 的 4 条：" % len(bad))
A("")
A("| gap | status | 点到的 accepted Task | 全部 task_ids |")
A("|---|---|---|---|")
for gid, st, acc, ids in bad:
    A("| `%s` | `%s` | %s | %s |" % (gid, st, "、".join("`%s`" % a for a in acc), "、".join("`%s`" % i for i in ids)))
A("")
A("其中两条**直接落在 P3-04 的文件面上**：`G03`（`open`，owner P3-01 已 accepted）记的是三个无停路径的 "
  "app-lifetime 线程，证据点含 `event_watcher.rs:66` 与 `usage_index.rs:1048`——正是 P3-04 Action 2 要接手的两个文件；"
  "`G13`（`open`，task_ids `P3-02, P3-04`，P3-02 已 accepted）是 `agent-homes/*` 全量放行 vs 注释承诺按 agent 校验的不一致，"
  "而 P3-04 的 OMP 发现根收窄（INV-06）就是它的主场。**结论：P3-04 真正该先闭合的是 `G03`/`G13` 加 "
  "`usage.statistics_updated` 这一条 topic，而不是 36 条。**")
A("")
A("结构性原因和 F3 一模一样：**账目没有机器可读的 owner→状态链接**。gap 有 `task_ids`（人写）、Task 有状态（现在有了 "
  "`docs/native/task-status.json`），但没有任何一条门禁把两者对起来。一条可行的规则：「`task_ids` 里若只剩 accepted Task "
  "而 gap 仍 `open`，`--check` 必须报错，除非显式改派或写明由谁接受」——这属门禁工具改动，需要新负例，**不在本轮顺手做**。")
A("")
A("## 六、处置建议（等用户在场定，Agent 不代答）")
A("")
A("- **建议 A（推荐）**：按发射点改 `event-topics.json` 的 `status`，把 35 条改派到各自 Task"
  "（`pending: P3-05` ×11、`pending: P8-03` ×5、`pending: P7-04` ×2、`pending: P7-05/P7-06` ×2、`pending: P8-04` ×1、"
  "`pending: P6-03` ×1、`pending: P3-02/P2-05 复议` ×3、动态那 10 条改成具名的 `pending: <Task>` 或写明 `disposition`），"
  "只留 `usage.statistics_updated` 给 P3-04。代价：`event-topics.json` 是人工维护、由 `export-schema` 校验并产出"
  " `contracts/native/v1/event-topics.schema.json`，改完要 `native:contracts --write` + 三方断言复跑（12 份产物里含它）。"
  "校验器现禁止 pending 行提前装作已决（`plannedDirection`/`payload` 必须仍是 `pending`），所以改派不会伪装成闭合。")
A("- **建议 B**：保留「36 条都归 P3-04」的字面账，把 P3-04 变成 36 条通道的搬运工。代价是 P3-04 要碰 dsh/remote/notebook/database "
  "四个尚未迁完的面，等于把 P3-05/P8-03/P7 的工作提前，且这些通道今天的语义还没定——不推荐。")
A("- 无论选哪个，`G01` 的 `task_ids` 与事件 note 的矛盾必须择一改掉（现在两份工件各说一个 Task）。")
A("")
io.open("docs/native/extraction/event-topics-pending.md", "w", encoding="utf-8").write("\n".join(L) + "\n")
print("rows", len(rows), "gaps-mis-owned", len(bad))
for o, n in owners.most_common():
    print(" ", n, o)
```
