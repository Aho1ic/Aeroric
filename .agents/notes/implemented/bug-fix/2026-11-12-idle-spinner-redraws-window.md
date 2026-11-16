# Agent Note: 运行中转圈不再逐帧重排整窗

Status: implemented

## Problem

2026-11-12 排查审阅版经常卡顿：侧栏只要有一个会话在运行（一个转圈），App 空闲时 CPU 就在 96.6%，主线程忙碌 97%。原因有四处：

- 转圈和工具行扫光用 `TimelineView(.animation)` 驱动，每一帧都让整个窗口重新布局。
- `WorkspaceRootView` 的侧栏、更多菜单、标题探针共用同一份读取，任务、导入、"需要我处理"任何一处变化都让整窗重算。
- `LiveSessionStore` 每次读历史都重新分回合；轮询在回合中每 50ms 一次，`@Observable` 对同值赋值也会通知，于是每次轮询都触发重绘。
- JSONL 分帧每来一块数据都从头扫描：1MB 的帧按 16KB 分块到达，Debug 下要 775ms。

## Decision

- 转圈与工具行扫光改用 CoreAnimation 层动画（`Design/Components/LayerAnimations.swift`），不再由 SwiftUI 每帧驱动布局；开启"减少动态效果"时保持静止。
- `WorkspaceRootView` 的侧栏、更多菜单、标题探针各自读取自己需要的 `sidebarItems`。
- `LiveSessionStore` 读历史时分好回合并缓存，页面直接读；轮询只在值变化时写回 `model` 和 `approvals`（`retainApprovals`）。导入会话页载入时只分一次回合。
- JSONL 分帧记住已扫描到的位置，用 `memchr` 找换行：同样的 1MB 帧降到 0.2ms。
- 2026-11-16 并入 main 时与 [A2](./2026-11-16-a2-approval-answer-must-arrive.md) 的 `syncSendFailures` 汇合：它同样改为先在副本上应用、变了才写回 `approvals`，否则每次轮询都会把这次修复抵消掉。

## Alternatives considered

无。原提交没有记录考虑过的其他做法；并入 main 时也没有新的取舍。

## Consequences

- **收益**：一个转圈时 App 空闲 CPU 从 96.6% 降到 0.2%，主线程忙碌从 97% 降到 0%（2026-11-12 在 Mac 上测）。
- **代价**：转圈和扫光不再是 SwiftUI 视图，样式要改 `LayerAnimations.swift`；以后在轮询路径上新加的状态都要遵守"变了才写"，否则会重新引入整页重绘。
- **验证**：`PackageSmokeTests`（JSONL 分帧）、`ConversationModelTests`（回合缓存）随原提交加入；并入 main 后 macOS 包全部测试与 App 编译重跑。界面效果未目检，记在路线文档 §8 待目检清单。
