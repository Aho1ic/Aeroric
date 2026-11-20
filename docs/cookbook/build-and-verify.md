# 构建与验证

## 环境

| 工具 | 要求 |
|---|---|
| Xcode / Swift | Xcode 27、Swift 6.4（开发机实测） |
| Rust | `core/Cargo.toml` 的 `rust-version`（1.93） |
| Node | 22.18+（笔记校验直接执行 `.ts`，需要默认的类型剥离；`native:doctor` 校验这一版本线），推荐 24；只跑 `scripts/native/` 与笔记校验脚本，无 npm 依赖。`pnpm` 只是脚本入口 |
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

## 在另一台 Mac 上跑

全量测试和打包吃满 CPU 和内存，可以放到另一台 Mac 上跑。`scripts/native/remote.sh` 在仓库所在的这台 Mac 上运行：它先把仓库和 `../dbx` 同步过去（git 忽略的路径都不传），再在远端执行。远端只放副本，提交照旧在本机做。

```bash
export AERORIC_REMOTE_HOST=user@host     # 必填；登录密钥默认 ~/.ssh/id_ed25519_mini
scripts/native/remote.sh verify          # fmt、cargo 全量、contracts、swift test、native:build、笔记与文档校验
scripts/native/remote.sh run '<命令>'     # 在远端仓库根目录跑一条命令
scripts/native/remote.sh build           # 远端 native:build，成功才把 .app 拷回本机同一路径
scripts/native/remote.sh status          # 远端磁盘余量与产物大小
```

- 远端的 Rust 版本由 rustup 钉成本机 `cargo --version` 的版本，两边的 `rustfmt` 和编译警告一致；要先在远端 `rustup toolchain install <版本> --profile minimal --component rustfmt`。
- `native:generate` 钉死 XcodeGen 2.46.0：远端在 `<目录>/tools/xcodegen-2.46.0/` 放一份（从本机 Homebrew 的 `Cellar/xcodegen/2.46.0` 原样拷过去），脚本把它排在 PATH 最前。
- 远端磁盘有限：编译不留增量缓存，调试信息只留行号；`core/target` 超过 `AERORIC_REMOTE_MAX_TARGET_GB`（默认 60）时先 `cargo clean`；每次结束报告磁盘余量。
- 同一时间只跑一次（远端 `.lock` 目录）。日志留在远端 `<目录>/logs/`。测试在远端临时目录里留下的 `aeroric-*` 目录，每次运行结束时删掉这次新建的。
- 本机打包是临时签名（ad-hoc），远端打包也是，拷回来的 .app 与本机打的等价。

## 门禁

`scripts/native/checks.json` 登记每个模块的 case：完整 argv、超时、`minCases` 与冻结的 `runner.floors`。runner 是 `scripts/native/check.mjs`。规则见 [门禁地板与正控](../../.agents/notes/implemented/testing/2026-09-21-gate-floors-and-positive-controls.md)。

- 只跑与改动相关的检查，报告实际结果；编译通过与方法数不证明用户路径可用。
- 真实窗口 UI 自动化暂停：`pnpm native:ui-test`、`pnpm native:check accessibility`、XCUITest 不运行，记为 NOT RUN。见 [UI 自动化暂停](../../.agents/notes/implemented/process/2026-10-19-ui-automation-pause.md)。
- 测试使用隔离数据根与假 Agent（`scripts/native/fake-claude.py`、`fake-codex-app-server.py`），不碰真实 agent home 与凭据。

## 线协议改动

1. 改 `aeroric-protocol` 类型或 `docs/native/command-map.json` / `event-topics.json`。
2. `cargo run --manifest-path core/Cargo.toml -p aeroric-protocol --bin export-schema -- --out .` 重新生成 `contracts/native/v1/**`。
3. 同步 Swift `AeroricProtocol` 与金样测试，跑 `pnpm native:check protocol`。
