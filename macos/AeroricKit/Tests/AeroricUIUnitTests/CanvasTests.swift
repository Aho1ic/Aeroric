import XCTest
import AeroricClient
@testable import AeroricUI

final class CanvasGeometryTests: XCTestCase {
    private func node(_ id: String, _ x: Double, _ y: Double, _ w: Double = 100, _ h: Double = 100,
                      kind: CanvasClient.Kind = .note) -> CanvasClient.Node {
        CanvasClient.Node(id: id, boardId: "b", kind: kind, x: x, y: y, width: w, height: h)
    }

    func testScreenAndWorldAreInverse() {
        let viewport = CanvasViewport(scale: 1.5, offset: CGPoint(x: 40, y: -20))
        let world = CGPoint(x: 100, y: 200)
        XCTAssertEqual(viewport.toWorld(viewport.toScreen(world)), world)
        XCTAssertEqual(viewport.toScreen(world), CGPoint(x: 190, y: 280))
    }

    func testZoomKeepsTheAnchorFixedAndClamps() {
        let viewport = CanvasViewport(scale: 1, offset: CGPoint(x: 10, y: 10))
        let anchor = CGPoint(x: 300, y: 200)
        let before = viewport.toWorld(anchor)
        let zoomed = viewport.zoomed(by: 1.6, around: anchor)
        XCTAssertEqual(zoomed.scale, 1.6, accuracy: 1e-9)
        XCTAssertEqual(zoomed.toWorld(anchor).x, before.x, accuracy: 1e-9)
        XCTAssertEqual(zoomed.toWorld(anchor).y, before.y, accuracy: 1e-9)
        XCTAssertEqual(viewport.zoomed(by: 100, around: anchor).scale, CanvasViewport.maxScale)
        XCTAssertEqual(viewport.zoomed(by: 0.001, around: anchor).scale, CanvasViewport.minScale)
    }

    func testFitShowsEverythingAndNeverMagnifies() {
        let fit = CanvasViewport.fitting(CGRect(x: 100, y: 100, width: 200, height: 100), in: CGSize(width: 1000, height: 600))
        XCTAssertEqual(fit.scale, 1, "small content is not blown up")
        let screen = fit.toScreen(CGRect(x: 100, y: 100, width: 200, height: 100))
        XCTAssertEqual(screen.midX, 500, accuracy: 1e-9)
        XCTAssertEqual(screen.midY, 300, accuracy: 1e-9)
        let big = CanvasViewport.fitting(CGRect(x: 0, y: 0, width: 4000, height: 1000), in: CGSize(width: 1000, height: 600))
        XCTAssertLessThan(big.scale, 1)
    }

    func testCullingKeepsVisibleNodesPlusMargin() {
        let nodes = [node("in", 0, 0), node("near", 1100, 0), node("far", 5000, 5000)]
        let visible = CanvasGeometry.visible(nodes, in: CGRect(x: 0, y: 0, width: 1000, height: 800))
        XCTAssertEqual(visible.map(\.id), ["in", "near"])
    }

    func testSnapMarqueeAndContainment() {
        XCTAssertEqual(CanvasGeometry.snap(13), 16)
        XCTAssertEqual(CanvasGeometry.snap(11), 8)
        XCTAssertEqual(CanvasGeometry.snap(13, enabled: false), 13)

        let nodes = [node("a", 0, 0), node("b", 300, 0), node("c", 600, 0)]
        XCTAssertEqual(CanvasGeometry.hits(nodes, marquee: CGRect(x: 50, y: 50, width: 300, height: 10)), ["a", "b"])

        let region = node("r", -10, -10, 450, 200, kind: .region)
        let inside = CanvasGeometry.contained(in: region, among: nodes + [region])
        XCTAssertEqual(inside.map(\.id), ["a", "b"], "only fully contained nodes move with a region")
    }

    func testMinimapMapsBothWays() {
        let nodes = [node("a", 0, 0, 1000, 500)]
        let map = CanvasGeometry.minimap(nodes: nodes, visible: CGRect(x: 0, y: 0, width: 500, height: 300),
                                         size: CGSize(width: 180, height: 120))
        let mapped = map.toMap(CGRect(x: 0, y: 0, width: 1000, height: 500))
        XCTAssertLessThanOrEqual(mapped.maxX, 180.0001)
        let back = map.toWorld(CGPoint(x: mapped.midX, y: mapped.midY))
        XCTAssertEqual(back.x, 500, accuracy: 1e-6)
        XCTAssertEqual(back.y, 250, accuracy: 1e-6)
    }

    func testNewNodesDropAtTheViewCentreOnTheGrid() {
        let origin = CanvasGeometry.dropOrigin(for: .note, viewport: CanvasViewport(), viewSize: CGSize(width: 1000, height: 600))
        XCTAssertEqual(origin, CGPoint(x: 392, y: 224))
        XCTAssertEqual(origin.x.truncatingRemainder(dividingBy: CanvasGeometry.grid), 0)
    }
}

/// In-memory host stand-in; `failNext` makes the next write throw.
private final class FakeCanvas: CanvasWriting, @unchecked Sendable {
    var boardList: [CanvasClient.Board] = []
    var store: [String: [CanvasClient.Node]] = [:]
    var failNext = false
    var counter = 0

    struct Failure: Error {}

    private func maybeFail() throws {
        if failNext { failNext = false; throw Failure() }
    }

    func boards() async throws -> [CanvasClient.Board] { boardList }
    func createBoard(name: String?) async throws -> CanvasClient.Board {
        counter += 1
        let board = CanvasClient.Board(id: "b\(counter)", name: name, color: nil, updatedAtMs: UInt64(counter), nodeCount: 0)
        boardList.insert(board, at: 0)
        store[board.id] = []
        return board
    }
    func renameBoard(id: String, name: String?) async throws {
        if let i = boardList.firstIndex(where: { $0.id == id }) { boardList[i].name = name }
    }
    func deleteBoard(id: String) async throws { boardList.removeAll { $0.id == id } }
    func nodes(boardId: String) async throws -> (nodes: [CanvasClient.Node], revision: UInt64) { (store[boardId] ?? [], 1) }
    func upsert(_ node: CanvasClient.Node) async throws -> CanvasClient.Node {
        try maybeFail()
        var saved = node
        if saved.id.isEmpty { counter += 1; saved.id = "n\(counter)" }
        store[node.boardId, default: []].removeAll { $0.id == saved.id }
        store[node.boardId, default: []].append(saved)
        return saved
    }
    func move(boardId: String, _ moves: [CanvasClient.Move]) async throws { try maybeFail() }
    func delete(boardId: String, ids: [String]) async throws { try maybeFail() }
}

@MainActor
final class CanvasBookTests: XCTestCase {
    func testLoadCreatesAFirstBoardAndAddsNodes() async {
        let fake = FakeCanvas()
        let book = CanvasBook()
        book.attach(client: fake, capabilityEnabled: true)
        await book.load()
        XCTAssertEqual(book.boards.map(\.name), ["我的画布"])
        XCTAssertNotNil(book.boardID)
        let saved = await book.add(CanvasClient.Node(boardId: "", kind: .note, content: "想法", x: 0, y: 0, width: 220, height: 160))
        XCTAssertEqual(saved?.boardId, book.boardID, "the open board is filled in")
        XCTAssertEqual(book.nodes.count, 1)
    }

    func testFailedWritesRollBack() async {
        let fake = FakeCanvas()
        let book = CanvasBook()
        book.attach(client: fake, capabilityEnabled: true)
        await book.load()
        let node = await book.add(CanvasClient.Node(boardId: "", kind: .note, x: 0, y: 0, width: 220, height: 160))!

        fake.failNext = true
        await book.move([CanvasClient.Move(id: node.id, x: 400, y: 400)])
        XCTAssertEqual(book.nodes.first?.x, 0, "a rejected move restores the previous layout")
        XCTAssertNotNil(book.lastError)

        fake.failNext = true
        var edited = node
        edited.content = "改过"
        await book.update(edited)
        XCTAssertNil(book.nodes.first?.content)

        fake.failNext = true
        await book.delete([node.id])
        XCTAssertEqual(book.nodes.count, 1)

        await book.move([CanvasClient.Move(id: node.id, x: 64, y: 32, width: 300)])
        XCTAssertEqual(book.nodes.first.map { [$0.x, $0.y, $0.width] }, [64, 32, 300])
    }

    func testWithoutTheCapabilityNothingIsAvailable() async {
        let book = CanvasBook()
        book.attach(client: FakeCanvas(), capabilityEnabled: false)
        await book.load()
        XCTAssertFalse(book.isAvailable)
        XCTAssertTrue(book.boards.isEmpty)
    }
}
