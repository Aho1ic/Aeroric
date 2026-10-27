# macOS App

SwiftUI 应用，Xcode 工程由 XcodeGen 从 `macos/project.yml` 生成（`.xcodeproj` 不入库）。最低部署 macOS 14.0；macOS 26 起的 Liquid Glass 用 `#available` 门控。

## 结构

| 路径 | 内容 |
|---|---|
| `macos/AeroricApp/Sources` | App 入口、菜单、`AppServices`（装配客户端与宿主监管）、任务工作区、结构化会话 |
| `macos/AeroricKit/Sources/AeroricProtocol` | 线协议的 Swift 类型（与 `contracts/native/v1` 对齐，`ProtocolGoldenTests` 核对） |
| `macos/AeroricKit/Sources/AeroricClient` | `HostConnection`（UDS）、`HostSupervisor`、`RequestRegistry`、`EventReducer`、各服务客户端 |
| `macos/AeroricKit/Sources/AeroricUI` | 各工作面界面；`Design/` 是设计原语（`NativeSelection.swift`、`GlassPanel.swift` 等） |
| `macos/AeroricUITests` | XCUITest（UI 自动化暂停中，不运行） |
| `macos/SandboxProbe` | 沙盒可行性探针 |

## 界面规则

- 导航：分组侧栏，见 [分组侧栏](../../.agents/notes/implemented/feature/2026-10-23-grouped-sidebar-navigation.md)。
- 相邻互斥选择用系统选择态或 `NativeSelection.swift` 的封装，新变体加在那里。
- 用语义色与既有设计原语；尊重减少透明度、增强对比度、减少动态效果。
- 界面不拼 Agent argv、不解析会话 JSONL、不直写共享数据；全部经 `AeroricClient` 调宿主。
- 新界面或大改的界面由用户在签名构建上审批，见 [逐屏审批](../../.agents/notes/implemented/process/2026-10-23-screen-approval-by-signed-build.md)。

## 测试

`swift test --package-path macos/AeroricKit [--filter <Suite>]`；App 编译检查用 `xcodebuild -scheme AeroricNative CODE_SIGNING_ALLOWED=NO build` 与 `build-for-testing`（只编译 UI 测试不运行）。
