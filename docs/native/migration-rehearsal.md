# 迁移演练（Task P8-05 Action 4/5）

> 建立时间：2026-10-07。对应计划：`MIGRATION_PLAN.md` §Task P8-05 Action 4（合成数据演练）
> 与 Action 5（生产数据，**需用户同意**）。脚本：`scripts/native/migration-rehearsal.mjs`。

## 1. 这个脚本回答什么

`smoke-headless.mjs` 在**空目录**上跑一遍理想生命周期；本脚本要回答的是**切换那一类**
问题，所以它的夹具是一份**手写的旧客户端形状目录**（含旧客户端自己写下的、新客户端不声明
的键）：

| 检查组 | 判据 | 为什么是它 |
|---|---|---|
| 读旧目录 | 宿主能在旧形状目录上启动；`doctor --json` 读得懂；启动**不改动**任务表 | "读不写"是回退面成立的前提：切换失败时把 Tauri 指回来，数据必须原封不动 |
| 字节保真 | 旧记录的 `legacyLaunchFamily`、项目记录的 `legacyOnlyKey` **原样保留**；新任务确实落盘 | ADR-037/`Persist::ViaHost` 的义务：core 决定的记录按字节转发，不经过只声明已知键的结构体（补记六十的教训） |
| 崩溃恢复 | 宿主被 **SIGKILL**（不是 SIGTERM）后冷启动成功；崩溃前已落盘的记录一条不少；目录仍可解析 | 断电/崩溃是真实场景；优雅退出的测试覆盖不到"写到一半死掉" |
| 锁竞争 | 崩溃后尸体 socket 不挡重启；活宿主的第二个竞争者**被拒**（rc 3）且原宿主仍可服务 | INV-03 的机器形状：同一 data dir 只能有一个业务写者 |
| 回退面 | 数据**原位**读写（没有第二个目录）；`doctor` 输出可解析 | 计划原文："只用测试数据成功演练回退到 Tauri，未恢复旧进程的陈旧全量内存 snapshot" |

## 2. 怎么跑

```bash
# 合成数据（默认；结束即清理 scratch）
node scripts/native/migration-rehearsal.mjs

# 保留 scratch 便于人工检查（会打印路径）
node scripts/native/migration-rehearsal.mjs --keep

# 门禁形态
pnpm native:check cutover          # 含 migration-rehearsal 这一条 case
```

2026-10-07 实测：**16 项 check 全过**（`migration-rehearsal: all checks passed (synthetic data only)`）。

## 3. 打包 helper 的那一半（Action 7/10）

`tauri-host-smoke.mjs` 的 bundle 段（opt-in：`AERORIC_TAURI_BUNDLE=1`，因为要先跑一次
`pnpm tauri build`）覆盖：

```bash
pnpm tauri build --bundles app         # beforeBuild hook 会先跑 native:prepare-tauri-host
AERORIC_TAURI_BUNDLE=1 node scripts/native/tauri-host-smoke.mjs
```

2026-10-07 实测（本机 arm64）：

- bundle 存在，`Contents/MacOS/` 下同时有 `aeroric`（125 157 424 B）与 `aeroric-cli`（28 431 632 B）；
- 两者都是 `arm64`（打包成功但架构不符是最难查的一类故障）；
- `codesign --verify --deep --strict Aeroric.app` 通过，且日志里能看到 bundler 单独签了 helper
  （`Signing …/Contents/MacOS/aeroric-cli` → `--validated:…/aeroric-cli`），即**嵌套签名与资源封印**成立；
- helper 的签名为 ad-hoc（`flags=0x2(adhoc)`，`TeamIdentifier=not set`）——与主程序同一种身份，
  **没有被强行重签成别的**；
- 从 repo 之外的 cwd 冷启动包里的 helper：成功，且没有在 cwd 里长出数据目录；
- 本机 `/Applications` 里只有 `Aeroric.app`，**没有 Native Preview** ⇒ 这次冷启动就是
  "只装 Tauri、未装 Native Preview"那条验收的真实形态。

## 4. Action 5 的边界（未执行，需用户同意）

计划原文："**用户同意后才**关旧写入进程、备份真实目录、校验备份、启动 host；备份含配置/任务/
notes DB 及 WAL/必要附件，以停写/DB backup API 保证一致性，不直接 cp 正在写的 DB。"

本轮**只做合成数据**（用户 2026-10-07 明确选择）。真实数据那一步的前置条件与检查项（供真正
执行时逐条对照，本文件不代替授权）：

1. **停写**：先让所有写者退出（Tauri 与 Native Preview 都关；确认 `~/.aeroric/.write.lock` 的
   持有者进程已消失），**不 kill 未确认的 PID**（ADR-027）。
2. **备份**：`settings.json`、`projects.json`、`projects/**`、`runtime.json`、`notes*` DB 与其
   `-wal`/`-shm`、`agent-homes/**` 的必要附件；DB 用 backup API 或先停写，**不直接 cp 正在写的
   文件**。
3. **校验备份**：逐文件 `sha256` 与体积；DB 用 `PRAGMA integrity_check`；抽查任务数与项目数。
4. **启动 host**：`aeroric-cli --data-dir ~/.aeroric serve`，确认 `host.token` 与 `host.sock`
   就位、`doctor --json` 报 `writerEligible: true`。
5. **回退**：把 Tauri 指回同一目录即可（数据原位、格式未变）；**不得**用 `git reset --hard` 或
   覆盖用户新写入的数据当"回滚"。
6. **失败判据**：任何一步与预期不符就停在那一步，保留现场（`.write.lock`、两个 client 的日志、
   `doctor --json` 输出），不要"再试一次看看"。

## 5. 已知边界（如实）

- 演练**不启动 GUI**：`old reader`（Tauri 客户端读同一目录）这一步需要人开着窗口看一眼——
  脚本能证明的是"数据形状没变"，不是"界面渲染正确"。
- 合成夹具的字段集来自 ADR-037 与 `storage.rs`/`storage.ts` 的对照，**不是**真实用户数据的
  抽样；真实数据的多样性（空项目、超长会话、损坏记录、云盘目录）不在合成覆盖里。
- bundle 段是 opt-in：`native:check cutover` 默认不跑它（要 30+ 分钟构建），跑过之后必须显式
  设 `AERORIC_TAURI_BUNDLE=1` 才有那 6 条 check。
