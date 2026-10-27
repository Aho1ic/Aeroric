# 外部 Agent 适配器契约(AP7 / P9-01)

外部 Agent 适配的基本要求是「每个 advertised 能力绑定具体 adapter/版本/边界,
抓 TUI 文本伪造协议被明确禁止」。本文是那条契约的落点。

## 1. 能力从哪来:探测,不是猜测

`aeroric_protocol::capability` 的设计前提是 **`v == 1` 从不蕴含能力**。所以:

- 来源只能是**结构化事实**:`adapters.capabilities` 里那份由
  `CapabilityProbe` 收集、`probe_agent_capabilities()` 翻译的 `AgentCapabilities`;
- 事实本身取自 core 内**钉在源码 commit 上**的方法表
  (`aeroric_core::agents::dsh_host::client::REMOTE_METHODS`,连同 `PACKAGE_VERSION`)——
  不是运行时截屏、不是终端文本匹配;
- 探不到就是 `false`,并附 `extra.unsupported` 列出**具体**缺哪个方法。
  宿主握手里的 `session-controls` 能力带 `note`,写清适配器名、版本与已广告的方法名。

**禁止**:解析 TUI 输出推断出「这个族大概支持 fork」。任何这类实现都违背本契约,
因为它把一个显示细节当成了协议事实。

## 2. 未广告 = 服务端拒绝

三道门,顺序不可颠倒:

| 层 | 行为 | 证据 |
|---|---|---|
| UI | 未广告的控件**不渲染** | `macos/.../AgentTools/AgentToolsViews.swift`(`store.offeredControls`) |
| 客户端 | 未广告的请求**构造不出** | `AgentToolsStore.makeRequest` 返回 `nil`;iOS `CompanionControlRequest.refusal` |
| 宿主 | 未广告的调用**被拒绝** | `SessionControls::authorize` → `ControlError::Unsupported` → `UnsupportedCapability` |

UI 隐藏只是体验层。**安全边界在宿主**:
`node scripts/native/check.mjs session-controls` 的 host 用例直接绕过 UI 调
`session.control` 并断言拿到 `UnsupportedCapability`。

方法与 `session.*` 字面量在 Swift/iOS/Rust 三处必须一致:宿主
`SessionCapability::method()`,iOS `CompanionSessionControl.rawValue`,macOS
`AdapterClient.CompanionControlName.rawValue`。

## 3. 端口画在哪

沿用项目既有的 app-owned port 模板(与 AP6 的 `PushTransport`、P3-05 的
`DshHostPorts` 同形):

| 关注点 | 归属 | 理由 |
|---|---|---|
| 能力探测与策略 | core(`aeroric_core::adapters`) | 纯逻辑,可单测,不碰 I/O |
| HTTP / 进程 / 麦克风 | 宿主端口 | core 不阻塞、不持有平台资源 |
| 展示与触发 | SwiftUI | 只读 store;`AgentToolsStore` 不做裁决 |

## 4. 各家能力现状(如实记账,2026-10-13)

| 能力 | DSH 上游 | 本仓库暴露 |
|---|---|---|
| `plan` | 无 `session.plan` 方法 | 未广告(UI 不显示) |
| `compact` | 无 | 未广告 |
| `fork` | `session.fork` | 已接(`session.control` → 宿主) |
| `revert` | 无方法;策略层已实现(会话/工作区分流 + 预览门槛) | 策略可用,上游调用待适配器 |
| `share` | 无方法;脱敏导出已实现 | 本地脱敏可用,公开上传需 endpoint+确认 |
| 子代理 | `subagents.list` / `subagents.prompt` / `subagents.interruptByParent` | core API 已存在;宿主面逐步接 |
| goal | `goals.create|edit|pause|resume|complete|clear|get` | 同上 |

未广告的项**不从清单里删除、也不放占位按钮**:它们在
`extra.unsupported` 与本节表里具名存在,等适配器补齐后自动变为可用。

## 5. 具名阻塞

- `GAP-AP7-EXTERNAL`:ACP 对端、OAuth 真实账号、媒体 provider 的线上验证需要用户
  授权与凭据。代码侧已实现 fail-closed 语义(版本交集、越界目录拒绝、不支持音频即
  拒绝整条消息、沙箱未就绪不执行),但**未经真实对端验证**。
- macOS 麦克风/屏幕录制/辅助功能的**实际授权**由用户在系统设置完成;
  原生端只提供指路与签名不稳定时的重新授权提示。
