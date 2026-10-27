# 验收矩阵（AP8 R-AP8-3 / 原 P10-01 Action 1）

> 机器可读台账:`docs/native/acceptance-matrix.json`(唯一真源);
> 对账器:`node scripts/native/parity.mjs`;需求源:`docs/native/feature-matrix.json` 的 F01–F26。
> 本文件是**叙事版**,内容由台账派生——两者不一致时以台账为准,并修本文件。

## 1. 这张表回答什么

`feature-matrix.json` 的 26 条 Fxx 说的是"这个产品有哪些能力";它不回答
**"每条能力在每一端到底有没有、证据在哪、没有的理由是什么"**。本矩阵就是那三列,
按 §F「P10 → AP8/AP9/AP10 重新覆盖」的归属重新落到 Apple-only 的四个端上。

端(按 AP1 重定基线后的真实形态):

| 端 | 含义 |
|---|---|
| `cli` | `aeroric-cli` 无头入口(host 的进程形态 + RPC 客户端) |
| `native-macos` | `macos/` 原生 SwiftUI 客户端 |
| `ios-companion` | `ios/AeroricCompanionKit` 伴随端(权威执行仍在 Mac) |
| `tauri-legacy` | 待退休的 Tauri/React 前端——**历史事实**,不是交付目标 |

状态只有三个值,每个都有机器含义:

| 状态 | 含义 | 对账器要求 |
|---|---|---|
| `accepted` | 该端**有**这条能力且有验收 | 该行**必须**至少有一个已注册且可执行的 case |
| `unsupported` | 该端**有意不**承载它 | **必须**写理由(空白会被读成两种完全不同的意思) |
| `retired` | 该端已退休或能力已退休 | 同上 |

## 2. 台账在拒绝什么

计划原文要求「拒绝 blanket skip、仅字符串 grep 证明行为、mock 调用不验证后果」。
`parity.mjs` 把它拆成七条判据,前六条是规则、第七条是**规则的自检**:

| # | 判据 | 拒绝的形态 |
|---|---|---|
| 1 | 台账行覆盖 `feature-matrix` 的每一条 Fxx | 漏掉一条就等于它从未被验收 |
| 2 | 台账不许凭空长出条目 | 台账与需求源漂移 |
| 3 | **每处证据都指向已注册且有 `argv` 的 case** | 编一个不存在的测试名来充数 |
| 4 | `accepted` 必须有载体 | "声称支持但无人验证" |
| 5 | `retired`/`unsupported` 必须写理由 | 空白 |
| 6 | `unsupported` 与证据不矛盾 | 声明未支持却挂着能跑的验收 |
| 7 | **自检**:伪造证据与无理由退休都能被前六条抓到 | 规则本身写错导致"全绿" |

第 7 条不是装饰:一个永远返回"通过"的对账器和一份手写的 `accepted` 清单一样没用。
实测(2026-10-13):往 F01 的 `evidence` 里塞一个不存在的 case 名,第 3 条当场报红;
恢复后全绿。

## 3. 当前落点(2026-10-13)

`parity: 26 feature(s), 57 accepted / 26 retired / 21 unsupported surface(s), 57 evidence pointer(s) resolve.`

- **57 个 `accepted` 端**——每一条都能追到一个已注册的门禁 case。
- **26 个 `retired`**:全部是 `tauri-legacy`,理由统一为"AP10 清退旧前端"
  (§F 的归属:旧前端不是产品目标,保留它只到 AP10 获授权清退为止)。
- **21 个 `unsupported`**:伴随端**有意**不承载的部分(数据库/notebook/扩展/worktree/
  SSH/LSP/RAG/workflow 编排/ACP 媒体面/视觉面/无头入口),以及 CLI 上的 IDE/Git/worktree/
  SSH/WSL/视觉面。每条都写了理由与归属;iOS 的那批统一指向 AP5 的范围界定,
  WSL 的那批指向 AP1 重定基线后的首发范围。

## 4. 实施中发现的真实缺口(已修)

**F04/F05(终端历史)的写侧此前只在待退休的 Tauri 桥里**。core 只上收了读与删除,
追加落盘留在 `src-tauri/src/pty.rs:607`;迁移到 Apple-only 之后**新宿主一个都没有它**,
于是无头宿主跑完的任务在 `terminal-history/` 下没有日志(`GAP-AP8-HISTORY-WRITER`)。
AP8 压测把这条当机器证据钉住(见 `docs/native/acceptance-budgets.md` §3),写侧已上收 core
并接进宿主 PTY reader;同一段压测还暴露了"答复装不进一帧"(§3b)并一并修掉。

**F06(DSH 面)此前没有任何门禁**。core 的 `tests/dsh_{approvals,host,reconnect}.rs` 与
`session_dsh_omp.rs` 在树里、共 **47 个用例**,但 `checks.json` 里 `dsh` 一直是一个
**未注册的计划模块键**(`registeredBy: P3-05`),`dsh-webui` 的抽取文档是唯一提到它的地方。

这条正好是本矩阵要抓的病:一条 `accepted` 的 F06 背后没有任何可跑的验收。
已注册 `dsh` 模块(4 case / 47 用例,地板取实测值),并把 F06 的证据指向它。

## 5. 怎么跑

```bash
node scripts/native/parity.mjs
node scripts/native/check.mjs system-parity    # 含 storage-backup / migration-rehearsal / fault-injection
```

台账里**每个** `evidence` 名字都必须是 `checks.json` 里已注册且有 `argv` 的 case——
改 `checks.json` 的 case 名而不同步这里,`parity.mjs` 会红。
