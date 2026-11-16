import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

/// Q-R6 (2026-11-13): 设置 → 参考代码库.
@MainActor
final class ReferenceLibraryTests: XCTestCase {
    private struct Refused: Error, CustomStringConvertible {
        var description: String { "这个文件夹已经在参考代码库里（yolo）" }
    }

    /// The host's list, kept by the test's fake calls.
    private actor Host {
        var rows: [LiveReference] = []
        func set(_ rows: [LiveReference]) { self.rows = rows }
    }

    func testARowDecodesAndSaysHowFarItsIndexIs() {
        let row = LiveReference.decode(.object([
            "id": .string("yolo-1a2b"), "name": .string("yolo"), "path": .string("/r/yolo"),
            "purpose": .string("检测头"), "files": .number("120"), "symbols": .number("340"),
            "indexedMs": .number("1794538000000"), "truncated": .bool(false),
            "indexing": .bool(false), "stale": .bool(true),
        ]))
        XCTAssertEqual(row?.name, "yolo")
        XCTAssertEqual(row?.purpose, "检测头")
        XCTAssertEqual(row?.statusText, "120 个文件 · 340 个定义 · 有改动，下次检索时更新")
        XCTAssertNil(LiveReference.decode(.object(["name": .string("no id")])))
        XCTAssertEqual(LiveReference(id: "a", name: "a", path: "/a", indexing: true).statusText, "正在建索引…")
        XCTAssertEqual(LiveReference(id: "a", name: "a", path: "/a").statusText, "还没有索引")
        XCTAssertEqual(LiveReference(id: "a", name: "a", path: "/a", error: "不是文件夹").statusText,
                       "索引失败：不是文件夹")
    }

    func testAHostWithoutTheMethodsHidesThePane() async {
        let store = ReferenceLibraryStore()
        store.attach(.init(list: { throw Refused() }, add: { _, _, _ in throw Refused() },
                           remove: { _ in }, reindex: { _ in }))
        await store.refresh()
        XCTAssertFalse(store.isAvailable)
        store.attach(nil)
        await store.refresh()
        XCTAssertFalse(store.isAvailable)
    }

    func testAddingLooksAgainUntilTheIndexIsDone() async throws {
        let host = Host()
        let store = ReferenceLibraryStore()
        store.pollInterval = .milliseconds(20)
        store.attach(.init(
            list: { await host.rows },
            add: { path, name, purpose in
                let row = LiveReference(id: "yolo-1", name: name ?? "x", path: path, purpose: purpose, indexing: true)
                await host.set([row])
                return row
            },
            remove: { _ in await host.set([]) },
            reindex: { _ in }))
        await store.refresh()
        XCTAssertTrue(store.isAvailable)
        XCTAssertTrue(store.references.isEmpty)

        await store.add(path: "/r/yolo", name: "yolo", purpose: "检测头")
        XCTAssertEqual(store.references.map(\.indexing), [true])
        await host.set([LiveReference(id: "yolo-1", name: "yolo", path: "/r/yolo", files: 3, indexedMs: 1)])
        for _ in 0..<50 where store.references.first?.indexing == true {
            try await Task.sleep(for: .milliseconds(20))
        }
        XCTAssertEqual(store.references.first?.files, 3, "polled until indexed")

        await store.remove("yolo-1")
        XCTAssertTrue(store.references.isEmpty)
    }

    func testARefusedAddSaysWhy() async {
        let store = ReferenceLibraryStore()
        store.attach(.init(list: { [] }, add: { _, _, _ in throw Refused() },
                           remove: { _ in }, reindex: { _ in }))
        await store.refresh()
        await store.add(path: "/r/yolo", name: nil, purpose: nil)
        XCTAssertEqual(store.lastError, "这个文件夹已经在参考代码库里（yolo）")
    }
}
