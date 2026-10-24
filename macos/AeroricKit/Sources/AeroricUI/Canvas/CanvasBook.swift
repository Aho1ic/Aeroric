import AeroricClient
import Foundation
import Observation

/// Canvas state for one window (M5): boards, the open board's nodes, and
/// optimistic edits that roll back to the host's state on failure. All writes
/// go through `CanvasClient`; nothing here touches files (INV-02).
@MainActor @Observable
public final class CanvasBook {
    public private(set) var isAvailable = false
    public private(set) var boards: [CanvasClient.Board] = []
    public private(set) var boardID: String?
    public private(set) var nodes: [CanvasClient.Node] = []
    public private(set) var revision: UInt64 = 0
    public private(set) var loading = false
    public var lastError: String?
    @ObservationIgnored private var client: (any CanvasWriting)?

    public init() {}

    public func attach(client: (any CanvasWriting)?, capabilityEnabled: Bool) {
        self.client = capabilityEnabled ? client : nil
        isAvailable = capabilityEnabled && client != nil
        if !isAvailable {
            boards = []
            nodes = []
            boardID = nil
        }
    }

    public var currentBoard: CanvasClient.Board? { boards.first { $0.id == boardID } }

    /// Load the board list; open the most recent board (creating one if none).
    public func load() async {
        guard let client else { return }
        loading = true
        defer { loading = false }
        do {
            boards = try await client.boards()
            if boards.isEmpty { boards = [try await client.createBoard(name: "我的画布")] }
            let target = boardID.flatMap { id in boards.first { $0.id == id }?.id } ?? boards.first?.id
            if let target { try await open(target) }
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func select(_ id: String) async {
        do { try await open(id) } catch { lastError = String(describing: error) }
    }

    private func open(_ id: String) async throws {
        guard let client else { return }
        let page = try await client.nodes(boardId: id)
        boardID = id
        nodes = page.nodes
        revision = page.revision
    }

    public func reload() async {
        guard let boardID else { return await load() }
        await select(boardID)
    }

    public func createBoard(name: String?) async {
        guard let client else { return }
        do {
            let board = try await client.createBoard(name: name)
            boards = try await client.boards()
            try await open(board.id)
        } catch { lastError = String(describing: error) }
    }

    public func renameBoard(_ id: String, name: String?) async {
        guard let client else { return }
        do {
            try await client.renameBoard(id: id, name: name)
            boards = try await client.boards()
        } catch { lastError = String(describing: error) }
    }

    public func deleteBoard(_ id: String) async {
        guard let client else { return }
        do {
            try await client.deleteBoard(id: id)
            if boardID == id { boardID = nil; nodes = [] }
            await load()
        } catch { lastError = String(describing: error) }
    }

    /// Add a node; returns the saved node (with its server id) or nil.
    @discardableResult
    public func add(_ node: CanvasClient.Node) async -> CanvasClient.Node? {
        guard let client, let boardID else { return nil }
        var node = node
        node.boardId = boardID
        do {
            let saved = try await client.upsert(node)
            nodes.append(saved)
            return saved
        } catch {
            lastError = String(describing: error)
            return nil
        }
    }

    /// Replace a node's content/title/colour (optimistic).
    public func update(_ node: CanvasClient.Node) async {
        guard let client, let index = nodes.firstIndex(where: { $0.id == node.id }) else { return }
        let previous = nodes[index]
        nodes[index] = node
        do {
            nodes[index] = try await client.upsert(node)
        } catch {
            if let i = nodes.firstIndex(where: { $0.id == node.id }) { nodes[i] = previous }
            lastError = String(describing: error)
        }
    }

    /// Batch move/resize (optimistic, all-or-nothing on the host).
    public func move(_ moves: [CanvasClient.Move]) async {
        guard let client, let boardID, !moves.isEmpty else { return }
        let previous = nodes
        apply(moves)
        do {
            try await client.move(boardId: boardID, moves)
        } catch {
            nodes = previous
            lastError = String(describing: error)
        }
    }

    public func delete(_ ids: Set<String>) async {
        guard let client, let boardID, !ids.isEmpty else { return }
        let previous = nodes
        nodes.removeAll { ids.contains($0.id) }
        do {
            try await client.delete(boardId: boardID, ids: Array(ids))
        } catch {
            nodes = previous
            lastError = String(describing: error)
        }
    }

    /// Local preview during a drag; committed later with `move`.
    public func preview(_ moves: [CanvasClient.Move]) { apply(moves) }

    private func apply(_ moves: [CanvasClient.Move]) {
        for move in moves {
            guard let i = nodes.firstIndex(where: { $0.id == move.id }) else { continue }
            nodes[i].x = move.x
            nodes[i].y = move.y
            if let width = move.width { nodes[i].width = width }
            if let height = move.height { nodes[i].height = height }
        }
    }
}

/// The calls `CanvasBook` needs; `CanvasClient` conforms, tests use a fake.
public protocol CanvasWriting: Sendable {
    func boards() async throws -> [CanvasClient.Board]
    func createBoard(name: String?) async throws -> CanvasClient.Board
    func renameBoard(id: String, name: String?) async throws
    func deleteBoard(id: String) async throws
    func nodes(boardId: String) async throws -> (nodes: [CanvasClient.Node], revision: UInt64)
    func upsert(_ node: CanvasClient.Node) async throws -> CanvasClient.Node
    func move(boardId: String, _ moves: [CanvasClient.Move]) async throws
    func delete(boardId: String, ids: [String]) async throws
}

extension CanvasClient: CanvasWriting {}
