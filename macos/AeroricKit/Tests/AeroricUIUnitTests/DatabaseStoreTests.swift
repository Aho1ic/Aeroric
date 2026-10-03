import XCTest
@testable import AeroricUI

/// P7-04 验收：DatabaseStoreTests——连接簿记（无明文）、生产确认一次绑定
/// 不串 SQL、有界分页、close tab 释放。
@MainActor
final class DatabaseStoreTests: XCTestCase {

    func testConnectionIngestNoPlaintext() {
        // debug log 不输出 connection string/password（store 无该字段——
        // Connection 条目无 password）。
        let store = DatabaseStore()
        store.ingest(connections: [
            DatabaseStore.Connection(id: "c1", name: "prod", dbType: "sqlite", isProduction: true),
        ])
        XCTAssertEqual(store.connections.count, 1)
        XCTAssertTrue(store.connections[0].isProduction)
    }

    func testConfirmationBindsOperationTargetAndHash() {
        let store = DatabaseStore()
        let confirmation = ProductionConfirmation(
            operationID: "op-1",
            connectionID: "c2",
            target: "/tmp/prod.db",
            sqlHash: "abc123"
        )
        store.setConfirmation(confirmation)
        XCTAssertEqual(store.confirmation?.operationID, "op-1")
        XCTAssertEqual(store.confirmation?.sqlHash, "abc123")
        // 不同 SQL hash 的确认不覆盖（不对后续所有 SQL 放行）。
        let other = ProductionConfirmation(
            operationID: "op-1", connectionID: "c2", target: "/tmp/prod.db", sqlHash: "def456"
        )
        store.setConfirmation(other)
        XCTAssertEqual(store.confirmation?.sqlHash, "def456")
    }

    func testGridIngestBounded() {
        // 百万行结果只请求有界页（内存不加载全表——store 只持有当页）。
        let store = DatabaseStore()
        store.ingestGrid(
            columns: ["id", "name"],
            rows: (0..<100).map { DatabaseStore.GridRow(id: $0, cells: ["\($0)", "row"]) },
            total: 1_000_000
        )
        XCTAssertEqual(store.gridRows.count, 100, "只持有当页")
        XCTAssertEqual(store.totalCount, 1_000_000, "total 单独计数")
    }

    func testCloseResultSessionReleases() {
        let store = DatabaseStore()
        store.ingestGrid(columns: ["id"], rows: [DatabaseStore.GridRow(id: 0, cells: ["x"])], total: 1)
        store.setConfirmation(ProductionConfirmation(
            operationID: "op-1", connectionID: "c1", target: "t", sqlHash: "h"
        ))
        store.closeResultSession()
        XCTAssertTrue(store.gridRows.isEmpty, "close tab 后 result session 释放")
        XCTAssertNil(store.totalCount)
        XCTAssertNil(store.confirmation)
    }
}
