import Foundation
import Observation

/// P7-03：基础设施簿记（五视图的数据源；快照来自宿主应答，本类型只记账）。
@MainActor
@Observable
public final class InfrastructureStore {
    public struct Container: Identifiable, Equatable {
        public var id: String
        public var image: String
        public var names: String
        public var state: String
        public var status: String
        public var ports: String

        public init(id: String, image: String, names: String, state: String, status: String, ports: String) {
            self.id = id
            self.image = image
            self.names = names
            self.state = state
            self.status = status
            self.ports = ports
        }
    }

    public struct Image: Identifiable, Equatable {
        public var id: String
        public var repository: String
        public var tag: String
        public var size: String

        public init(id: String, repository: String, tag: String, size: String) {
            self.id = id
            self.repository = repository
            self.tag = tag
            self.size = size
        }
    }

    public struct Port: Identifiable, Equatable {
        public var id: String { "\(port):\(pid)" }
        public var port: UInt16
        public var pid: UInt32
        public var processName: String

        public init(port: UInt16, pid: UInt32, processName: String) {
            self.port = port
            self.pid = pid
            self.processName = processName
        }
    }

    public struct CondaEnv: Identifiable, Equatable {
        public var id: String { path }
        public var name: String
        public var path: String

        public init(name: String, path: String) {
            self.name = name
            self.path = path
        }
    }

    public private(set) var containers: [Container] = []
    public private(set) var images: [Image] = []
    public private(set) var ports: [Port] = []
    public private(set) var condaEnvironments: [CondaEnv] = []
    public private(set) var selectedContainerID: String?
    public private(set) var selectedCondaEnvPath: String?
    /// Docker 不可用原因（无 daemon 时显示不可用，不弹 sudo）。
    public private(set) var daemonUnavailableReason: String?
    /// 长任务（operationId → 状态；取消/状态查询）。
    public private(set) var operationStatuses: [String: String] = [:]

    public init() {}

    public var selectedContainer: Container? {
        selectedContainerID.flatMap { id in containers.first { $0.id == id } }
    }

    public func ingest(containers: [Container]) {
        self.containers = containers
    }

    public func ingest(images: [Image]) {
        self.images = images
    }

    public func ingest(ports: [Port]) {
        self.ports = ports
    }

    public func ingest(condaEnvironments: [CondaEnv]) {
        self.condaEnvironments = condaEnvironments
    }

    public func selectContainer(_ id: String?) {
        selectedContainerID = id
    }

    public func selectCondaEnv(path: String?) {
        selectedCondaEnvPath = path
    }

    public func setDaemonUnavailable(_ reason: String?) {
        daemonUnavailableReason = reason
    }

    public func setOperationStatus(_ operationID: String, _ status: String) {
        operationStatuses[operationID] = status
    }
}
