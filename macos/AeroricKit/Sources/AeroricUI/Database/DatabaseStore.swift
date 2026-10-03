import Foundation
import Observation

/// P7-04：database 簿记（Action 2 十视图的数据源；快照来自宿主应答，本类型
/// 只记账——**一次批准绑定 operationId+目标+SQL hash，不对后续所有 SQL 放
/// 行**；两个窗口确认不能串 SQL）。
@MainActor
@Observable
public final class DatabaseStore {
    public struct Connection: Identifiable, Equatable {
        public var id: String
        public var name: String
        public var dbType: String
        public var isProduction: Bool

        public init(id: String, name: String, dbType: String, isProduction: Bool) {
            self.id = id
            self.name = name
            self.dbType = dbType
            self.isProduction = isProduction
        }
    }

    public struct GridRow: Identifiable {
        public var id: Int
        public var cells: [String]

        public init(id: Int, cells: [String]) {
            self.id = id
            self.cells = cells
        }
    }

    public private(set) var connections: [Connection] = []
    public private(set) var selectedConnectionID: String?
    /// 结果网格（有界页；百万行只请求有界页，内存不加载全表）。
    public private(set) var gridRows: [GridRow] = []
    public private(set) var gridColumns: [String] = []
    public private(set) var totalCount: UInt64?
    /// 生产确认（一次批准绑定 operationId+目标+SQL hash；两窗口不串 SQL）。
    public private(set) var confirmation: ProductionConfirmation?
    /// debug log 不输出 connection string/password（store 无该字段——
    /// Connection 条目无 password）。

    public init() {}

    public func ingest(connections: [Connection]) {
        self.connections = connections
    }

    public func selectConnection(_ id: String) {
        selectedConnectionID = id
    }

    public func ingestGrid(columns: [String], rows: [GridRow], total: UInt64?) {
        gridColumns = columns
        gridRows = rows
        totalCount = total
    }

    /// 生产确认（一次批准绑定 operationId+目标+SQL hash——确认由用户触发，
    /// 不同 hash 不放行）。
    public func setConfirmation(_ confirmation: ProductionConfirmation?) {
        self.confirmation = confirmation
    }

    /// 关 tab（close tab 后 result session 释放——网格清空）。
    public func closeResultSession() {
        gridRows = []
        gridColumns = []
        totalCount = nil
        confirmation = nil
    }
}

/// 生产确认（一次批准绑定 operationId+目标+SQL hash；与 core 形状对齐）。
public struct ProductionConfirmation: Equatable, Sendable {
    public var operationID: String
    public var connectionID: String
    public var target: String
    public var sqlHash: String

    public init(operationID: String, connectionID: String, target: String, sqlHash: String) {
        self.operationID = operationID
        self.connectionID = connectionID
        self.target = target
        self.sqlHash = sqlHash
    }
}
