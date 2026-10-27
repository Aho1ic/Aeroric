# Agent Note: 门禁用例地板与正控，绿灯不等于工件完好

Status: implemented

## Problem

`pnpm native:check <module>` 跑 `scripts/native/checks.json` 里登记的 case。一个 runner 执行 0 个用例也能退出 0；一次看似只删空行的编辑把 `#[test]` 并进了上一行文档注释，测试停止运行却没有任何失败信号；静态扫描"没匹配上"既可能是干净，也可能是根本没读到文件。绿灯因此不能证明工件完好。

## Decision

- 每个 case 存完整展开的 argv（不用 shell 字符串）、超时、最少非零用例数（`minCases`）与登记它的任务。
- `runner.floors` 冻结每个 case 的最少用例数，只升不降；与 `minCases` 同一次编辑才允许改，改动要附理由与新的实测值。
- 静态扫描必须带**正控**：每个被扫路径至少有一条断言证明扫描确实读到了内容。
- skip 不算通过；被暂停的检查记为 NOT RUN / DEFERRED。
- git 语义的验收归属：`git` 模块用同一套 target 参数化 fixtures 覆盖 local/ssh 语义一致性；`connections` 只测 SSH 适配反例（主机密钥、非 ASCII/空格/前导 `-` 的远端路径、quoting、断线后先 reconcile 再重放）；能力门控反例归 `platforms`。其它模块不另写一份 git 行为断言。

## Alternatives considered

- **只看退出码**：runner 最简单。否掉是因为 0 用例与漏跑都会退出 0。
- **每个模块各自完整测试 git 行为**：阶段出口最干净。否掉是因为同一语义三处实现会各自漂移，漂移只能靠人读 diff 发现。

## Consequences

- **收益**：用例数下降、扫描读空、测试被意外注释掉都会让门禁变红。
- **代价与已知上限**：每次合法地删减用例都要同步改地板并说明理由；`checks.json` 本身体量很大，维护它是一项持续成本。
