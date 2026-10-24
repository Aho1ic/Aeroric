import AeroricProtocol
import Foundation

/// AP7:会话控制与适配器能力面(macOS + iOS 共用)。
///
/// ## 为什么客户端只做「显示已广告的能力」
///
/// 宿主在握手里返回的能力清单是**唯一**真源:未广告的项在服务端会被
/// `UnsupportedCapability` 拒绝(`aeroric_core::adapters::SessionControls::authorize`)。
/// 客户端在这里再过滤一次,只是为了让 UI 不必显示注定失败的按钮——它**不是**
/// 安全边界,任何绕过这里的调用仍会被宿主拒绝。
///
/// ## 这一层拿不到的东西
///
/// `adapters.capabilities` 只回探测结果;OAuth 凭据只在状态里(见 core 的
/// `OAuthStatus`,类型上就没有 token 字段)。
public struct AdapterClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    // MARK: - 能力

    /// 一个适配器上报的真实能力。字段名与协议 `AgentCapabilities` 逐字对齐。
    public struct AgentCapabilityReport: Sendable, Equatable {
        public var adapter: String
        public var version: String?
        public var nativePrompt: Bool
        public var terminalInput: Bool
        public var planMode: Bool
        public var compact: Bool
        public var fork: Bool
        public var revert: Bool
        public var share: Bool
        public var usage: Bool
        public var attachments: Bool
        /// 探不到的能力(展示给用户「为什么没有这个按钮」)。
        public var unsupported: [String]
        /// 面向用户的一句话证据(适配器 + 版本 + 已广告项)。
        public var note: String?

        /// 会话控制是否可用。未广告即 UI 不渲染,宿主也拒绝。
        public func advertises(_ control: CompanionControlName) -> Bool {
            switch control {
            case .plan: return planMode
            case .compact: return compact
            case .fork: return fork
            case .revert: return revert
            case .share: return share
            }
        }
    }

    public enum CompanionControlName: String, CaseIterable, Sendable {
        case plan, compact, fork, revert, share
    }

    public func capabilities() async throws -> AgentCapabilityReport {
        let frame = try await connection.request("adapters.capabilities", params: .null)
        let agent = frame.result["agent"] ?? .null
        let extra = agent["extra"] ?? .null
        return AgentCapabilityReport(
            adapter: agent["adapter"]?.stringValue
                ?? extra["adapter"]?.stringValue ?? "unknown",
            version: extra["adapterVersion"]?.stringValue,
            nativePrompt: agent["nativePrompt"]?.boolValue ?? false,
            terminalInput: agent["terminalInput"]?.boolValue ?? false,
            planMode: agent["planMode"]?.boolValue ?? false,
            compact: agent["compact"]?.boolValue ?? false,
            fork: agent["fork"]?.boolValue ?? false,
            revert: agent["revert"]?.boolValue ?? false,
            share: agent["share"]?.boolValue ?? false,
            usage: agent["usage"]?.boolValue ?? false,
            attachments: agent["attachments"]?.boolValue ?? false,
            unsupported: (extra["unsupported"]?.arrayValue ?? []).compactMap(\.stringValue),
            note: nil
        )
    }

    // MARK: - 会话控制

    /// 一次控制请求。`queuedPermissionMode` 与 `currentPermissionMode` 同时带上,
    /// 由**宿主**判定漂移——客户端不预判,也不替用户改权限。
    public struct ControlRequest: Sendable, Equatable {
        public var capability: CompanionControlName
        public var sessionID: String
        public var currentPermissionMode: String?
        public var requestedPermissionMode: String?
        public var planEnabled: Bool?
        public var revertTarget: RevertTarget?
        public var previewConfirmed: Bool
        public var shareDestination: ShareDestination?
        public var shareRedaction: String?
        public var confirmed: Bool
        /// 参与脱敏导出的内容。
        public var payload: JSONValue

        public init(capability: CompanionControlName, sessionID: String) {
            self.capability = capability
            self.sessionID = sessionID
            self.currentPermissionMode = nil
            self.requestedPermissionMode = nil
            self.planEnabled = nil
            self.revertTarget = nil
            self.previewConfirmed = false
            self.shareDestination = nil
            self.shareRedaction = nil
            self.confirmed = false
            self.payload = .null
        }

        /// 序列化成宿主 `session.control` 的 params(空值不上线)。
        public var params: JSONValue {
            var fields: [String: JSONValue] = [
                "capability": .string(capability.rawValue),
                "sessionId": .string(sessionID),
            ]
            if let mode = currentPermissionMode { fields["currentPermissionMode"] = .string(mode) }
            if let mode = requestedPermissionMode { fields["requestedPermissionMode"] = .string(mode) }
            if let enabled = planEnabled { fields["planEnabled"] = .bool(enabled) }
            if let target = revertTarget { fields["revertTarget"] = target.json }
            if previewConfirmed { fields["previewConfirmed"] = .bool(true) }
            if let destination = shareDestination { fields["shareDestination"] = destination.json }
            if let redaction = shareRedaction { fields["shareRedaction"] = .string(redaction) }
            if confirmed { fields["confirmed"] = .bool(true) }
            if payload != .null { fields["payload"] = payload }
            return .object(fields)
        }
    }

    public enum RevertTarget: Sendable, Equatable {
        case session
        case workspace(affectedPaths: [String])

        public var json: JSONValue {
            switch self {
            case .session:
                return .object(["target": .string("session")])
            case let .workspace(paths):
                return .object([
                    "target": .string("workspace"),
                    "affectedPaths": .array(paths.map { JSONValue.string($0) }),
                ])
            }
        }
    }

    public enum ShareDestination: Sendable, Equatable {
        case local(path: String)
        case `public`(endpoint: String)

        public var json: JSONValue {
            switch self {
            case let .local(path):
                return .object(["target": .string("local"), "path": .string(path)])
            case let .public(endpoint):
                return .object(["target": .string("public"), "endpoint": .string(endpoint)])
            }
        }
    }

    public struct ControlOutcome: Sendable, Equatable {
        public var capability: String
        public var method: String
        public var planEnabled: Bool?
        public var sharePayload: JSONValue?
        public var shareRedacted: [String]
    }

    /// 发一次控制请求。拒绝由宿主给出(未广告/权限漂移/缺预览/缺确认),
    /// 这里原样抛出,不做静默降级。
    public func control(_ request: ControlRequest) async throws -> ControlOutcome {
        let frame = try await connection.request("session.control", params: request.params)
        let share = frame.result["share"] ?? .null
        return ControlOutcome(
            capability: frame.result["capability"]?.stringValue ?? request.capability.rawValue,
            method: frame.result["method"]?.stringValue ?? "",
            planEnabled: frame.result["planEnabled"]?.boolValue,
            sharePayload: share == .null ? nil : share["payload"],
            shareRedacted: (share["redacted"]?.arrayValue ?? []).compactMap(\.stringValue)
        )
    }
}
