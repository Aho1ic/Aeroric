# P7-03 评审记录 — Docker、端口与 Conda（2026-10-03 收口）

## 1. 范围

P7-03 是 Phase 7 第三个 Task：core `infrastructure/` 模块（Docker/端口/Conda）
＋Infrastructure 五视图＋门禁 2 case。

## 2. 实现链（两刀）

- **刀一（core infrastructure 模块）**：`core/crates/aeroric-core/src/
  infrastructure/{mod,docker,ports,conda}.rs`——docker（OperationRegistry
  **长任务 operationId/取消/状态查询**——Cancelled 后不可改；Docker 形状
  verbatim PascalCase 解析＋输出异常→空列表不 panic；**只读/破坏显式区分**；
  `daemon_unavailable_reason`——无 daemon 显示不可用不弹 sudo，permission
  denied 有因提示）＋ports（ListeningPort、**PID 身份判定**——stop 前比对
  进程名避免 PID 重用误杀、`can_stop_port` owner/stop 显式区分）＋conda
  （环境发现 cfg 候选路径、`env_name_from_path` 推导、环境路径校验、**spawn
  环境白名单不能污染全局 shell rc**）。`tests/infrastructure.rs` 6 用例。
- **刀二（五视图）**：`AeroricUI/Infrastructure/{InfrastructureStore,
  InfrastructureViews}.swift`——InfrastructureStore（容器/镜像/端口/Conda
  簿记＋daemon 不可用原因＋长任务状态）＋DockerPanel＋ImageList＋
  ContainerDetail（**Remove 二段确认**）＋PortInspector（Owner/Stop 二段
  确认——PID 身份由宿主校验）＋CondaEnvironmentPicker。`InfrastructureStoreTests`(5)。

## 3. 验证实测（2026-10-03）

| 验收 | 结果 |
|---|---|
| `pnpm native:check infrastructure`（新注册） | 2 case / 11 用例 ok（infrastructure 6＋InfrastructureStoreTests 5） |
| `cargo test -p aeroric-core` | 552 passed / 0 failed |
| `swift test --package-path macos/AeroricKit` | 190 passed / 0 failed |
| `xcodebuild -scheme AeroricNative build` | BUILD SUCCEEDED |
| `inventory --check` | consistent（infrastructure floor 双写） |
| `doctor` | 18/18 |
| `smoke-headless.mjs` | rc 0 |

## 4. 验收原文逐条对照

- `tests/infrastructure.rs`(6)＋`InfrastructureStoreTests.swift`(5) ✅；`pnpm
  native:check infrastructure` ✅。
- fake CLI 覆盖：unavailable（daemon_unavailable_reason）✅、permission
  denied ✅、输出异常（非法 JSON→空列表不 panic）✅、取消（OperationRegistry
  Cancelled 后不可改）✅；端口释放前确认 PID 启动身份，避免 PID 重用误杀
  （`pid_matches_process` 单测）✅。
- `grill-me [infrastructure]`：已真实调用（用户：接受并收口）✅。

## 5. 已记录的边界与挂账

- 可选真 Docker smoke（随机 test label 临时资源 finally 清理）→ P10-01
  压力矩阵（需 Docker daemon；fake 层已覆盖 unavailable/permission denied/
  输出异常/取消）（用户确认记账）。
- Docker/Conda CLI 真实执行 → 宿主接线（P7 收尾或 P10-01）。
