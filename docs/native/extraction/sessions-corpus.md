# P3-04 语料与发现层普查（从 `sessions.md` 迁出）

> **为什么这份文件存在**：`docs/native/extraction/sessions.md` 的三分手写普查节（真实语料普查／发现层
> 覆盖普查／金样脱敏规范）在 2026-09-29 被一次 `extraction.mjs --write` **整体覆盖删掉**——当时的生成器
> 会把生成区之外的散文一起抹掉。该缺陷同日已修（生成区现在由 `<!-- generated:start/end -->` 标记圈定，
> 标记之外的散文一律保留；无标记的文件拒绝覆盖）。内容按 补记一百八十六／一百八十七／一百八十八 重铺，
> **数字逐条回引那三份记录，本轮没有重新普查**；重铺当日（2026-09-29）实测过的是文件名、行数与门禁退出码。
>
> **为什么不放回去**：`sessions.md` 在 `.trellis/tasks/09-21-native-phase-3/implement.jsonl` 的注入名单里，
> 当日实测该文件 25 883 B，phase-3 注入总量 **129 552 B＝默认 `max_total_bytes` 131 072 的 98.8%**（余量
> 1 520 B，登记为 `G36`／表态项 25）。三节合计约 12 KB，放回必然越界——而 `max_total_bytes` 在全仓**没有
> 任何消费者**（`task_context.py:252` 只取 `max_file_bytes`），越界的表现是无警告的静默降级。所以这里按
> 补记一百十二 §七 的先例走「不进注入清单、由路径引用」。**要读原始普查结论的人：先读这一份，它比注入预算便宜。**

## 一、真实语料普查（2026-09-29 只读，零写入；金样测试的取材清单就按这一节定）

**为什么做**：P3-04 的验收写着「首片必须自带真实 JSONL 金样测试」（补记一百七十二），但此前没人查过
**本机到底有没有语料、每种家族长什么形状**。普查方式：只读遍历 `~/.aeroric/agent-homes/**`，对每个样本
只取头部 4 MiB、只统计键名/行数/行长，**不读取也不复制任何用户内容**（§3.1 #7 与 core 的
`core-discovers-no-real-home` 是硬约束）。

**规模（实测）**：托管档案 **101** 个、`.jsonl` **1 223** 份、合计 **2 201.8 MB**。按档案目录后缀分族：
`bare` 446、`claude` 296、`codex` 275、`omp` 201、`dsh` 5。

**目录形态有四套，深度 1～5 不等**（⇒ 发现逻辑不能只认一种布局）：

| 形态 | 例子 | 说明 |
|---|---|---|
| `sessions/<年-月-日>/` | bare、codex | 日期分桶，不按 cwd 编码 |
| `projects/<编码 cwd>/` | claude | **深度 5 处还有 `<uuid>/subagents/worker-<uuid>.jsonl`**（嵌套子代理转录） |
| `sessions/--<绝对路径编码>--/session-<uuid>/` | dsh | 精确目录段，含 `session.jsonl[.zstd]` |
| `sessions/-Downloads-<项目名>-LYX/` | omp | home 相对式编码（真实值含非 ASCII 目录名，这里刻意不抄）|

**⇒ 三条硬约束**：① 发现层必须同时支持 `sessions/**` 与 `projects/**` 两种根；② `claude` 的**嵌套
subagent 转录**（键里带 `agentId`／`isSidechain`）算不算"一个会话"必须由 P3-04 明确裁决，不能默认全收；
③ 编码 cwd 的形态至少三种（home 相对 `-Downloads-…`、绝对 `--Users-…--`、dsh 的 `~540C~6B65…` UTF-16
码元转义）——`G13`（OMP 放行根）与 INV-06 的边界检查都建立在这套编码之上。

**五种转录方言**（按首行键集与 `type` 枚举区分，`type` 值互不相交）：

| 家族 | `type` 枚举 |
|---|---|
| Claude | `user`/`assistant`/`attachment`/`ai-title`/`last-prompt`/`mode`/`permission-mode`/`file-history-snapshot`/`file-history-delta` |
| Codex | `session_meta`/`response_item`/`event_msg`/`turn_context`/`world_state`/`compacted` |
| OMP | `session`/`session_init`/`message`/`model_change`/`thinking_level_change`/`title`/`custom`/`custom_message`/`compaction` |
| DSH | **斜杠式** `session/approval/policy`、`assistant/chunk`、`command/done`、`agent/inbox/spliced` 等，带 `seq0`/`time0`/`version`/`delegationDepth`/`sourceEventSeqs` |

**两种非转录形态也住在同一批根目录里**（⇒ 光靠后缀 `.jsonl` 判断"是会话"必然误收）：
`{session_id,text,ts}`（一份 579 KB 只有 3 行、单行 550 014 字节）与
`{display,pastedContents,project,sessionId,timestamp}`，另见过 `{name,timeAgo}` 与 `{id,metadata,usage}`。
超长单行是常态：样本最大单行 **399 838 字节**、最大文件 **153.3 MB** ⇒ 分页与读取上限要按
"一行就超预算"设计。

**自然损坏：本轮样本里 0 例。** 抽样里 3 份各报 1 条「坏行」；对这三个档案目录做全量定位后一共 13 份报
坏行，而 **13/13 都恰好落在我 4 MiB 窗口的最后一行**（`badIdx == 扫描末位` 且窗口末尾不含换行）——属仪器
截断，不是语料损坏。⇒ 验收要求的「损坏记录」反例**必须自己合成**（截断的 JSON、非 object 行、`type` 缺失、
超长单行四种），不能指望真实语料提供。

**金样取材规则（首片照此执行，脱敏后落进 `core/crates/aeroric-core/tests/fixtures/sessions/`）**：五族各取
「最小／中位／最大」＝ 15 份，加三类非转录形态各 1 份 ＝ **18 份金样**；测试断言只看**结构**（行数、type
序列、分页边界、路径校验结果），**fixture 里的 `cwd`／编码目录名／`gitBranch`／`sessionId`／`uuid`／
`--iso` 时间戳一律替换成中性固定值**（普查已证实这些编码含用户真实项目路径与中文目录名）；金样一旦入库，
测试**不得再读 `~/.aeroric`**（core 里没有 HOME 发现路径，门禁 `core-discovers-no-real-home` 会守这条）。

## 二、发现层覆盖普查（2026-09-29 只读；宿主四条规则的原文、可见/不可见计数）

**规则原文（照抄行为，不是转述）**：① Claude 族 `<agent-home>/projects/<编码 cwd>/`，**递归且收任意
`*.jsonl`**（`src-tauri/src/session.rs:163-182` 决定根、`:187-216` 递归）；② Codex 族 `<agent-home>/sessions/`
（外加 `~/.codex/sessions` 与项目内 `.codex/sessions`），**递归但只收 `rollout-*.jsonl`**（同一函数里的
`is_rollout_jsonl`）；③ DSH `sessions/--<路径 slug>--/<转义会话 id>/session.jsonl[.zstd]`，**精确文件名**
（`session_dsh.rs:3-38`）；④ OMP `<agent-home>/sessions/<编码 cwd>/<ISO>_<uuidv7>.jsonl`，递归任意
`*.jsonl`（`session_omp.rs:3,110-151`）。

**计数（含 `.zstd`，共 1 409 份转录/附属文件）**：真转录 **1 249** 份只出现在两个地方——`projects/…` 572 份、
`sessions/…` 677 份（另 `jobs/<id>/tmp/parent-transcript.jsonl` 1 份是子代理父链指针，不是独立会话）。
首行键集与路径完全相关：**可见路径上 1 025 份全部是"含 `type` 的转录"，零例外**；所有非转录形态都不在转录
路径上——`{display,pastedContents,project,sessionId,timestamp}` 30 份与 `{session_id,text,ts}` 49 份
**直躺在档案根**，`{id,metadata,usage}` 49 份在 `.tmp/<uuid>/`，`{name,timeAgo}` 1 份在
`cache/composer/<hash>/recent-sessions.jsonl`，另有 27 份空/不可解析。

**⇒ 三条硬结论**：

1. **不得为"多找几个会话"放宽 glob**（读档案根／`.tmp`／`cache`）——那正是 `G13`／INV-06 的失效形状。
   金样应含一份"档案根同名形态"的 **must-reject** 用例把边界钉住。
2. **家族不能从目录名推断**：21 个不以 `_codex` 结尾的档案里放着 **221** 份 `rollout-*.jsonl`（其中 1 个
   同时有 `projects/` 与 `sessions/`），宿主靠任务记录的 agent family 决定 `is_codex`。实现若"简化"成按
   后缀分族，这 221 份会静默变成"会话找不到"。
   同批更正：第一条普查里 `bare 446／claude 296／codex 275／omp 201／dsh 5` 是**按目录后缀**分的，混合了
   真实家族，只能当布局参考、不能当家族归属证据（补记一百八十七 §四）。
3. `.tmp`/`tmp` 里共 **77** 份 jsonl（仅 0.1 MB）与 `G19`／`G20` 在凭据目录抓到的 24 个隐藏临时件**同源**
   （同一套原子写模板）⇒ P3-04 若要钉"会话目录卫生"，出口判据应与 `G20` 第 ⑤ 条同写法（钉文件数与隐藏
   `.tmp` 数，不只钉哈希）。

**仪器自纠（两条，别再犯）**：① `fs.readdirSync(dir,{recursive:true})` 返回的是**带分隔符的相对路径**，
直接 `startsWith('rollout-')` 会数出 0，与另一台仪器的 221 相互矛盾；改自写递归遍历后两次数到同一个 221。
**两台仪器不一致时，先怀疑取数方式，再怀疑仓库。** ② 第一版按"档案后缀"当家族证据，见结论 2 的更正。

## 三、金样脱敏规范（2026-09-29 实测；决定 fixture 能不能安全进版本库）

**命中面（可见路径 1 026 份里分层抽 54 份，每份最多读 2 MiB，只统计不打印）**：含绝对家目录
（`/Users/…`）**51/54＝94%**；含非 ASCII 项目祖先目录名 **51/54＝94%**；含任一真实项目目录名 **51/54＝94%**；
含 `gitBranch` 值 **21/54＝39%**；含横线编码的家目录形态 5/54＝9%。凭据面另测 166 份：**0 份**含
`api_key`/`access_token`/`authorization`/`cookie`/`password`/`secret` 一类键名，**0 份**含
`sk-…`/`ghp_…`/`xox…-` 样式字串——第一版正则曾报 89%，那是误报：它把 `input_tokens`／
`cache_read_input_tokens` 这类**用量计数键**当成了凭据，收紧后归零（教训：报"零命中"之前先确认正则不会
命中同名前缀）。

**现有门禁能挡与不能挡**：`core-discovers-no-real-home` 扫 `core/crates/aeroric-core/src` 与 `tests`，
模式 `home_dir|dirs::home|env::var(_os)?\(\s*"HOME|USERPROFILE|/Users/`。⇒ **绝对路径 `/Users/…` 会被它
当场抓住（94% 那一类已被覆盖）**；但编码后的 cwd 把斜杠换成横线（`-Users-…`、`--Users-…--`）**不含
`/Users/` 子串故放行**，项目名与非 ASCII 目录名同样不在模式内 ⇒ 今天没有任何 case 拦得住"编码 cwd 与
项目名进 fixture"（已登记 `G32`，owner `P3-04, P10-05`）。

**脱敏变换表（生成 fixture 时逐字段照做）**：

| 字段 | 变换 | 为什么 |
|---|---|---|
| 编码目录名／cwd | `fixture-project-a` ／ `/fixture/project-a` | 两者必须同步，否则发现层算出的目录名与 `cwd` 不再一致，金样就测不到真路径 |
| `gitBranch` | 常量 `main` | 值本身可能带项目名 |
| `sessionId`／`uuid`／`requestId` | 固定常量表 | 可重放、可断言 |
| 时间戳（含 `--iso` 文件名、`time0`/`ts`） | 固定值 | 分页与"按 mtime 比较"的判定必须确定 |
| **自由文本（消息正文、命令、路径串）** | **按原字节长度替换** | 长度变了，UTF-8 边界与分页金样的行为就变了 |
| `model`／`usage.*_tokens`／`effort`／`permission-mode` | **保留** | 它们是解析分支的判据，抹掉就测不到真行为 |

**入库后的零命中清单**（谁落 fixture 都要跑，全部必须为空）：
`rg -n --no-ignore '(/Users/|-Users-|--Users-|同步空间)' core/crates/aeroric-core/tests/fixtures/`；
以及把本机真实项目目录名与 `gitBranch` 实际值列成清单逐个 `rg -F` 验空。现有门禁只覆盖第一项的一半
（`/Users/`），所以 `G32` 的要求是：新增一条 `absent` case（建议名 `fixtures-carry-no-real-paths`），
模式覆盖 `-Users-`／`--Users-`／非 ASCII 目录名／`sync-space` 之类的编码痕迹，并自带 canaries
（与 `G30` 同一套机制），否则这条边界只会存在于本文档里、没人守。
