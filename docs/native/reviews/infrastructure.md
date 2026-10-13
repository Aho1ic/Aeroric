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

## 6. P8-05 slice 111 — 本地 Conda 宿主化（2026-10-12）

本节是增量独立代码审查与机器证据，不改写以上 P7-03 历史验收，也不是
P8-05 人工／grill 收口。

| 审查字段 | 本轮记录 |
|---|---|
| 范围 | core Conda DTO／候选表／进程与解析器；HostWiring＋UDS；延迟Tauri命令；共用fake夹具、core／host／CLI／actual command测试 |
| 基线 | main，HEAD `6ea745030318`；入场 platform/mod.rs 用户WIP保持原hash，相邻DBX不动 |
| 审查者 | 独立只读 reviewer `01a12fe3-d492-7ef0-b453-85ff2e8b0bfb`，对照 HEAD 的旧 conda.rs，不以主代理自测作通过证据 |
| 发现与处理 | 无有充分证据、可行动的正确性／安全／并发／契约问题；没有需要合入的审查修复 |
| 快照 | `.artifacts/native/p8-05-slice111-20261012/111-review-status.txt` 与 `111-review.patch`；patch SHA256 `0d8e4962bfe07819be048f2ab0e17847ab8289562a4ff7f9dbb27cad50c0b859`（排除用户WIP，包含新增源码与测试） |
| 主代理行为证据 | core policy 7/0、fake进程4/0、原infrastructure6/0；认证UDS8/0、实际桌面命令＋远端构建器8/0、真实attach CLI4/0。宿主未注册与桌面错误placement各先证0/1行为红，接线后复绿 |
| 限制／后续 | reviewer未运行测试；Windows及生产IPC/UI未执行。全链路只用owned fake，不宣称已测真实安装的Conda；旧阻塞探测仍无deadline/cancel。SSH孪生继续Tauri，G43 open；最后门禁与续跑指针以计划slice111清单为准 |

实现与边界：`infrastructure::conda_face` 是本地业务的唯一持有者；旧native
可执行路径命名API仍按旧输入工作。`CondaProbe` 不可由RPC反序列化，显式
空候选禁止fallback；子进程环境可注入但不改全局rc。宿主严格无参准入在
进程前，认证在handler前；查询不要求writer。External只转发，错误／坏响应／
断连均不运行本地探测。没有提交／推送／真实数据切换或平台、签名改动。
