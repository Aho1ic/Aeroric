# Agent Note: 新界面由用户在签名构建上逐屏审批

Status: implemented

## Problem

新界面或大改的界面需要用户视觉确认。浏览器 mockup 与真实 SwiftUI 的材质、密度、Liquid Glass 效果差距大，确认了 mockup 不代表确认了产品；而"已实现""已打包"也常被误当作"已批准"。

## Decision

- 新界面或大改的界面直接用 SwiftUI 实现，用户在**签名的 App 构建**上审阅；用户豁免了浏览器 mockup。
- 只有用户明确批准某个构建后，该屏才算批准。"implemented" 或 "packaged" 不等于 "approved"；记录屏、构建与状态。
- 产品 UI 是 SwiftUI + Apple 原生 Liquid Glass。本规则不解除 [UI 自动化暂停](2026-10-19-ui-automation-pause.md)：Agent 不自行启动 App 或运行 UI 自动化。

## Alternatives considered

- **先出浏览器 mockup，确认后再实现**：改设计成本低。否掉是因为 mockup 无法呈现原生材质与系统控件，确认结果不可信，用户已明确豁免。

## Consequences

- **收益**：批准对象就是要发布的东西。
- **代价与已知上限**：每次视觉迭代都要打一个签名构建；签名需要用户本人在 Mac 终端里使用钥匙串。
