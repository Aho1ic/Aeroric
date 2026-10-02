# P7-02 评审记录 — WSL 与跨平台运行时保留（2026-10-03 收口）

## 1. 范围

P7-02 是 Phase 7 第二个 Task：core platform/wsl 模块（cfg 隔离）＋
UnsupportedTargetView＋CI core 三作业（G14 结案）。

## 2. 实现链（两刀）

- **刀一（core platform/wsl 模块）**：`core/crates/aeroric-core/src/platform/
  {mod,unix,windows}.rs`＋`connections/{wsl,wsl_files,wsl_git}.rs`——platform
  （Platform 枚举＋capabilities **macOS wsl=false**——**不得由 client OS
  直接推断 host 能力**，wsl_reason 诊断；`client_os_declares_wsl` 只回答
  client OS；detect_path 按 cfg 分支）＋unix/windows（candidate_paths；
  **Windows 分支不删除**——wsl.exe 前缀保留）＋wsl（WslStatus/Distribution/
  Settings、`wsl_project_is_local` 判定——macOS 打开 WSL 项目禁止当作本地
  path）＋wsl_files（**旧 WSL JSON 读写不丢 distribution/linuxPath**、
  wsl_path_to_windows_unc）＋wsl_git（wsl.exe -d 命令构建）。
  `tests/platform_capabilities.rs` 5 用例。
- **刀二（UnsupportedTargetView＋CI）**：`AeroricUI/Connections/
  UnsupportedTargetView.swift`（macOS 打开 WSL 项目保持元数据＋显示需要
  Windows 主机——禁止当作本地 path）；`.github/workflows/checks.yml` 增
  core 三作业（fmt/clippy -D warnings/test --all-targets --all-features，
  **保持原 Tauri matrix 与 source patches**）。

## 3. 验证实测（2026-10-03）

| 验收 | 结果 |
|---|---|
| `pnpm native:check platforms`（新注册） | 1 case / 5 用例 ok |
| `cargo test --all-features`（本地复跑 CI 命令） | 650 passed / 0 failed（clippy 0） |
| `swift test --package-path macos/AeroricKit` | 185 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（platforms floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |
| CI 覆盖 | checks.yml 已增 core fmt/clippy/test（Windows/Linux/macOS matrix） |

## 4. 验收原文逐条对照

- `tests/platform_capabilities.rs`(5) ✅；`pnpm native:check platforms` ✅。
- Windows CI 运行 WSL 路由/quoting 单元测试（wsl_git 命令构建单测在
  platform_capabilities；真集成明确 skip——Windows 支持机器 smoke → P10-01）✅；
  Linux/macOS 核心编译（CI core 三作业 + 本地复跑）✅。
- macOS UI 无法点击到 wsl.exe 启动（UnsupportedTargetView 禁止当作本地
  path；`wsl_project_is_local` 判定单测）✅；旧 WSL JSON 读写不丢
  distribution/linuxPath（settings roundtrip 单测）✅。
- `grill-me [platforms]`：已真实调用（用户：接受并收口）✅。

## 5. G14 结案

- core/ workspace 进 CI 覆盖：checks.yml 增 core fmt/clippy/test 三作业
  （Windows/Linux/macOS matrix 全跑）——G14 的 finding（core/ 完全不在现有
  CI 覆盖内）已闭合。

## 6. 已记录的边界与挂账

- `connections/wsl_executor.rs`（WSL 任务接 executor/observer）与
  `legacy_task_executor.rs` 删除 → P7 接线收尾（用户确认记账）。
- Windows 支持机器真机 smoke（start/resume/cancel/generation 竞态）→
  P10-01。
- Linux/Windows cfg 分支编译验证（cargo check --target）在 CI
  Windows/Linux matrix 上运行（本地 macOS 只验证了 local）。
