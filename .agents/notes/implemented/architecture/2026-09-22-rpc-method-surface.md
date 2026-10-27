# Agent Note: 本地 RPC 方法面——服务面分组命名、孪生并入 target 参数、能力由宿主声明

Status: implemented

## Problem

本地协议 v1 的方法名一旦发布就冻结：改名、删方法都是协议变更，Swift 客户端、CLI、iOS/relay 都按名字调用。方法面从旧桌面端的 660 条命令清点而来，其中同一行为常以本地、`remote_*`、`wsl_*` 三个名字出现；平台特定命令起初没有线名；DSH 的 66 条能力里 41 条找不到调用者。不定规则，方法面要么缺一块，要么同一行为在契约里出现三次，要么塞满无人调用的名字。

## Decision

- **命名**：`rpc_method = <服务面>.<旧命令名>`（如 `git.git_status`、`terminal.send_input`）。服务面前缀由 `docs/native/command-map.json` 的 `meta.rpc_resolution` 推导：`module_to_service` 映射加带 `why` 的 `overrides`。凡"归属"类字段（`category`、`rpc_method`、`core_service`、`check_module`、`caller_evidence`）的值必须能由登记的推导复现，否则门禁失败。服务面拆分或改名是协议变更。
- **孪生收敛**：`remote_*` / `wsl_*` 孪生不进入方法面，由父方法的 `params.target: ProjectTarget`（`local` / `ssh` / `wsl`）表达传输目标。旧名降级为 `twinAliases` 别名表（`legacy → {method, targetKind}`），别名表内的改动不是协议变更，新增/删除方法才是。参数形状未核实的别名标 `pending: <task>`，宿主对其返回 `INVALID_ARGUMENT`。
- **能力门控**：平台特定方法进入方法面，并在 schema 上带 `requiresCapability`。**能力由宿主声明，客户端不得用自己所在的 OS 推断宿主能力。** 宿主未声明时客户端禁用控件并显示原因，服务端同时拒绝（`UNSUPPORTED_CAPABILITY`）。门控键是 `(方法, target)`：只有任何 target 都需要该能力的方法才在方法上声明，孪生的能力要求随别名走。
- **DSH 能力面**：全部 typed 入表（服务面 `dsh`）；没有调用者的方法在 `command-map.json` 上带 `caller_evidence`，取值只有 `proven: <证据>` 或 `pending: <task>`，两者皆无即门禁失败。"schema 里有、门禁绿"不算已验收。
- 线协议 schema 由 Rust 生成：`core/crates/aeroric-protocol/src/bin/export-schema.rs` 读 `command-map.json` 写 `contracts/native/v1/**`，`pnpm native:check protocol` 的 `contracts-current` 逐字节比对，漂移即红。

## Alternatives considered

- **源模块前缀（`pty.run_task`）**：最强理由是零映射表、纯规则自证。否掉是因为线名会绑定源码文件布局，而模块拆分恰恰频繁发生，届时要么规则失效、要么被迫改协议名。
- **去冗余前缀（`git.git_status` → `git.status`）**：名字更短，实测也零碰撞。否掉是因为叶子不再逐字等于旧命令名，丢掉了与冻结清单逐条对账的锚点。
- **先保留 641 个名字、以后再收敛孪生**：当下改动最小。否掉是因为以后删方法就是协议变更，现在能做的清理会变成将来必须走决策的破坏。
- **平台命令永久排除在协议外**：协议更小。否掉是因为这是产品范围收缩，不能由协议设计顺带决定；远程 Windows 宿主将来可以声明 `wsl=true` 复用同一份协议。
- **只冻结有调用证据的 DSH 方法**：协议面只含被证明的部分。否掉是因为每补一条都是协议变更，能力声明随之抖动。

## Consequences

- **收益**：方法面规模可机械复算；同一行为只有一个方法名；"宿主不支持"在协议层就有明确拒绝与原因，不靠客户端猜。
- **代价与已知上限**：`meta.rpc_resolution` 与别名表是协议的一部分，改动要按协议变更评审；按 `(方法, target)` 门控让宿主分派多一层判断。若将来需要 v2 方法面，先写新 Note（见 [协议版本边界](2026-09-22-protocol-version-boundary.md)）。
