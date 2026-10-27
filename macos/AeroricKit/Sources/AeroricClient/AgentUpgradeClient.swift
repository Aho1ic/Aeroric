import AeroricProtocol
import Foundation

/// Agent upgrades and new-version reminders (2026-10-27): host `agents.versions`,
/// `agents.checkUpdates`, `agents.upgrade`, `agents.setReminder`. Capability
/// `agents.upgrade`; without it nothing here is called.
public struct AgentUpgradeClient: Sendable {
    public static let capability = "agents.upgrade"
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public enum Status: String, Sendable {
        case upToDate, updateAvailable, required, notInstalled, unknown
    }

    public enum JobState: String, Sendable {
        case running, succeeded, failed
    }

    public enum ReminderMode: String, Sendable {
        case on, skipVersion, off
    }

    public struct Job: Sendable, Equatable {
        public var state: JobState
        public var installing: Bool
        public var log: [String]
        public var message: String?
        public var finishedAtMs: UInt64?

        public init(state: JobState, installing: Bool = false, log: [String] = [], message: String? = nil,
                    finishedAtMs: UInt64? = nil) {
            self.state = state
            self.installing = installing
            self.log = log
            self.message = message
            self.finishedAtMs = finishedAtMs
        }
    }

    public struct Harness: Sendable, Equatable, Identifiable {
        public var agent: String
        public var title: String
        public var installed: String?
        public var latest: String?
        public var minimum: String?
        public var channel: String
        public var status: Status
        public var remind: Bool
        public var skipVersion: String?
        public var shouldRemind: Bool
        public var job: Job?
        public var id: String { agent }

        public init(agent: String, title: String, installed: String?, latest: String?, minimum: String? = nil,
                    channel: String = "", status: Status, remind: Bool = true, skipVersion: String? = nil,
                    shouldRemind: Bool = false, job: Job? = nil) {
            self.agent = agent
            self.title = title
            self.installed = installed
            self.latest = latest
            self.minimum = minimum
            self.channel = channel
            self.status = status
            self.remind = remind
            self.skipVersion = skipVersion
            self.shouldRemind = shouldRemind
            self.job = job
        }
    }

    public struct Snapshot: Sendable, Equatable {
        public var checkedAtMs: UInt64?
        public var checkError: String?
        public var agents: [Harness]

        public init(checkedAtMs: UInt64? = nil, checkError: String? = nil, agents: [Harness]) {
            self.checkedAtMs = checkedAtMs
            self.checkError = checkError
            self.agents = agents
        }
    }

    public func versions() async throws -> Snapshot {
        Self.decode(try await connection.request("agents.versions", params: .object([:])).result)
    }

    public func checkUpdates() async throws -> Snapshot {
        Self.decode(try await connection.request("agents.checkUpdates", params: .object([:])).result)
    }

    public func upgrade(agent: String) async throws -> Snapshot {
        Self.decode(try await connection.request("agents.upgrade", params: .object(["agent": .string(agent)])).result)
    }

    public func setReminder(agent: String, mode: ReminderMode) async throws -> Snapshot {
        Self.decode(try await connection.request(
            "agents.setReminder",
            params: .object(["agent": .string(agent), "mode": .string(mode.rawValue)])
        ).result)
    }

    public static func decode(_ value: JSONValue) -> Snapshot {
        let agents = (value.fields["agents"]?.arrayValue ?? []).compactMap { item -> Harness? in
            let row = item.fields
            guard let agent = row["agent"]?.stringValue else { return nil }
            let job = row["job"].flatMap { job -> Job? in
                let fields = job.fields
                guard let state = fields["state"]?.stringValue.flatMap(JobState.init(rawValue:)) else { return nil }
                return Job(state: state, installing: fields["installing"]?.boolValue ?? false,
                           log: (fields["log"]?.arrayValue ?? []).compactMap(\.stringValue),
                           message: fields["message"]?.stringValue,
                           finishedAtMs: number(fields["finishedAt"]))
            }
            return Harness(agent: agent, title: row["title"]?.stringValue ?? agent,
                           installed: row["installed"]?.stringValue, latest: row["latest"]?.stringValue,
                           minimum: row["minimum"]?.stringValue, channel: row["channel"]?.stringValue ?? "",
                           status: row["status"]?.stringValue.flatMap(Status.init(rawValue:)) ?? .unknown,
                           remind: row["remind"]?.boolValue ?? true, skipVersion: row["skipVersion"]?.stringValue,
                           shouldRemind: row["shouldRemind"]?.boolValue ?? false, job: job)
        }
        return Snapshot(checkedAtMs: number(value.fields["checkedAt"]),
                        checkError: value.fields["checkError"]?.stringValue, agents: agents)
    }

    static func number(_ value: JSONValue?) -> UInt64? {
        value?.numberText.flatMap { UInt64($0) ?? Double($0).map { UInt64($0) } }
    }
}
