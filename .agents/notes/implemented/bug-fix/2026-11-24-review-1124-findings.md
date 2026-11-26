# Agent Note: 目检 1124 查出的两处：宿主不提供远程控制方法、委派默认只读没说清

Status: implemented

## Problem

目检报告在 `~/Documents/computer-use/2026-11-24-0019-aeroric-review-1124/report.md`。测的是 `8730cefe` 之前的构建（宿主 2026-11-23 21:56）。

**通过的：**
- A1：新委派能选 Agent 配置和模型；
- A2：委派卡按钮的辅助功能标识、全部通过时检查点小标题写“义务 · 已通过 1/1”。

**没测成的：**
- **审查卡的按钮标识：** 点了“请人审查”，没等到审查卡；
- **手机端全部：** 模拟器打不开。Xcode 提示 Simulator 需要的系统组件还没装，要用户在 Xcode 里装；
- **B2、B3、B5：** 宿主构建早于手机相关的宿主改动。

**查出的两处：**

1. **宿主不认 `remote.*`：** `remote.status`、`remote.start`、`remote.createInvite`、`remote.stop`、`remote.listDevices` 都回 `UnsupportedCapability`。
   - **原因：** `aeroric-cli serve`（App 用的宿主）构建分发器时，`HostWiring.remote` 一直是空的。只有远程访问已经开着（`remote-config.json` 的 `enabled`）时，`serve` 才建远程状态和句柄，而且是在分发器建好之后才建，只用来启动 WS 服务。
   - **后果：**
     - App 和 CLI 都调不到这组控制方法，远程访问打不开，也配不了对；
     - 本批新加的“设置 › 远程与手机”页同样用不了。
   - **只在测试里接通过：** `ap6_push_plane.rs` 测试里把 `RemotePlaneInjection` 填上了，所以测试一直是过的。
2. **委派默认只读，选择器没说：**
   - 新委派没选角色、也没选工作区时，子会话是只读工作区、`plan` 权限。这是有意的设计（`Workspace` 默认 `ReadOnly`）。
   - 但工作区选择器写的是“工作区按角色”，看不出会是只读。目检里题目要子会话写一个文件，子会话照规则没写。

## Decision

**宿主总是提供远程控制方法（`aeroric-cli` 的 `serve`）。**
- **先建状态和句柄：** 远程状态、手机事件桥、`CliRemoteHandle` 在建分发器之前就建好，并通过 `HostWiring.remote` 注册 `remote.*`；
- **WS 服务照旧：** 只在配置里开着时启动，或者 App 调 `remote.start` 时由 `ensure_started` 启动。
- **两处“先有鸡还是先有蛋”：**
  - **宿主服务：** 句柄的 `host_services` 改成 `HostServicesCell`（`OnceLock`），分发器建好后再填进去。手机发消息走的 `session.prompt` 用的就是它；
  - **审批表：** 句柄和分发器要共用同一张审批表。`HostWiring` 加一个可选的 `approvals`，`serve` 先建好审批表交给两边；不给时和原来一样新建一张。
- **事件桥：** 手机事件桥总是挂上。没有手机连着时，它什么也不发。

**委派的工作区选择器写明子会话会拿到什么（`WorkPlanText.draftWorkspace`）：**
- 没选角色也没选工作区：写“只读（默认）”；
- 选了角色：写“按角色：在副本里写”这类，按角色的工作区；
- 选了工作区：照旧。

## Alternatives considered

**远程关着时只注册 `remote.status` 和 `remote.start`。** 没选：配对、设备列表、撤销设备在远程关着时也该能查、能撤销，而且整组方法本来就是给本机 App 用的（只走本机 UDS）。

**新委派默认改成可写。** 没选：默认只读是委派的安全设计（子会话不能悄悄改主会话的目录）。只让界面说清楚。

## Consequences

- **收益：**
  - App 的“远程与手机”页能读状态、打开远程访问、生成配对二维码、管理设备；
  - 新委派一眼能看出子会话是不是只读。
- **局限：**
  - 本机还没有装 Simulator 组件，手机端仍要等装好后再测；
  - 远程控制方法和以前一样，只走本机 UDS，没有其他限制。
- **验证：**
  - CLI 端到端测试 `approval_and_task_lifecycle_real_cli_e2e` 加了一步：远程关着时 `remote.status` 能回答，`running` 为假；
  - 宿主两套远程集成测试、`remote_headless` 单元测试都过；
  - Swift 新增 1 个测试：工作区选择器的写法。
