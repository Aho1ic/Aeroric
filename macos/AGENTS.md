# macos/ 规则

SwiftUI macOS App（`AeroricApp`）与 `AeroricKit` 包（`AeroricProtocol` / `AeroricClient` / `AeroricUI`）。现状见 [docs/subsystems/macos-app.md](../docs/subsystems/macos-app.md)。

- 界面只读 read model、经 `AeroricClient` 发 typed 动作；不拼 Agent argv、不解析会话 JSONL、不直写共享数据。
- 相邻互斥选择用系统选择态或 `AeroricKit/Sources/AeroricUI/Design/NativeSelection.swift`，新变体加在那里；不手绘选中底纹。
- 用语义色与 `Design/` 下的既有原语；尊重减少透明度、增强对比度、减少动态效果。最低 macOS 14，Liquid Glass 用 `#available` 门控，只放可交互控件上。
- 会话呈现只来自协议事件；未知内容带来源显示，绝不自动批准。
- `.xcodeproj` 由 `pnpm native:generate`（XcodeGen）从 `project.yml` 生成，不手改、不入库。
- 新界面或大改的界面由用户在签名构建上审批。UI 自动化暂停：`AeroricUITests` 只编译（`build-for-testing`），不运行。
- 验证：`swift test --package-path macos/AeroricKit [--filter <Suite>]`。
