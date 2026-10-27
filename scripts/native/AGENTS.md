# scripts/native/ 规则

构建、签名、门禁 runner、契约与 UI 测试工具。只用 Node 内置模块，无 npm 依赖；`pnpm` 只是入口。命令见 [docs/cookbook/build-and-verify.md](../../docs/cookbook/build-and-verify.md)。

- `checks.json` 每个 case 存完整 argv、超时、`minCases`；`runner.floors` 只升不降，与 `minCases` 同一次编辑才允许改，改时附理由与新实测值。静态扫描必须带正控。→ [门禁地板](../../.agents/notes/implemented/testing/2026-09-21-gate-floors-and-positive-controls.md)
- skip 不算通过；被暂停的检查记 NOT RUN。`ui-test.mjs`、`accessibility` 模块与 VM 相关入口处于暂停状态（`ui-test-policy.json`），不要运行或绕过。
- 测试只用隔离数据根与 `fake-claude.py`、`fake-codex-app-server.py` 等假 agent。
- `digest.mjs` 生成 `docs/native/digest/`；改 `feature-matrix.json` 后重跑 `pnpm native:digest`。
- 每个 `.mjs` 有同名 `.test.mjs` 时一并跑：`node --test scripts/native/<name>.test.mjs`。
