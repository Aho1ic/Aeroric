import Foundation
import Observation

/// P7-01：连接簿记（Action 2 六视图的数据源；快照来自宿主应答，本类型只
/// 记账——密码不进 store，credential owner 在宿主）。
@MainActor
@Observable
public final class ConnectionStore {
    /// 连接条目（**无明文密码**——只有 hasPassword 布尔，与 core serde 形状
    /// 对齐：密码不进 snapshot/log/新配置目录）。
    public struct Connection: Identifiable, Equatable {
        public var id: String
        public var name: String
        public var group: String?
        public var host: String
        public var port: UInt16
        public var username: String
        public var identityFile: String?
        public var hasPassword: Bool

        public init(
            id: String, name: String, group: String? = nil, host: String, port: UInt16,
            username: String, identityFile: String? = nil, hasPassword: Bool = false
        ) {
            self.id = id
            self.name = name
            self.group = group
            self.host = host
            self.port = port
            self.username = username
            self.identityFile = identityFile
            self.hasPassword = hasPassword
        }
    }

    /// host key 状态（首连/主机 key 变化分别提示）。
    public enum HostKeyState: Equatable, Sendable {
        case trusted
        case unknown(target: String, fingerprints: [String])
        case unreachable(target: String)
    }

    public private(set) var connections: [Connection] = []
    public private(set) var selectedID: String?
    /// host key 待确认（首连/变化分别提示；拒绝不可自动重试为信任）。
    public private(set) var pendingHostKey: HostKeyState?
    /// 远程 cwd（与 local cwd 分开显示，防止 native 文件动作误落本机）。
    public private(set) var remoteCWD: String?

    public init() {}

    public var selected: Connection? {
        selectedID.flatMap { id in connections.first { $0.id == id } }
    }

    public func ingest(_ connections: [Connection]) {
        self.connections = connections
    }

    public func select(_ id: String) {
        selectedID = id
    }

    /// host key 待确认（首连/主机 key 变化分别提示；**不自动重试为信任**）。
    public func setPendingHostKey(_ state: HostKeyState?) {
        pendingHostKey = state
    }

    /// 信任后清除待确认（信任由用户确认触发，不自动）。
    public func clearPendingHostKey() {
        pendingHostKey = nil
    }

    public func setRemoteCWD(_ cwd: String?) {
        remoteCWD = cwd
    }
}
