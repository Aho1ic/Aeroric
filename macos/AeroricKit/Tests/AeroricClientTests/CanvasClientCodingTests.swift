import XCTest
import AeroricProtocol
@testable import AeroricClient

final class CanvasClientCodingTests: XCTestCase {
    func testNodesRoundTripThroughTheWireShape() {
        let node = CanvasClient.Node(id: "a", boardId: "b", kind: .session, taskId: "t1", title: "会话",
                                     x: 12, y: -8.5, width: 240, height: 120, z: 3)
        XCTAssertEqual(CanvasClient.decodeNode(CanvasClient.encode(node)), node)
        let fields = CanvasClient.encode(node).fields
        XCTAssertEqual(fields["x"], .number("12"), "whole numbers stay integral on the wire")
        XCTAssertEqual(fields["y"], .number("-8.5"))
        XCTAssertNil(fields["importedId"], "absent references are omitted, not null")
    }

    func testMalformedRowsAreDropped() {
        let nodes = CanvasClient.decodeNodes(.object(["nodes": .array([
            .object(["id": .string("a"), "boardId": .string("b"), "kind": .string("note"),
                     "x": .number("0"), "y": .number("0"), "width": .number("200"), "height": .number("120")]),
            .object(["id": .string("c"), "boardId": .string("b"), "kind": .string("terminal"),
                     "x": .number("0"), "y": .number("0"), "width": .number("200"), "height": .number("120")]),
            .object(["id": .string("d")]),
        ])]))
        XCTAssertEqual(nodes.map(\.id), ["a"], "unknown kinds and incomplete rows are skipped")

        let boards = CanvasClient.decodeBoards(.object(["boards": .array([
            .object(["id": .string("b1"), "name": .string("规划"), "updatedAtMs": .number("9"), "nodeCount": .number("4")]),
            .object(["name": .string("no id")]),
        ])]))
        XCTAssertEqual(boards, [CanvasClient.Board(id: "b1", name: "规划", color: nil, updatedAtMs: 9, nodeCount: 4)])
    }
}
