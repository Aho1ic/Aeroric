import AeroricProtocol
import Foundation

/// 无限画布（UI 改版 M5）的类型化客户端：宿主 `canvas.boards.*` / `canvas.nodes.*`。
/// 能力门控名 `canvas`。
public struct CanvasClient: Sendable {
    public static let capability = "canvas"
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    // MARK: - Models

    public struct Board: Sendable, Equatable, Identifiable {
        public var id: String
        public var name: String?
        public var color: String?
        public var updatedAtMs: UInt64
        public var nodeCount: Int

        public init(id: String, name: String?, color: String?, updatedAtMs: UInt64, nodeCount: Int) {
            self.id = id
            self.name = name
            self.color = color
            self.updatedAtMs = updatedAtMs
            self.nodeCount = nodeCount
        }
    }

    public enum Kind: String, Sendable, CaseIterable {
        case session, note, region, file
    }

    public struct Node: Sendable, Equatable, Identifiable {
        public var id: String
        public var boardId: String
        public var kind: Kind
        public var taskId: String?
        public var importedId: String?
        public var projectId: String?
        public var path: String?
        public var title: String?
        public var content: String?
        public var color: String?
        public var x: Double
        public var y: Double
        public var width: Double
        public var height: Double
        public var z: Int

        public init(id: String = "", boardId: String, kind: Kind, taskId: String? = nil, importedId: String? = nil,
                    projectId: String? = nil, path: String? = nil, title: String? = nil, content: String? = nil,
                    color: String? = nil, x: Double, y: Double, width: Double, height: Double, z: Int = 0) {
            self.id = id
            self.boardId = boardId
            self.kind = kind
            self.taskId = taskId
            self.importedId = importedId
            self.projectId = projectId
            self.path = path
            self.title = title
            self.content = content
            self.color = color
            self.x = x
            self.y = y
            self.width = width
            self.height = height
            self.z = z
        }
    }

    public struct Move: Sendable, Equatable {
        public var id: String
        public var x: Double
        public var y: Double
        public var width: Double?
        public var height: Double?

        public init(id: String, x: Double, y: Double, width: Double? = nil, height: Double? = nil) {
            self.id = id
            self.x = x
            self.y = y
            self.width = width
            self.height = height
        }
    }

    // MARK: - Calls

    public func boards() async throws -> [Board] {
        Self.decodeBoards(try await connection.request("canvas.boards.list", params: .object([:])).result)
    }

    public func createBoard(name: String?) async throws -> Board {
        var params: [String: JSONValue] = [:]
        if let name { params["name"] = .string(name) }
        let frame = try await connection.request("canvas.boards.create", params: .object(params))
        guard let board = Self.decodeBoard(frame.result.fields["board"] ?? .null) else { throw Malformed.reply("canvas.boards.create") }
        return board
    }

    /// `name: nil` clears the name.
    public func renameBoard(id: String, name: String?) async throws {
        _ = try await connection.request("canvas.boards.update", params: .object([
            "id": .string(id), "name": name.map(JSONValue.string) ?? .null,
        ]))
    }

    public func deleteBoard(id: String) async throws {
        _ = try await connection.request("canvas.boards.delete", params: .object(["id": .string(id)]))
    }

    public func nodes(boardId: String) async throws -> (nodes: [Node], revision: UInt64) {
        let frame = try await connection.request("canvas.nodes.list", params: .object(["boardId": .string(boardId)]))
        return (Self.decodeNodes(frame.result), Self.number(frame.result.fields["revision"]).map(UInt64.init) ?? 0)
    }

    public func upsert(_ node: Node) async throws -> Node {
        let frame = try await connection.request("canvas.nodes.upsert", params: .object(["node": Self.encode(node)]))
        guard let saved = Self.decodeNode(frame.result.fields["node"] ?? .null) else { throw Malformed.reply("canvas.nodes.upsert") }
        return saved
    }

    public func move(boardId: String, _ moves: [Move]) async throws {
        let items: [JSONValue] = moves.map { move in
            var fields: [String: JSONValue] = ["id": .string(move.id), "x": Self.json(move.x), "y": Self.json(move.y)]
            if let width = move.width { fields["width"] = Self.json(width) }
            if let height = move.height { fields["height"] = Self.json(height) }
            return .object(fields)
        }
        _ = try await connection.request("canvas.nodes.move", params: .object([
            "boardId": .string(boardId), "moves": .array(items),
        ]))
    }

    public func delete(boardId: String, ids: [String]) async throws {
        _ = try await connection.request("canvas.nodes.delete", params: .object([
            "boardId": .string(boardId), "ids": .array(ids.map(JSONValue.string)),
        ]))
    }

    // MARK: - Coding

    public enum Malformed: Error, Equatable { case reply(String) }

    static func number(_ value: JSONValue?) -> Double? {
        if case let .number(text)? = value { return Double(text) }
        return nil
    }

    static func json(_ value: Double) -> JSONValue {
        value.rounded() == value && abs(value) < 1e15 ? .number(String(Int64(value))) : .number(String(value))
    }

    public static func decodeBoard(_ value: JSONValue) -> Board? {
        let row = value.fields
        guard let id = row["id"]?.stringValue else { return nil }
        return Board(id: id, name: row["name"]?.stringValue, color: row["color"]?.stringValue,
                     updatedAtMs: number(row["updatedAtMs"]).map(UInt64.init) ?? 0,
                     nodeCount: Int(number(row["nodeCount"]) ?? 0))
    }

    public static func decodeBoards(_ value: JSONValue) -> [Board] {
        (value.fields["boards"]?.arrayValue ?? []).compactMap(decodeBoard)
    }

    public static func decodeNode(_ value: JSONValue) -> Node? {
        let row = value.fields
        guard let id = row["id"]?.stringValue, let boardId = row["boardId"]?.stringValue,
              let kind = row["kind"]?.stringValue.flatMap(Kind.init(rawValue:)),
              let x = number(row["x"]), let y = number(row["y"]),
              let width = number(row["width"]), let height = number(row["height"]) else { return nil }
        return Node(id: id, boardId: boardId, kind: kind, taskId: row["taskId"]?.stringValue,
                    importedId: row["importedId"]?.stringValue, projectId: row["projectId"]?.stringValue,
                    path: row["path"]?.stringValue, title: row["title"]?.stringValue,
                    content: row["content"]?.stringValue, color: row["color"]?.stringValue,
                    x: x, y: y, width: width, height: height, z: Int(number(row["z"]) ?? 0))
    }

    public static func decodeNodes(_ value: JSONValue) -> [Node] {
        (value.fields["nodes"]?.arrayValue ?? []).compactMap(decodeNode)
    }

    public static func encode(_ node: Node) -> JSONValue {
        var fields: [String: JSONValue] = [
            "id": .string(node.id), "boardId": .string(node.boardId), "kind": .string(node.kind.rawValue),
            "x": json(node.x), "y": json(node.y), "width": json(node.width), "height": json(node.height),
            "z": .number(String(node.z)), "updatedAtMs": .number("0"),
        ]
        for (key, value) in [("taskId", node.taskId), ("importedId", node.importedId), ("projectId", node.projectId),
                             ("path", node.path), ("title", node.title), ("content", node.content), ("color", node.color)] {
            if let value { fields[key] = .string(value) }
        }
        return .object(fields)
    }
}
