import AeroricClient
import Foundation
import Observation

/// The 连接 page and the open-remote-project sheet (2026-10-27): saved SSH
/// hosts by group, one draft being edited, connection tests and host-key
/// confirmation. Passwords only pass through on save.
@MainActor @Observable
public final class SshHostsStore {
    public typealias Host = SshClient.Host

    public enum AuthMode: String, Hashable, Sendable { case key, password }

    public enum TestState: Equatable {
        case testing
        case result(SshClient.TestResult)
    }

    /// A host waiting for the user to confirm its fingerprint, and whether a
    /// test should follow the confirmation.
    public struct PendingTrust: Equatable, Identifiable {
        public var hostID: String
        public var hostName: String
        public var keys: [SshClient.HostKey]
        public var id: String { hostID }
    }

    public private(set) var hosts: [Host] = []
    /// Groups the user created, in their order; groups only named by a host join them.
    public private(set) var savedGroups: [String] = []
    public private(set) var isAvailable = false
    public var selectedID: String?
    public var draft: Host?
    public var authMode: AuthMode = .key
    public var passwordDraft = ""
    public private(set) var tests: [String: TestState] = [:]
    public var pendingTrust: PendingTrust?
    public var errorText: String?
    public var busy = false
    /// Each host's last successful test (agents, npm), kept by the host.
    public private(set) var probes: [String: SshClient.Probe] = [:]
    /// Remote installs per host, refreshed every second while one runs.
    public private(set) var installs: [String: [SshClient.InstallJob]] = [:]
    /// An install waiting for the user to pick a way and confirm (O1).
    public struct PendingInstall: Equatable, Identifiable {
        public var hostID: String
        public var hostName: String
        public var agent: String
        public var plan: SshClient.InstallPlan
        public var upgrade: Bool
        /// "direct" or "offline"; starts at the plan's recommendation.
        public var method: String
        public var id: String { hostID + "." + agent }
        public init(hostID: String, hostName: String, agent: String, plan: SshClient.InstallPlan, upgrade: Bool) {
            self.hostID = hostID
            self.hostName = hostName
            self.agent = agent
            self.plan = plan
            self.upgrade = upgrade
            method = plan.recommended
        }
        /// Whether the chosen way can run.
        public var ready: Bool { method == "offline" ? plan.offline != nil : plan.command != nil }
    }
    public var pendingInstall: PendingInstall?
    /// "host.agent" while its plan is being worked out.
    public private(set) var preparing: Set<String> = []
    /// Bytes of standalone builds kept on this Mac (O4).
    public private(set) var packageCacheBytes: UInt64 = 0
    @ObservationIgnored private var installPoll: Task<Void, Never>?

    /// The minimum each agent's native session needs (ADR-043 §8, omp phase two).
    public static let minimums = ["claude": "2.1.0", "codex": "0.162.0", "omp": "18.12.0"]

    /// One remote agent's standing, from a probe.
    public enum RemoteAgent: Equatable {
        case missing
        case tooOld(version: String, minimum: String)
        case ready(version: String)
    }

    public static func remoteAgent(_ agent: String, in probe: SshClient.Probe) -> RemoteAgent {
        guard let raw = probe.agents.first(where: { $0.agent == agent })?.version else { return .missing }
        let version = raw.split(whereSeparator: { $0 == " " || $0 == "/" })
            .first { $0.trimmingCharacters(in: CharacterSet(charactersIn: "v")).first?.isNumber == true }
            .map { String($0).trimmingCharacters(in: CharacterSet(charactersIn: "v")) } ?? raw
        if let minimum = minimums[agent], version.compare(minimum, options: .numeric) == .orderedAscending {
            return .tooOld(version: version, minimum: minimum)
        }
        return .ready(version: version)
    }

    public func installJob(_ hostID: String, _ agent: String) -> SshClient.InstallJob? {
        installs[hostID]?.first { $0.agent == agent }
    }

    /// Ask the host what would run, then wait for the user's confirmation.
    public func prepareInstall(hostID: String, agent: String) async {
        guard let client else { return }
        let key = hostID + "." + agent
        preparing.insert(key)
        defer { preparing.remove(key) }
        do {
            let plan = try await client.installPlan(id: hostID, agent: agent)
            let upgrade = probes[hostID].map { Self.remoteAgent(agent, in: $0) != .missing } ?? false
            pendingInstall = PendingInstall(hostID: hostID, hostName: hosts.first { $0.id == hostID }?.name ?? "",
                                            agent: agent, plan: plan, upgrade: upgrade)
        } catch {
            errorText = "\(error)"
        }
    }

    public func confirmInstall() async {
        guard let client, let pending = pendingInstall, pending.ready else { return }
        pendingInstall = nil
        do {
            try await client.install(id: pending.hostID, agent: pending.agent, method: pending.method)
            await pollInstalls(pending.hostID)
        } catch {
            errorText = "安装没能开始：\(error)"
        }
    }

    public func clearPackageCache() async {
        guard let client else { return }
        do {
            try await client.clearPackageCache()
            packageCacheBytes = 0
        } catch {
            errorText = "清空本机缓存失败：\(error)"
        }
    }

    private func pollInstalls(_ hostID: String) async {
        guard let client else { return }
        installs[hostID] = (try? await client.installStatus(id: hostID)) ?? installs[hostID]
        guard installs[hostID]?.contains(where: \.running) == true, installPoll == nil else { return }
        installPoll = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                guard let self, let client = self.client else { return }
                self.installs[hostID] = (try? await client.installStatus(id: hostID)) ?? self.installs[hostID]
                if self.installs[hostID]?.contains(where: \.running) != true {
                    self.probes = (try? await client.probes()) ?? self.probes
                    self.packageCacheBytes = (try? await client.packageCacheBytes()) ?? self.packageCacheBytes
                    break
                }
            }
            self?.installPoll = nil
        }
    }

    @ObservationIgnored private var client: SshClient?

    public init() {}

    public func attach(client: SshClient?) {
        self.client = client
        isAvailable = client != nil
    }

    public static let ungrouped = "未分组"

    /// Groups in name order — created ones even when empty (not while searching) —
    /// "未分组" last; each group's hosts by name.
    public static func grouped(_ hosts: [Host], groups: [String] = [], query: String = "")
        -> [(group: String, hosts: [Host])] {
        let query = query.trimmingCharacters(in: .whitespaces).lowercased()
        let matching = hosts.filter { host in
            query.isEmpty || [host.name, host.host, host.username, host.group ?? ""]
                .contains { $0.lowercased().contains(query) }
        }
        let byGroup = Dictionary(grouping: matching) { ($0.group ?? "").trimmingCharacters(in: .whitespaces) }
        var keys = Set(byGroup.keys)
        if query.isEmpty { keys.formUnion(groups.map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }) }
        return keys
            .sorted { lhs, rhs in
                if lhs.isEmpty != rhs.isEmpty { return !lhs.isEmpty }
                return lhs.localizedStandardCompare(rhs) == .orderedAscending
            }
            .map { key in
                (key.isEmpty ? ungrouped : key,
                 byGroup[key, default: []].sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending })
            }
    }

    /// Every group: created ones and those a host names.
    public var groupNames: [String] {
        let named = hosts.compactMap { $0.group?.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        return Array(Set(savedGroups + named)).sorted { $0.localizedStandardCompare($1) == .orderedAscending }
    }

    /// Why `name` cannot be a new group, or nil.
    public func groupProblem(_ name: String, renaming old: String? = nil) -> String? {
        let name = name.trimmingCharacters(in: .whitespaces)
        if name.isEmpty { return "填写分组名" }
        if name == Self.ungrouped { return "“未分组”是保留名" }
        if name != old, groupNames.contains(name) { return "已有同名分组" }
        return nil
    }

    public func createGroup(_ name: String) async {
        guard let client, groupProblem(name) == nil else { return }
        let name = name.trimmingCharacters(in: .whitespaces)
        do {
            savedGroups = try await client.saveGroupsReturning(savedGroups + [name])
        } catch {
            errorText = "新建分组失败：\(error)"
        }
    }

    /// Rename a group, moving its hosts along.
    public func renameGroup(_ old: String, to new: String) async {
        guard let client, groupProblem(new, renaming: old) == nil else { return }
        let new = new.trimmingCharacters(in: .whitespaces)
        do {
            let moved = hosts.map { host -> Host in
                var host = host
                if host.group?.trimmingCharacters(in: .whitespaces) == old { host.group = new }
                return host
            }
            if moved != hosts { try await client.save(moved) }
            let groups = savedGroups.contains(old) ? savedGroups.map { $0 == old ? new : $0 } : savedGroups + [new]
            savedGroups = try await client.saveGroupsReturning(groups)
            if draft?.group == old { draft?.group = new }
            await refresh()
        } catch {
            errorText = "重命名分组失败：\(error)"
        }
    }

    /// Delete a group; its hosts move to 未分组 (they are never deleted).
    public func deleteGroup(_ name: String) async {
        guard let client else { return }
        do {
            let moved = hosts.map { host -> Host in
                var host = host
                if host.group?.trimmingCharacters(in: .whitespaces) == name { host.group = nil }
                return host
            }
            if moved != hosts { try await client.save(moved) }
            savedGroups = try await client.saveGroupsReturning(savedGroups.filter { $0 != name })
            if draft?.group == name { draft?.group = nil }
            await refresh()
        } catch {
            errorText = "删除分组失败：\(error)"
        }
    }

    public func refresh() async {
        guard let client else { return }
        do {
            hosts = try await client.load()
            savedGroups = (try? await client.loadGroups()) ?? savedGroups
            probes = (try? await client.probes()) ?? probes
            packageCacheBytes = (try? await client.packageCacheBytes()) ?? packageCacheBytes
            if let id = selectedID, !hosts.contains(where: { $0.id == id }) { select(nil) }
            if selectedID == nil, let first = hosts.first { select(first.id) }
        } catch {
            errorText = "读取 SSH 主机失败：\(error)"
        }
    }

    public func select(_ id: String?) {
        selectedID = id
        draft = id.flatMap { id in hosts.first { $0.id == id } }
        authMode = (draft?.hasPassword ?? false) ? .password : .key
        passwordDraft = ""
    }

    public func newHost() {
        selectedID = nil
        draft = Host(id: UUID().uuidString, name: "", host: "", port: 22, username: NSUserName())
        authMode = .key
        passwordDraft = ""
    }

    /// Why the draft cannot be saved yet, or nil.
    public var draftProblem: String? {
        guard let draft else { return nil }
        if draft.host.trimmingCharacters(in: .whitespaces).isEmpty { return "填写主机" }
        if draft.username.trimmingCharacters(in: .whitespaces).isEmpty { return "填写用户名" }
        if !(1...65535).contains(draft.port) { return "端口应在 1–65535" }
        if authMode == .password, !draft.hasPassword, passwordDraft.isEmpty { return "填写密码" }
        return nil
    }

    public func save() async {
        guard let client, var draft, draftProblem == nil else { return }
        busy = true
        defer { busy = false }
        if draft.name.trimmingCharacters(in: .whitespaces).isEmpty { draft.name = draft.host }
        draft.group = draft.group?.trimmingCharacters(in: .whitespaces).nilIfEmpty
        if authMode == .key { draft.hasPassword = false }
        var next = hosts.filter { $0.id != draft.id }
        next.append(draft)
        let password = authMode == .password && !passwordDraft.isEmpty ? (draft.id, passwordDraft) : nil
        do {
            try await client.save(next, password: password)
            if authMode == .key, hosts.first(where: { $0.id == draft.id })?.hasPassword == true {
                // Switching to key auth: drop the stored password too.
                try? await client.delete(id: draft.id)
                try await client.save(next, password: nil)
            }
            passwordDraft = ""
            selectedID = draft.id
            await refresh()
            select(draft.id)
        } catch {
            errorText = "保存失败：\(error)"
        }
    }

    public func delete() async {
        guard let client, let id = selectedID else { return }
        do {
            try await client.delete(id: id)
            selectedID = nil
            draft = nil
            tests[id] = nil
            await refresh()
        } catch {
            errorText = "删除失败：\(error)"
        }
    }

    /// Test the selected host; an untrusted host asks for its fingerprint first.
    public func test(_ id: String? = nil) async {
        guard let client, let id = id ?? selectedID else { return }
        tests[id] = .testing
        do {
            let result = try await client.test(id: id)
            tests[id] = .result(result)
            if case .ok = result { probes = (try? await client.probes()) ?? probes }
            if case let .untrusted(keys) = result {
                pendingTrust = PendingTrust(hostID: id, hostName: hosts.first { $0.id == id }?.name ?? "", keys: keys)
            }
        } catch {
            tests[id] = .result(.failed("\(error)"))
        }
    }

    public func trustPending() async {
        guard let client, let pending = pendingTrust else { return }
        do {
            try await client.trust(pending.keys)
            pendingTrust = nil
            await test(pending.hostID)
        } catch {
            errorText = "信任失败：\(error)"
        }
    }

    /// Opens a project on a host; returns its project id.
    public func openRemoteProject(hostID: String, path: String, name: String) async throws -> String {
        guard let client else { throw URLError(.notConnectedToInternet) }
        return try await client.openRemoteProject(connectionId: hostID, remotePath: path, name: name).id
    }

    public func test(for id: String) -> TestState? { tests[id] }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}
