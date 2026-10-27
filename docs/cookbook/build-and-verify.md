# 构建与验证

## 环境

| 工具 | 要求 |
|---|---|
| Xcode / Swift | Xcode 27、Swift 6.4（开发机实测） |
| Rust | `core/Cargo.toml` 的 `rust-version`（1.93） |
| Node | 24+；只跑 `scripts/native/` 与笔记校验脚本，无 npm 依赖。`pnpm` 只是脚本入口 |
| XcodeGen | 2.46.0（`scripts/native/doctor.mjs` 校验版本） |
| dbx | 仓库外的 `../dbx`，版本钉在 `scripts/dbx-ref.txt`；`scripts/prepare-dbx.sh` 打 `patches/dbx-security.patch` |

`pnpm native:doctor` 检查以上全部。

## 常用命令

```bash
pnpm native:doctor                       # 环境检查
pnpm native:generate                     # XcodeGen 生成 .xcodeproj
pnpm native:build                        # 构建并本地签名 AeroricNative.app
pnpm native:check <module>               # 跑 scripts/native/checks.json 中的门禁模块
pnpm native:contracts --check            # 线协议 schema 与 Rust 生成结果逐字节比对
pnpm native:digest                       # 重写 docs/native/digest/
pnpm verify-notes                        # 校验 .agents/notes/
cargo test --manifest-path core/Cargo.toml --workspace
cd core && cargo fmt --check
swift test --package-path macos/AeroricKit [--filter <Suite>]
swift test --package-path ios/AeroricCompanionKit
```

## 门禁

`scripts/native/checks.json` 登记每个模块的 case：完整 argv、超时、`minCases` 与冻结的 `runner.floors`。runner 是 `scripts/native/check.mjs`。规则见 [门禁地板与正控](../../.agents/notes/implemented/testing/2026-09-21-gate-floors-and-positive-controls.md)。

- 只跑与改动相关的检查，报告实际结果；编译通过与方法数不证明用户路径可用。
- 真实窗口 UI 自动化暂停：`pnpm native:ui-test`、`pnpm native:check accessibility`、XCUITest 不运行，记为 NOT RUN。见 [UI 自动化暂停](../../.agents/notes/implemented/process/2026-10-19-ui-automation-pause.md)。
- 测试使用隔离数据根与假 Agent（`scripts/native/fake-claude.py`、`fake-codex-app-server.py`），不碰真实 agent home 与凭据。

## 线协议改动

1. 改 `aeroric-protocol` 类型或 `docs/native/command-map.json` / `event-topics.json`。
2. `cargo run --manifest-path core/Cargo.toml -p aeroric-protocol --bin export-schema -- --out .` 重新生成 `contracts/native/v1/**`。
3. 同步 Swift `AeroricProtocol` 与金样测试，跑 `pnpm native:check protocol`。
