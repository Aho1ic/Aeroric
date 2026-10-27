# Agent Note: 暂停真实窗口 UI 自动化与测试 VM

Status: implemented

## Problem

真实窗口的 UI 自动化（XCUITest、合成输入、App 预热）需要独占的测试 VM 与系统权限，曾占用两台 VM 并反复要求授权；Agent 在没有明确授权时自行重建 VM 或启动 App 跑 UI，会打断用户使用自己的 Mac。

## Decision

- 项目范围内暂停全部真实窗口/UI 自动化：不运行 XCUITest、`pnpm native:ui-test`、`pnpm native:check accessibility`、手动自动化、App 预热或等价的宿主/客户机 UI runner。
- 不创建、克隆、导入、下载、恢复、启动或自动重建测试 VM，也不为绕开暂停而重装 VM 工具。旧的任务级 VM 预留与 UI 权限不授权新的运行。
- 只允许经审查的无窗口、无输入的单元、fixture、静态与仅编译检查，使用隔离数据根与假 agent。不能因为 target 名里有 "unit" 就假定它不交互。
- 被暂停的检查一律记为 **NOT RUN / DEFERRED**，永不记为通过。
- 恢复真实 UI 或启用替代 VM 需要用户新的明确授权与一次策略修订。机器可读的门禁策略在 `scripts/native/ui-test-policy.json`，由 `scripts/native/ui-test-policy.mjs` 执行。

## Alternatives considered

- **允许 Agent 在用户 Mac 上直接跑 UI 测试**：不需要 VM。否掉是因为会抢占用户正在使用的桌面与权限。
- **保留 VM，按任务申请时段**：验证面更全。否掉是因为用户已决定释放两台 VM 并防止重建。

## Consequences

- **收益**：Agent 不会在无授权时占用桌面或资源；验收报告不会把未运行的 UI 检查写成通过。
- **代价与已知上限**：用户可见的交互回归只能由用户在签名构建上人工验收（见 [逐屏审批](2026-10-23-screen-approval-by-signed-build.md)）；无障碍门禁处于停跑状态。
