import XCTest
@testable import AeroricUI

/// P7-06 验收：SyncStoreTests——冲突决定的内容绑定（hash 对不上号作废重问）、
/// 退避档位标签、状态合并、冲突清账、凭据不进 store。
@MainActor
final class SyncStoreTests: XCTestCase {

    func testConflictResolutionBindsToTheSeenHashes() {
        let conflict = SyncStore.PendingConflict(
            path: "c.md", localHash: "L1", remoteHash: "R1")

        // 决定与用户看到的冲突一致 → 有效。
        XCTAssertTrue(SyncStore.resolutionMatches(
            conflict, localHash: "L1", remoteHash: "R1"))

        // 另一轮同步改了远端 → 决定作废重新问（不覆盖用户没见过的内容）。
        XCTAssertFalse(SyncStore.resolutionMatches(
            conflict, localHash: "L1", remoteHash: "R2"))
        XCTAssertFalse(SyncStore.resolutionMatches(
            conflict, localHash: "L2", remoteHash: "R1"))
    }

    func testBackoffLabelsMatchTheScheduleVocabulary() {
        XCTAssertEqual(SyncStore.backoffLabel(failures: 0), "正常")
        XCTAssertEqual(SyncStore.backoffLabel(failures: 1), "失败 1 次，稍后重试")
        XCTAssertTrue(SyncStore.backoffLabel(failures: 5).contains("退避"))
        // 封顶语义可见：多次失败也只说「至多 15 分钟」（schedule 的 FALLBACK）。
        XCTAssertTrue(SyncStore.backoffLabel(failures: 99).contains("15 分钟"))
    }

    func testStatusMergeKeepsRemoteOrderAndResolvesLookup() {
        let store = SyncStore()
        store.ingest(remotes: [
            SyncStore.RemoteTarget(id: "r1", kind: "cloud", root: "/notes", connectionId: "c1", autoSync: true),
            SyncStore.RemoteTarget(id: "r2", kind: "git", root: "/docs", connectionId: "", autoSync: false),
        ])
        store.ingest(statuses: [
            SyncStore.Status(remoteId: "r1", autoSync: true, failures: 2, dirty: true, nextRunInMs: 30_000),
        ])
        XCTAssertEqual(store.statuses.count, 1)
        XCTAssertEqual(store.status(of: "r1")?.failures, 2)
        XCTAssertEqual(store.status(of: "r2"), nil, "没跑过的远端状态查询给 nil 而不是假 0")
        XCTAssertNil(store.lastSyncAtMs)
        store.recordSyncFinished(atMs: 1_756_700_000_000)
        XCTAssertEqual(store.lastSyncAtMs, 1_756_700_000_000)
    }

    func testResolvingRemovesOnlyThatConflict() {
        let store = SyncStore()
        store.ingestConflicts([
            SyncStore.PendingConflict(path: "a.md", localHash: "L", remoteHash: "R"),
            SyncStore.PendingConflict(path: "b.md", localHash: "L", remoteHash: "R"),
        ])
        store.recordResolved(path: "a.md")
        XCTAssertEqual(store.pendingConflicts.map(\.path), ["b.md"], "精确清除，别的冲突保留")
        store.recordResolved(path: "a.md") // 重复清除无害
        XCTAssertEqual(store.pendingConflicts.map(\.path), ["b.md"])
    }

    func testStoreNeverHoldsCredentials() {
        // 凭据不进事件快照/store：类型面就没有 secrets 字段（编译期保证）。
        // 这里只断言 store 的公开可写面全部走 ingest/record——无 setSecret 之类入口。
        let store = SyncStore()
        store.ingest(remotes: [
            SyncStore.RemoteTarget(id: "r", kind: "cloud", root: "/n", connectionId: "c", autoSync: true),
        ])
        let mirrored = Mirror(reflecting: store.remotes[0]).children.compactMap(\.label)
        XCTAssertFalse(
            mirrored.contains { $0.lowercased().contains("secret") || $0.lowercased().contains("password") },
            "RemoteTarget 无凭据字段: \(mirrored)")
    }
}
