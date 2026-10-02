import SwiftUI

/// P6-02 Action 3：编辑协调器（保存/格式化/历史恢复的编排；wire 调用由
/// 调用方提交，这里维护 dirty/冲突簿记与保存流程的顺序契约）。
@MainActor
@Observable
public final class EditorCoordinator {
    public let store: DocumentStore

    public init(store: DocumentStore) {
        self.store = store
    }

    /// 保存顺序契约：先快照（宿主 record_snapshot_before_write）→ 写 →
    /// markSaved（基线推进）。冲突（宿主拒绝覆盖）→ recordConflict（不可
    /// 静默覆盖）。
    public enum SaveOutcome: Equatable {
        case saved
        case conflict
        case rejected(reason: String)
    }

    public func handleSaveResult(path: String, result: Result<Void, Error>, savedContent: String?) -> SaveOutcome {
        switch result {
        case .success:
            if let savedContent {
                store.markSaved(path: path, diskContent: savedContent)
            }
            return .saved
        case let .failure(error):
            let reason = String(describing: error)
            if reason.lowercased().contains("conflict") || reason.contains("外部修改") || reason.lowercased().contains("modified") {
                store.recordConflict(path: path)
                return .conflict
            }
            return .rejected(reason: reason)
        }
    }

    /// 格式化失败原文不变（宿主 formatter 非零退出即拒绝）；本方法只记账。
    public func handleFormatResult(path: String, result: Result<Void, Error>) {
        // 格式化成功后磁盘内容变了——下轮刷新校正 diskContent 基线（快照轮询）。
    }
}

/// P6-02 Action 2：编辑 tab（多文档；未保存提示——关 tab 明确
/// save/discard/cancel）。
public struct EditorTabs: View {
    let store: DocumentStore
    public var onClose: ((_ path: String) -> Void)?

    public init(store: DocumentStore, onClose: ((_ path: String) -> Void)? = nil) {
        self.store = store
        self.onClose = onClose
    }

    public var body: some View {
        HStack(spacing: 2) {
            ForEach(store.documents) { doc in
                HStack(spacing: 4) {
                    if doc.content != doc.diskContent {
                        Circle()
                            .fill(Color.dsWarning)
                            .frame(width: 6, height: 6)
                            .accessibilityIdentifier("editor.dirty.\(doc.path)")
                    }
                    Text((doc.path as NSString).lastPathComponent)
                        .font(.caption)
                    Button("关闭") { onClose?(doc.path) }
                        .font(.caption2)
                        .accessibilityIdentifier("editor.close.\(doc.path)")
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(doc.path == store.selectedPath ? Color.dsSurface : Color.clear)
                .onTapGesture { store.select(doc.path) }
                .accessibilityIdentifier("editor.tab.\(doc.path)")
            }
        }
        .padding(4)
        .accessibilityIdentifier("editor.tabs")
    }
}

/// 保存冲突单（autosave 版本冲突不可静默覆盖——三选：覆盖/save as/放弃）。
public struct SaveConflictSheet: View {
    public let path: String
    public var onOverwrite: () -> Void
    public var onSaveAs: () -> Void
    public var onDiscard: () -> Void
    public var onCancel: () -> Void

    public init(
        path: String,
        onOverwrite: @escaping () -> Void,
        onSaveAs: @escaping () -> Void,
        onDiscard: @escaping () -> Void,
        onCancel: @escaping () -> Void
    ) {
        self.path = path
        self.onOverwrite = onOverwrite
        self.onSaveAs = onSaveAs
        self.onDiscard = onDiscard
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("文件已被外部修改")
                .font(.headline)
                .accessibilityIdentifier("editor.conflict.title")
            Text("磁盘上的 \(path) 内容与打开时不同；不可静默覆盖。")
                .font(.caption)
                .foregroundStyle(Color.dsTextSecondary)
            HStack(spacing: 8) {
                Button("覆盖磁盘") { onOverwrite() }
                    .accessibilityIdentifier("editor.conflict.overwrite")
                Button("另存为…") { onSaveAs() }
                    .accessibilityIdentifier("editor.conflict.save-as")
                Button("放弃编辑") { onDiscard() }
                    .accessibilityIdentifier("editor.conflict.discard")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("editor.conflict.cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 380)
        .accessibilityIdentifier("editor.conflict.sheet")
    }
}

/// P6-02 Action 1：本地历史面板（列出/读取/恢复——恢复有预览/确认且可
/// 撤回：宿主 restore 前兜底快照）。
public struct LocalHistoryPanel: View {
    public struct Entry: Identifiable, Equatable {
        public var id: String
        public var createdAtMs: UInt64
        public var size: UInt64

        public init(id: String, createdAtMs: UInt64, size: UInt64) {
            self.id = id
            self.createdAtMs = createdAtMs
            self.size = size
        }
    }

    public let entries: [Entry]
    public var onPreview: ((_ entryID: String) -> Void)?
    public var onRestore: ((_ entryID: String) -> Void)?
    @State private var previewingEntryID: String?
    @State private var pendingRestoreID: String?

    public init(
        entries: [Entry],
        onPreview: ((_ entryID: String) -> Void)? = nil,
        onRestore: ((_ entryID: String) -> Void)? = nil
    ) {
        self.entries = entries
        self.onPreview = onPreview
        self.onRestore = onRestore
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("本地历史")
                .font(.headline)
            List(entries) { entry in
                HStack {
                    Text(Self.timeText(entry.createdAtMs))
                        .font(.caption)
                    Spacer()
                    Text("\(entry.size) B")
                        .font(.caption2)
                        .foregroundStyle(Color.dsTextMuted)
                    Button("预览") {
                        previewingEntryID = entry.id
                        onPreview?(entry.id)
                    }
                    .accessibilityIdentifier("editor.history.preview.\(entry.id)")
                    Button("恢复") { pendingRestoreID = entry.id }
                        .accessibilityIdentifier("editor.history.restore.\(entry.id)")
                }
            }
            .frame(minHeight: 160)
            .accessibilityIdentifier("editor.history.list")
            if let pendingRestoreID {
                // 恢复确认（预览/确认且可撤回——宿主 restore 前兜底快照）。
                HStack(spacing: 8) {
                    Text("恢复该快照？（恢复前会自动记录当前内容的兜底快照）")
                        .font(.caption)
                    Button("确认恢复") {
                        onRestore?(pendingRestoreID)
                        self.pendingRestoreID = nil
                    }
                    .accessibilityIdentifier("editor.history.confirm")
                    Button("取消") { self.pendingRestoreID = nil }
                        .accessibilityIdentifier("editor.history.confirm-cancel")
                }
            }
        }
        .padding(12)
        .accessibilityIdentifier("editor.history-panel")
    }

    static func timeText(_ ms: UInt64) -> String {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000).formatted(date: .abbreviated, time: .shortened)
    }
}
