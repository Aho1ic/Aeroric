# Agent Note: 原生边界——业务归 Rust core，一个数据目录一个写者

Status: implemented

## Problem

Aeroric 同时有 SwiftUI macOS App、iOS 伴随端、`aeroric-cli` 三个入口，它们都可能想读写同一个数据目录 `~/.aeroric`，也都可能想自己拼 Agent 启动参数、自己解析会话文件。若各端各写一套，同一条业务规则会在 Swift 与 Rust 里各漂一份，两个进程同时写同一目录会静默覆盖用户数据。

## Decision

- 业务逻辑与写操作只在 Rust `core/`（`aeroric-core` 服务 + `aeroric-host` 宿主进程）。宿主是独立进程，App 经 UDS 连接；SwiftUI 只读 read model、发 typed 动作。
- SwiftUI **不得**拼 Agent argv、**不得**解析原始会话 JSONL、**不得**直写共享业务数据。`aeroric-core` 里不出现 UI 类型。
- 一个数据目录同时只有一个业务写者（INV-03），由 `storage_lock` 的数据目录租约保证。拿不到租约时返回 `AlreadyInUse` 并显示持有者与安全退出方式；**绝不**自动 kill 持有者、**绝不**删锁文件后继续。
- 能力分类：协议方法由 core 服务实现并经宿主暴露；`host_local_os_ui` 一类（打开 Finder、系统对话框等）由客户端适配层实现，没有线名，CLI 侧返回 `INTERACTIVE_UI_REQUIRED`。
- 最低部署系统 macOS 14.0（`macos/project.yml`、`macos/AeroricKit/Package.swift` 写死）。macOS 26 起的 Liquid Glass（`glassEffect`）只作 `#available` 门控的增强，且只放在可交互控件上；窗口底用 `NSVisualEffectView` / SwiftUI `Material`，终端与编辑器用足够不透明的底。
- 相邻互斥的导航、tab、分段选择用系统选择态（`Picker`、`List(selection:)`、`Toggle`、`Menu`）或 `macos/AeroricKit/Sources/AeroricUI/Design/NativeSelection.swift` 的封装；禁止手绘相邻项的 active 底纹或下划线动画。独立开关、折叠按钮不在此列。
- 外观 glass/classic 是外观偏好，与任何交互/会话设置互不复用开关；reduced transparency / increased contrast / reduced motion 三个系统设置必须真实改变表现。
- OMP 历史只从托管根 `~/.aeroric/agent-homes/**` 发现，**不**读写用户自己的 `~/.omp`。解禁外部历史导入需要单独审定，且只能是只读、仅导入用户显式选中的文件、不以 `readdir ~/.omp/profiles` 作为发现手段。

## Alternatives considered

- **Swift 端直接实现部分业务（例如本地读会话文件做展示）**：最强理由是少一次 RPC、界面响应更快。否掉是因为会话格式、四族差异和写入规则只要有第二份实现就会漂移，且 iOS 端无法复用 Swift 里的这份逻辑，最终还是要在 core 再写一遍。
- **最低部署 macOS 13**：可覆盖更多老机器。否掉是因为 `@Observable`/Observation 与严格并发的可用面都要求 14，13 会迫使 store 退回 `ObservableObject`。
- **把外部 `~/.omp` 历史默认读进来**：用户能直接看到自己手动跑的 omp 历史。否掉是因为它违反"不碰用户 agent home"的承诺，且 `readdir ~/.omp/profiles` 本身就是信息读取。

## Consequences

- **收益**：业务规则只有一处实现，Swift 与 iOS 端变薄；数据目录不会被两个进程同时写坏；UI 选中态行为一致且自带无障碍。
- **代价与已知上限**：每加一个界面能力都要先在 core/host 加方法与契约；13.x 用户无法使用；原生侧看不到用户在 Aeroric 之外用系统 omp 跑出的历史。若出现必须在客户端本地完成、且无法经宿主表达的能力，需要新 Note 讨论而不是在 Swift 里悄悄实现。
