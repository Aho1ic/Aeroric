import AeroricClient
import Foundation
import SwiftUI

/// AP7:Agent 工具面(会话控制 + 外部协议)的簿记。
///
/// ## 这个 store 不做决定
///
/// 它保存**宿主探测结果**与**用户已经看到的证据**:哪些能力被广告、每次控制请求
/// 的裁决、哪些危险动作还没拿到确认。所有「能不能做」的判定都在宿主
/// (`aeroric_core::adapters`);客户端在这里只做两件事:
///
/// 1. 未广告的控件**不渲染**(避免用户点一个注定失败的按钮);
/// 2. 把宿主的拒绝原因原样展示,而不是翻译成含糊的失败。
///
/// ## 用户可见性优先
///
/// 危险动作先给证据:工作区 revert 要预览确认、公开 share 要显式确认。这些门槛在
/// [`makeRequest`] 里以**必填输入**表达,没有默认值可以跳过。
@MainActor
@Observable
public final class AgentToolsStore {
    /// 宿主探测到的能力(未探测 = 全 false,UI 什么都不显示)。
    public private(set) var capabilities: AdapterClient.AgentCapabilityReport?
    /// 最近一次控制请求的裁决记录(新→旧)。
    public private(set) var outcomes: [ControlLogEntry] = []
    /// 最近一次失败原因(原样来自宿主)。
    public private(set) var lastRefusal: String?
    /// 任务入队时写进记录的权限。
    public var queuedPermissionMode: String = "ask"
    /// 用户当前在界面上选的权限(只用于展示差异)。
    public var currentPermissionMode: String = "ask"
    public var activeSessionID: String = ""

    public struct ControlLogEntry: Identifiable, Equatable, Sendable {
        public let id = UUID()
        public var capability: String
        public var method: String
        /// 用户是否确认过预览/公开导出。
        public var confirmed: Bool
        /// 被脱敏拿掉的字段路径(分享时)。
        public var redacted: [String]

        public init(capability: String, method: String, confirmed: Bool, redacted: [String]) {
            self.capability = capability
            self.method = method
            self.confirmed = confirmed
            self.redacted = redacted
        }
    }

    /// 入队权限与当前权限不一致:UI 必须提示「这个任务按入队时的权限跑」,
    /// 而不是暗示可以中途改。
    public var permissionDrift: Bool {
        queuedPermissionMode != currentPermissionMode
    }

    public init() {}

    public func attach(_ report: AdapterClient.AgentCapabilityReport) {
        capabilities = report
    }

    /// 只渲染被广告的控制。没有探测结果时返回空——不是「全都显示」。
    public var offeredControls: [AdapterClient.CompanionControlName] {
        guard let capabilities else { return [] }
        return AdapterClient.CompanionControlName.allCases.filter(capabilities.advertises)
    }

    public func isOffered(_ control: AdapterClient.CompanionControlName) -> Bool {
        capabilities?.advertises(control) ?? false
    }

    /// 为什么不可用——给用户看的理由,不是「不能点」。
    public func reason(for control: AdapterClient.CompanionControlName) -> String? {
        guard let capabilities else { return "尚未探测到适配器能力" }
        if capabilities.advertises(control) { return nil }
        let method = "session.\(control.rawValue)"
        if capabilities.unsupported.contains(method) {
            return "适配器 \(capabilities.adapter) 未实现 \(method)"
        }
        return "适配器 \(capabilities.adapter) 未广告 \(method)"
    }

    public func record(_ outcome: AdapterClient.ControlOutcome, confirmed: Bool) {
        outcomes.insert(
            ControlLogEntry(
                capability: outcome.capability,
                method: outcome.method,
                confirmed: confirmed,
                redacted: outcome.shareRedacted
            ),
            at: 0
        )
        lastRefusal = nil
    }

    /// 记录一次拒绝。原因原样保留,不做二次包装。
    public func recordRefusal(_ error: Error) {
        lastRefusal = String(describing: error)
    }

    public func clearRefusal() {
        lastRefusal = nil
    }

    /// 构造一次控制请求。危险动作用**参数**强制先给证据;返回 `nil` 表示用户还没
    /// 给足证据,UI 应保持按钮禁用,而不是发一个注定被宿主拒绝的请求。
    public func makeRequest(
        control: AdapterClient.CompanionControlName,
        sessionID: String,
        planEnabled: Bool? = nil,
        workspacePaths: [String] = [],
        previewConfirmed: Bool = false,
        shareIsPublic: Bool = false,
        shareEndpoint: String? = nil,
        confirmed: Bool = false
    ) -> AdapterClient.ControlRequest? {
        guard isOffered(control) else { return nil }
        switch control {
        case .revert where !workspacePaths.isEmpty && !previewConfirmed:
            return nil
        case .share where shareIsPublic:
            guard confirmed, let endpoint = shareEndpoint, !endpoint.isEmpty else { return nil }
        default:
            break
        }

        var request = AdapterClient.ControlRequest(capability: control, sessionID: sessionID)
        request.currentPermissionMode = currentPermissionMode
        request.planEnabled = planEnabled
        request.previewConfirmed = previewConfirmed
        request.confirmed = confirmed
        if control == .revert {
            request.revertTarget = workspacePaths.isEmpty
                ? .session
                : .workspace(affectedPaths: workspacePaths)
        }
        if control == .share {
            request.shareDestination = shareIsPublic
                ? .public(endpoint: shareEndpoint ?? "")
                : .local(path: "")
        }
        return request
    }
}

/// AP7:外部协议面的只读状态(ACP/OAuth/沙箱/工具/语音)。
///
/// 全部是**展示**状态。这里没有「申请权限」「导出 token」「直接执行工具」的入口
/// ——那些要么属于系统设置面板,要么属于外部 Agent。
@MainActor
@Observable
public final class ExternalProtocolStore {
    /// OAuth:只有状态,没有 token(类型上就没有这个字段)。
    public struct AuthStatus: Equatable, Sendable {
        public enum State: Equatable, Sendable {
            case notConfigured
            case configured(owner: String, expiresAt: Int64?, generation: UInt64)
            case expired(owner: String, generation: UInt64)
        }

        public var state: State
        public var launchURL: String?
        public var userCode: String?

        public init(state: State, launchURL: String? = nil, userCode: String? = nil) {
            self.state = state
            self.launchURL = launchURL
            self.userCode = userCode
        }

        public var isUsable: Bool {
            if case .configured = state { return true }
            return false
        }

        public var owner: String? {
            switch state {
            case .notConfigured: return nil
            case let .configured(owner, _, _): return owner
            case let .expired(owner, _): return owner
            }
        }
    }

    /// 有效沙箱:requested 与 effective 分开显示。
    public struct SandboxStatus: Equatable, Sendable {
        public enum State: Equatable, Sendable {
            case disabled
            case ready
            case notReady
            case unsupported
        }

        public var requestedEnabled: Bool
        public var state: State
        public var backend: String
        public var note: String?
        public var writableRoots: [String]

        public init(
            requestedEnabled: Bool,
            state: State,
            backend: String,
            note: String? = nil,
            writableRoots: [String] = []
        ) {
            self.requestedEnabled = requestedEnabled
            self.state = state
            self.backend = backend
            self.note = note
            self.writableRoots = writableRoots
        }

        /// 工具能否执行:**要求了就必须就绪**,否则 fail closed。
        public var allowsToolExecution: Bool {
            !requestedEnabled || state == .ready
        }

        public var blockingReason: String? {
            guard requestedEnabled else { return nil }
            switch state {
            case .ready:
                return nil
            case .unsupported:
                return "此平台不支持沙箱,但设置要求启用:工具不会执行"
            case .notReady:
                return note ?? "沙箱后端 \(backend) 未就绪:工具不会执行"
            case .disabled:
                return "沙箱被禁用但设置要求启用:工具不会执行"
            }
        }
    }

    /// 外部工具的活动提示。执行者永远是外部 Agent。
    public struct ToolActivity: Identifiable, Equatable, Sendable {
        public let id = UUID()
        public var tool: String
        public var summary: String
        public var executor: String
        public var permissions: [String: String]

        public init(tool: String, summary: String, executor: String, permissions: [String: String]) {
            self.tool = tool
            self.summary = summary
            self.executor = executor
            self.permissions = permissions
        }
    }

    /// 语音输入:**默认关闭**;草稿要用户显式提交才成为 prompt。
    public struct VoiceState: Equatable, Sendable {
        public enum Phase: Equatable, Sendable {
            case idle
            case recording
            case draft
            case sent
            case cancelled
        }

        public var optedIn: Bool
        public var phase: Phase
        public var micPermission: String
        public var transcript: String
        /// 真正发出去的 prompt;为空说明什么都没发。
        public var submittedPrompt: String?

        public init(
            optedIn: Bool = false,
            phase: Phase = .idle,
            micPermission: String = "notDetermined",
            transcript: String = "",
            submittedPrompt: String? = nil
        ) {
            self.optedIn = optedIn
            self.phase = phase
            self.micPermission = micPermission
            self.transcript = transcript
            self.submittedPrompt = submittedPrompt
        }

        public var canSubmit: Bool {
            phase == .draft && !transcript.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }
    }

    public var auth: AuthStatus
    public var sandbox: SandboxStatus
    public var activities: [ToolActivity]
    public var voice: VoiceState
    /// ACP 协商结果(未协商 = nil)。
    public var acpVersion: UInt8?

    public init(
        auth: AuthStatus = AuthStatus(state: .notConfigured),
        sandbox: SandboxStatus = SandboxStatus(requestedEnabled: false, state: .disabled, backend: "none"),
        activities: [ToolActivity] = [],
        voice: VoiceState = VoiceState(),
        acpVersion: UInt8? = nil
    ) {
        self.auth = auth
        self.sandbox = sandbox
        self.activities = activities
        self.voice = voice
        self.acpVersion = acpVersion
    }

    /// 用户显式提交草稿:只有非空草稿能变成 prompt。
    @discardableResult
    public func submitVoiceDraft() -> String? {
        guard voice.canSubmit else { return nil }
        let text = voice.transcript.trimmingCharacters(in: .whitespacesAndNewlines)
        voice.submittedPrompt = text
        voice.phase = .sent
        return text
    }

    public func cancelVoice() {
        voice.phase = .cancelled
        voice.submittedPrompt = nil
    }

    /// 语音输入是显式可选项:用户开启之前不录音。
    public func setVoiceOptIn(_ optedIn: Bool) {
        voice.optedIn = optedIn
        if !optedIn { cancelVoice() }
    }

    /// 系统设置面板的入口说明(原生端不代用户授权)。
    public func permissionHandoff(for kind: String) -> String {
        let grant = activities
            .flatMap { activity in activity.permissions.map { ($0.key, $0.value) } }
            .first { $0.0 == kind }?
            .1
        switch grant {
        case "granted":
            return "已授权,可直接使用"
        case "unstableSignature":
            return "签名已变化,请在系统设置中重新授权后重启"
        default:
            return "需要在系统设置 → 隐私与安全性中手动开启"
        }
    }
}
