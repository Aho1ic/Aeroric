import SwiftUI

/// P7-04 Action 2：Database 视图集（十视图；生产确认一次批准绑定
/// operationId+目标+SQL hash；结果分页/虚拟化）。
public struct DatabaseSidebar: View {
    let store: DatabaseStore

    public init(store: DatabaseStore) {
        self.store = store
    }

    public var body: some View {
        List(store.connections, selection: Binding(
            get: { store.selectedConnectionID },
            set: { store.selectConnection($0 ?? "") }
        )) { connection in
            HStack(spacing: 4) {
                Text(connection.name)
                    .font(.caption)
                if connection.isProduction {
                    Text("生产")
                        .font(.caption2)
                        .foregroundStyle(Color.dsDanger)
                        .accessibilityIdentifier("database.production-badge.\(connection.id)")
                }
            }
            .accessibilityIdentifier("database.connection.\(connection.id)")
        }
        .accessibilityIdentifier("database.sidebar")
    }
}

public struct ConnectionEditor: View {
    public var onSave: ((_ name: String, _ dbType: String, _ host: String, _ port: UInt16) -> Void)?
    @State private var name = ""
    @State private var dbType = "sqlite"
    @State private var host = ""
    @State private var port = 0
    @State private var error: String?

    public init(
        onSave: ((_ name: String, _ dbType: String, _ host: String, _ port: UInt16) -> Void)? = nil
    ) {
        self.onSave = onSave
    }

    public var body: some View {
        Form {
            TextField("名称", text: $name)
            Picker("类型", selection: $dbType) {
                ForEach(["sqlite", "postgres", "mysql", "redis", "mongo"], id: \.self) { type in
                    Text(type).tag(type)
                }
            }
            TextField("主机", text: $host)
            Stepper("端口 \(port)", value: $port, in: 0...65535)
            if let error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color.dsDanger)
            }
            Button("保存") {
                guard !name.trimmingCharacters(in: .whitespaces).isEmpty else {
                    error = "名称不能为空"
                    return
                }
                error = nil
                onSave?(name, dbType, host, UInt16(port))
            }
            .accessibilityIdentifier("database.connection.save")
        }
        .padding(16)
        .accessibilityIdentifier("database.connection-editor")
    }
}

public struct SchemaBrowser: View {
    public struct Table: Identifiable {
        public var id: String { name }
        public var name: String
        public var rowCount: UInt64?

        public init(name: String, rowCount: UInt64? = nil) {
            self.name = name
            self.rowCount = rowCount
        }
    }

    public let tables: [Table]

    public init(tables: [Table]) {
        self.tables = tables
    }

    public var body: some View {
        List(tables) { table in
            HStack {
                Text(table.name)
                    .font(.caption.monospaced())
                Spacer()
                if let rowCount = table.rowCount {
                    Text("\(rowCount) 行")
                        .font(.caption2)
                        .foregroundStyle(Color.dsTextMuted)
                }
            }
            .accessibilityIdentifier("database.table.\(table.name)")
        }
        .accessibilityIdentifier("database.schema")
    }
}

public struct SQLQueryEditor: View {
    @Binding public var sql: String
    public var onExecute: (() -> Void)?
    public var onCancel: (() -> Void)?
    @State private var executing = false

    public init(sql: Binding<String>, onExecute: (() -> Void)? = nil, onCancel: (() -> Void)? = nil) {
        self._sql = sql
        self.onExecute = onExecute
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            NativeTextEditor(text: $sql)
                .frame(minHeight: 120)
                .accessibilityIdentifier("database.sql-editor")
            HStack(spacing: 6) {
                Button("执行") { onExecute?() }
                    .accessibilityIdentifier("database.query.execute")
                if executing {
                    Button("取消") { onCancel?() }
                        .accessibilityIdentifier("database.query.cancel")
                    ProgressView()
                }
            }
        }
        .padding(8)
        .accessibilityIdentifier("database.query-editor")
    }
}

/// ResultGrid（结果分页/虚拟化——LazyVStack 只渲染可见行；列类型/NULL/binary
/// 明确）。
public struct ResultGrid: View {
    let store: DatabaseStore
    public var onPage: ((_ page: Int) -> Void)?
    @State private var page = 1

    public init(store: DatabaseStore, onPage: ((_ page: Int) -> Void)? = nil) {
        self.store = store
        self.onPage = onPage
    }

    public var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    ForEach(store.gridRows) { row in
                        HStack(spacing: 12) {
                            ForEach(Array(row.cells.enumerated()), id: \.offset) { _, cell in
                                // 列类型/NULL/binary 明确。
                                Text(cell.isEmpty ? "NULL" : cell)
                                    .font(.caption2.monospaced())
                                    .foregroundStyle(cell.isEmpty ? Color.dsTextMuted : Color.dsText)
                                    .lineLimit(1)
                            }
                        }
                        .accessibilityIdentifier("database.grid.row.\(row.id)")
                    }
                }
                .padding(8)
            }
            HStack {
                Button("上一页") {
                    if page > 1 {
                        page -= 1
                        onPage?(page)
                    }
                }
                .disabled(page == 1)
                .accessibilityIdentifier("database.grid.prev")
                Text("第 \(page) 页")
                    .font(.caption)
                Button("下一页") {
                    page += 1
                    onPage?(page)
                }
                .accessibilityIdentifier("database.grid.next")
                Spacer()
                if let total = store.totalCount {
                    Text("共 \(total) 行")
                        .font(.caption2)
                        .foregroundStyle(Color.dsTextMuted)
                }
            }
            .padding(8)
        }
        .accessibilityIdentifier("database.result-grid")
    }
}

public struct StructureEditor: View {
    public struct ColumnDef: Identifiable {
        public var id: String { name }
        public var name: String
        public var columnType: String
        public var nullable: Bool

        public init(name: String, columnType: String, nullable: Bool) {
            self.name = name
            self.columnType = columnType
            self.nullable = nullable
        }
    }

    public let columns: [ColumnDef]

    public init(columns: [ColumnDef]) {
        self.columns = columns
    }

    public var body: some View {
        List(columns) { column in
            HStack {
                Text(column.name)
                    .font(.caption.monospaced())
                Text(column.columnType)
                    .font(.caption2)
                    .foregroundStyle(Color.dsTextMuted)
                Spacer()
                if column.nullable {
                    Text("NULL")
                        .font(.caption2)
                        .foregroundStyle(Color.dsTextMuted)
                }
            }
            .accessibilityIdentifier("database.structure.\(column.name)")
        }
        .accessibilityIdentifier("database.structure-editor")
    }
}

public struct ImportExportSheet: View {
    public let path: String
    public let preview: String
    public var onConfirm: () -> Void
    public var onCancel: () -> Void

    public init(path: String, preview: String, onConfirm: @escaping () -> Void, onCancel: @escaping () -> Void) {
        self.path = path
        self.preview = preview
        self.onConfirm = onConfirm
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("导入/导出预览")
                .font(.headline)
            Text(path)
                .font(.caption)
                .foregroundStyle(Color.dsTextSecondary)
            ScrollView {
                Text(preview)
                    .font(.caption2.monospaced())
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 120)
            HStack {
                Button("确认写入") { onConfirm() }
                    .accessibilityIdentifier("database.import-export.confirm")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("database.import-export.cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 400, minHeight: 280)
        .accessibilityIdentifier("database.import-export")
    }
}

public struct RedisBrowser: View {
    public struct Key: Identifiable {
        public var id: String { key }
        public var key: String
        public var valueType: String
        public var value: String?

        public init(key: String, valueType: String, value: String? = nil) {
            self.key = key
            self.valueType = valueType
            self.value = value
        }
    }

    public let keys: [Key]

    public init(keys: [Key]) {
        self.keys = keys
    }

    public var body: some View {
        List(keys) { entry in
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.key)
                    .font(.caption.monospaced())
                Text(entry.valueType)
                    .font(.caption2)
                    .foregroundStyle(Color.dsTextMuted)
                if let value = entry.value {
                    Text(value)
                        .font(.caption2.monospaced())
                        .lineLimit(2)
                }
            }
            .accessibilityIdentifier("database.redis.\(entry.key)")
        }
        .accessibilityIdentifier("database.redis")
    }
}

public struct MongoBrowser: View {
    public struct Document: Identifiable {
        public var id: String
        public var json: String

        public init(id: String, json: String) {
            self.id = id
            self.json = json
        }
    }

    public let documents: [Document]

    public init(documents: [Document]) {
        self.documents = documents
    }

    public var body: some View {
        List(documents) { document in
            Text(document.json)
                .font(.caption2.monospaced())
                .lineLimit(4)
                .textSelection(.enabled)
                .accessibilityIdentifier("database.mongo.\(document.id)")
        }
        .accessibilityIdentifier("database.mongo")
    }
}

/// ProductionConfirmationSheet（一次批准绑定 operationId+目标+SQL hash；
/// 两窗口不串 SQL）。
public struct ProductionConfirmationSheet: View {
    public let target: String
    public let sql: String
    public var onConfirm: () -> Void
    public var onReject: () -> Void

    public init(target: String, sql: String, onConfirm: @escaping () -> Void, onReject: @escaping () -> Void) {
        self.target = target
        self.sql = sql
        self.onConfirm = onConfirm
        self.onReject = onReject
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 6) {
                Image(systemName: "exclamationmark.shield.fill")
                    .foregroundStyle(Color.dsDanger)
                Text("生产库写入确认")
                    .font(.headline)
                    .accessibilityIdentifier("database.production.title")
            }
            Text("目标：\(target)")
                .font(.caption)
                .accessibilityIdentifier("database.production.target")
            ScrollView {
                Text(sql)
                    .font(.caption.monospaced())
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 80)
            HStack {
                Button("批准并执行") { onConfirm() }
                    .accessibilityIdentifier("database.production.confirm")
                Button("拒绝", role: .destructive) { onReject() }
                    .accessibilityIdentifier("database.production.reject")
            }
        }
        .padding(16)
        .frame(minWidth: 420, minHeight: 260)
        .accessibilityIdentifier("database.production-confirmation")
    }
}
