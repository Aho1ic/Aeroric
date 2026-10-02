import Foundation
import Observation

/// P6-02 Action 3：document model（dirty/revision/selection 纯簿记；TextKit
/// 桥接与 UndoManager 在 NativeTextEditor/EditorCoordinator）。autosave 版本
/// 冲突不可静默覆盖——保存冲突由 SaveConflictSheet 呈现。
@MainActor
@Observable
public final class DocumentStore {
    public struct Document: Identifiable, Equatable {
        public var id: String { path }
        public var path: String
        /// 磁盘内容（打开/保存时的基线；冲突判定用）。
        public var diskContent: String
        /// 编辑中的内容（dirty 判定）。
        public var content: String
        /// 磁盘修订（每次保存递增；外部修改检测用）。
        public var revision: UInt64
        public var isReadOnly: Bool
        public var readOnlyReason: String?

        public init(
            path: String, diskContent: String, content: String, revision: UInt64 = 0,
            isReadOnly: Bool = false, readOnlyReason: String? = nil
        ) {
            self.path = path
            self.diskContent = diskContent
            self.content = content
            self.revision = revision
            self.isReadOnly = isReadOnly
            self.readOnlyReason = readOnlyReason
        }
    }

    public private(set) var documents: [Document] = []
    public private(set) var selectedPath: String?
    /// 保存冲突（宿主拒绝覆盖：磁盘内容与打开基线不符——不可静默覆盖，
    /// UI 呈现三选：save as/overwrite/discard）。
    public private(set) var conflictedPaths: Set<String> = []

    public init() {}

    public var selected: Document? {
        selectedPath.flatMap { path in documents.first { $0.path == path } }
    }

    public var isDirty: Bool {
        documents.contains { $0.content != $0.diskContent }
    }

    /// 打开（预览判定由宿主 PreviewDecision 承载，这里只记账）。
    public func open(_ document: Document) {
        if let index = documents.firstIndex(where: { $0.path == document.path }) {
            documents[index] = document
        } else {
            documents.append(document)
        }
        selectedPath = document.path
    }

    public func select(_ path: String) {
        selectedPath = path
    }

    public func updateContent(_ content: String, for path: String) {
        guard let index = documents.firstIndex(where: { $0.path == path }) else { return }
        documents[index].content = content
    }

    public func updateSelection(path: String, selection: Range<Int>) {
        // selection 簿记（TextKit 桥接回填）；这里不存——TextKit 自己持有。
    }

    /// 保存成功：diskContent 基线推进＋revision 递增＋冲突清除。
    public func markSaved(path: String, diskContent: String) {
        guard let index = documents.firstIndex(where: { $0.path == path }) else { return }
        documents[index].diskContent = diskContent
        documents[index].revision += 1
        conflictedPaths.remove(path)
    }

    /// 保存冲突（宿主拒绝覆盖）：记录冲突路径——不可静默覆盖。
    public func recordConflict(path: String) {
        conflictedPaths.insert(path)
    }

    public func clearConflict(path: String) {
        conflictedPaths.remove(path)
    }

    /// 关 tab（保留未保存文档——关 tab 明确 save/discard/cancel，这里只移除；
    /// 未保存提示由 EditorTabs 呈现）。
    @discardableResult
    public func close(path: String) -> Bool {
        // 返回是否 dirty（调用方决定提示）。
        let dirty = documents.first { $0.path == path }.map { $0.content != $0.diskContent } ?? false
        documents.removeAll { $0.path == path }
        conflictedPaths.remove(path)
        if selectedPath == path {
            selectedPath = documents.first?.path
        }
        return dirty
    }

    public func discardChanges(path: String) {
        guard let index = documents.firstIndex(where: { $0.path == path }) else { return }
        documents[index].content = documents[index].diskContent
    }
}
