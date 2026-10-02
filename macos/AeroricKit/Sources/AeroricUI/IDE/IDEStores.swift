import Foundation
import Observation

/// P6-03：断点 store（Action 3 BreakpointStore——mutation 身份 diff；断点
/// 按文件/行去重，condition/logMessage 随条目携带）。
@MainActor
@Observable
public final class BreakpointStore {
    public struct Breakpoint: Identifiable, Equatable, Sendable {
        public var id: String { "\(file):\(line):\(column)" }
        public var file: String
        public var line: UInt32
        public var column: UInt32
        public var condition: String?
        public var logMessage: String?

        public init(
            file: String, line: UInt32, column: UInt32,
            condition: String? = nil, logMessage: String? = nil
        ) {
            self.file = file
            self.line = line
            self.column = column
            self.condition = condition
            self.logMessage = logMessage
        }
    }

    public private(set) var breakpoints: [Breakpoint] = []
    /// 未知 adapter 状态诊断（Action 4：未知 adapter 状态可诊断）。
    public private(set) var lastAdapterDiagnostic: String?

    public init() {}

    /// 断点增删（diff 判定：集合语义，同 file:line 去重）。
    public func toggle(_ breakpoint: Breakpoint) {
        if let index = breakpoints.firstIndex(where: {
            $0.file == breakpoint.file && $0.line == breakpoint.line
        }) {
            breakpoints.remove(at: index)
        } else {
            breakpoints.append(breakpoint)
        }
    }

    public func removeAll(inFile file: String) {
        breakpoints.removeAll { $0.file == file }
    }

    /// mutation 身份（集合语义：与顺序无关）。
    public func matches(_ other: [Breakpoint]) -> Bool {
        let selfSet = Set(breakpoints.map(\.id))
        let otherSet = Set(other.map(\.id))
        return selfSet == otherSet
    }

    public func setAdapterDiagnostic(_ diagnostic: String?) {
        lastAdapterDiagnostic = diagnostic
    }
}

/// P6-03：诊断簿记（ProblemsView 数据源；过时 diagnostics 丢弃——document
/// version 守卫与 core `drop_stale` 同语义）。
@MainActor
@Observable
public final class DiagnosticsStore {
    public struct Problem: Identifiable, Equatable {
        public var id: String { "\(source):\(file):\(line):\(column)" }
        public var source: String
        public var severity: String
        public var message: String
        public var file: String
        public var line: Int
        public var column: Int
        public var code: String?

        public init(
            source: String, severity: String, message: String, file: String,
            line: Int, column: Int, code: String? = nil
        ) {
            self.source = source
            self.severity = severity
            self.message = message
            self.file = file
            self.line = line
            self.column = column
            self.code = code
        }
    }

    public private(set) var problems: [Problem] = []
    public private(set) var documentVersion: UInt32 = 0

    public init() {}

    /// 过时 diagnostics 丢弃（incoming < current ＝ 空）。
    public func ingest(version: UInt32, problems: [Problem]) {
        guard version >= documentVersion else { return }
        documentVersion = version
        self.problems = problems
    }

    public var errorCount: Int {
        problems.filter { $0.severity == "error" }.count
    }

    public var warningCount: Int {
        problems.filter { $0.severity == "warning" }.count
    }
}

/// P6-03 Action 4：Debug 会话状态机（start/attach/pause/continue/step/stop
/// 生命周期判定与 core `is_action_allowed` 同语义——断开 debug 不误 kill
/// 其他任务：按 debugId 定向）。
@MainActor
@Observable
public final class DebugSessionStore {
    public enum Status: String, Equatable, Sendable {
        case idle, starting, running, paused, exited, failed, stopped
    }

    public struct Session: Equatable {
        public var debugId: String
        public var configId: String
        public var name: String
        public var status: Status
        public var pausedReason: String?
        /// call stack / scopes / variables（DAP 快照镜像）。
        public var callStack: [(functionName: String, file: String, line: UInt32)] = []
        public var scopes: [(name: String, variables: [(name: String, value: String)])] = []

        public static func == (lhs: Session, rhs: Session) -> Bool {
            lhs.debugId == rhs.debugId && lhs.status == rhs.status
        }
    }

    public private(set) var sessions: [String: Session] = [:]

    public init() {}

    public func upsert(_ session: Session) {
        sessions[session.debugId] = session
    }

    public func remove(debugId: String) {
        sessions.removeValue(forKey: debugId)
    }

    /// 动作是否合法（生命周期判定；与 core `is_action_allowed` 同语义）。
    public func isAllowed(
        action: DapAction, debugId: String
    ) -> Bool {
        guard let session = sessions[debugId] else {
            return action == .start || action == .attach
        }
        let hasSession = true
        let isPaused = session.status == .paused
        switch action {
        case .start, .attach:
            return !hasSession
        case .pause:
            return hasSession && !isPaused
        case .continue:
            return hasSession && isPaused
        case .stepOver, .stepInto, .stepOut:
            return hasSession && isPaused
        case .stop:
            return hasSession
        }
    }
}

/// DAP 动作（Swift 侧词汇表；与 core `DebugAction` 对齐）。
public enum DapAction: String, CaseIterable, Sendable {
    case start, attach, pause, `continue`, stepOver, stepInto, stepOut, stop
}
