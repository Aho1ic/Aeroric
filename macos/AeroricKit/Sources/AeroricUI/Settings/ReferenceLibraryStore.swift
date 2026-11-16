import AeroricClient
import AeroricProtocol
import Foundation
import Observation

/// 设置 → 参考代码库 (Q-R6, 2026-11-13): local folders an agent may search
/// and read (`search_references`, `read_reference`). One list for every
/// project; Aeroric never writes into the folders. Shown only once the host
/// answered `references.list`.
@MainActor @Observable
public final class ReferenceLibraryStore {
    public struct Calls: Sendable {
        public var list: @Sendable () async throws -> [LiveReference]
        public var add: @Sendable (_ path: String, _ name: String?, _ purpose: String?) async throws -> LiveReference
        public var remove: @Sendable (_ id: String) async throws -> Void
        public var reindex: @Sendable (_ id: String) async throws -> Void

        public init(list: @escaping @Sendable () async throws -> [LiveReference],
                    add: @escaping @Sendable (String, String?, String?) async throws -> LiveReference,
                    remove: @escaping @Sendable (String) async throws -> Void,
                    reindex: @escaping @Sendable (String) async throws -> Void) {
            self.list = list
            self.add = add
            self.remove = remove
            self.reindex = reindex
        }
    }

    public private(set) var isAvailable = false
    public private(set) var references: [LiveReference] = []
    public private(set) var lastError: String?
    @ObservationIgnored private var calls: Calls?
    @ObservationIgnored private var polling = false
    /// How long to wait before looking again while a folder is indexing.
    @ObservationIgnored var pollInterval: Duration = .seconds(2)

    public init() {}

    public func attach(client: SessionsClient?) {
        guard let client else {
            attach(nil)
            return
        }
        attach(Calls(
            list: { try await client.references() },
            add: { try await client.addReference(path: $0, name: $1, purpose: $2) },
            remove: { try await client.removeReference(id: $0) },
            reindex: { try await client.reindexReference(id: $0) }))
    }

    public func attach(_ calls: Calls?) {
        self.calls = calls
        isAvailable = false
        references = []
        lastError = nil
    }

    public func refresh() async {
        guard let calls else {
            isAvailable = false
            return
        }
        do {
            references = try await calls.list()
            isAvailable = true
        } catch {
            isAvailable = false
        }
        pollWhileIndexing()
    }

    public func add(path: String, name: String?, purpose: String?) async {
        await run { calls in _ = try await calls.add(path, name, purpose) }
    }

    public func remove(_ id: String) async {
        await run { calls in try await calls.remove(id) }
    }

    public func reindex(_ id: String) async {
        await run { calls in try await calls.reindex(id) }
    }

    private func run(_ action: (Calls) async throws -> Void) async {
        guard let calls else { return }
        do {
            try await action(calls)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
        await refresh()
    }

    /// Indexing runs on the host; look again until every folder is done.
    private func pollWhileIndexing() {
        guard !polling, references.contains(where: \.indexing) else { return }
        polling = true
        Task { @MainActor [weak self] in
            guard let self else { return }
            try? await Task.sleep(for: self.pollInterval)
            self.polling = false
            await self.refresh()
        }
    }
}
