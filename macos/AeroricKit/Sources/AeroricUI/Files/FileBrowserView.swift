import AeroricClient
import SwiftUI

/// P6-01 Action 4：文件浏览器视图（树懒加载＋操作菜单）。树懒加载（首次
/// 展开才拉子目录）；操作走 FilesOperationSheet（新建/重命名/删除——删除
/// 进废纸篓）。
public struct FileBrowserView: View {
    let store: FileTreeStore
    public var onExpand: ((_ path: String) -> Void)?
    public var onOperation: ((_ operation: FilesOperation, _ path: String) -> Void)?
    /// A file row was clicked (directories toggle instead).
    public var onOpen: ((_ path: String) -> Void)?

    public init(
        store: FileTreeStore,
        onExpand: ((_ path: String) -> Void)? = nil,
        onOperation: ((_ operation: FilesOperation, _ path: String) -> Void)? = nil,
        onOpen: ((_ path: String) -> Void)? = nil
    ) {
        self.store = store
        self.onExpand = onExpand
        self.onOperation = onOperation
        self.onOpen = onOpen
    }

    public var body: some View {
        List {
            rootRows(path: "")
        }
        .listStyle(.plain)
        .accessibilityIdentifier("files.browser")
    }

    @ViewBuilder
    private func rootRows(path: String) -> some View {
        if let nodes = store.visibleChildren(of: path) {
            ForEach(nodes) { node in
                FileRow(
                    node: node,
                    isExpanded: store.expandedPaths.contains(node.path),
                    isLoading: store.isLoading(node.path),
                    onToggle: {
                        if store.toggleExpanded(node.path) {
                            onExpand?(node.path)
                        }
                    },
                    onOperation: { operation in
                        onOperation?(operation, node.path)
                    },
                    onOpen: onOpen.map { open in { open(node.path) } },
                    allowsOperations: onOperation != nil
                )
                if store.expandedPaths.contains(node.path) {
                    FileTreeBranch(store: store, path: node.path, onExpand: onExpand, onOperation: onOperation,
                                   onOpen: onOpen)
                }
            }
        } else if store.isLoading(path) {
            ProgressView()
                .padding(.leading, 24)
        }
    }
}

/// 树分支（递归用独立 struct：@ViewBuilder 递归函数的 opaque type 自引用
/// 无法编译——SwiftUI 递归的标准出路）。
struct FileTreeBranch: View {
    let store: FileTreeStore
    let path: String
    var onExpand: ((_ path: String) -> Void)?
    var onOperation: ((_ operation: FilesOperation, _ path: String) -> Void)?
    var onOpen: ((_ path: String) -> Void)?

    var body: some View {
        FileBrowserBranchRows(store: store, path: path, onExpand: onExpand, onOperation: onOperation, onOpen: onOpen)
    }
}

struct FileBrowserBranchRows: View {
    let store: FileTreeStore
    let path: String
    var onExpand: ((_ path: String) -> Void)?
    var onOperation: ((_ operation: FilesOperation, _ path: String) -> Void)?
    var onOpen: ((_ path: String) -> Void)?

    var body: some View {
        if let nodes = store.visibleChildren(of: path) {
            ForEach(nodes) { node in
                FileRow(
                    node: node,
                    isExpanded: store.expandedPaths.contains(node.path),
                    isLoading: store.isLoading(node.path),
                    onToggle: {
                        if store.toggleExpanded(node.path) {
                            onExpand?(node.path)
                        }
                    },
                    onOperation: { operation in
                        onOperation?(operation, node.path)
                    },
                    onOpen: onOpen.map { open in { open(node.path) } },
                    allowsOperations: onOperation != nil
                )
                if store.expandedPaths.contains(node.path) {
                    FileBrowserBranchRows(
                        store: store, path: node.path, onExpand: onExpand, onOperation: onOperation, onOpen: onOpen
                    )
                }
            }
        } else if store.isLoading(path) {
            ProgressView()
                .padding(.leading, 24)
        }
    }
}

/// P6-01 Action 4：单行（懒加载箭头＋symlink 标记＋操作菜单）。
public struct FileRow: View {
    let node: FileTreeStore.Node
    var isExpanded: Bool
    var isLoading: Bool
    var onToggle: () -> Void
    var onOperation: (FilesOperation) -> Void
    var onOpen: (() -> Void)?
    /// Without an operation handler the "…" menu would offer actions that do nothing.
    var allowsOperations: Bool

    public init(
        node: FileTreeStore.Node,
        isExpanded: Bool,
        isLoading: Bool,
        onToggle: @escaping () -> Void,
        onOperation: @escaping (FilesOperation) -> Void,
        onOpen: (() -> Void)? = nil,
        allowsOperations: Bool = true
    ) {
        self.node = node
        self.isExpanded = isExpanded
        self.isLoading = isLoading
        self.onToggle = onToggle
        self.onOperation = onOperation
        self.onOpen = onOpen
        self.allowsOperations = allowsOperations
    }

    public var body: some View {
        HStack(spacing: 6) {
            if node.isDir {
                Button {
                    onToggle()
                } label: {
                    Image(systemName: isLoading ? "loading" : (isExpanded ? "chevron.down" : "chevron.right"))
                        .font(.caption2)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("files.toggle.\(node.path)")
                Image(systemName: "folder")
                    .foregroundStyle(AeroricColor.accent)
            } else {
                Image(systemName: node.isSymlink ? "link" : "doc")
                    .foregroundStyle(AeroricColor.textSecondary)
                    .padding(.leading, 14)
            }
            Text(node.name)
                .lineLimit(1)
            Spacer()
            if allowsOperations { Menu("…") {
                if node.isDir {
                    Button("新建文件…") { onOperation(.createFile(path: node.path)) }
                    Button("新建文件夹…") { onOperation(.createDirectory(path: node.path)) }
                }
                Button("重命名…") { onOperation(.rename(path: node.path)) }
                Button("删除（进废纸篓）", role: .destructive) { onOperation(.delete(path: node.path)) }
            }
            .accessibilityIdentifier("files.menu.\(node.path)") }
        }
        .contentShape(Rectangle())
        .onTapGesture { if node.isDir { onToggle() } else { onOpen?() } }
        .accessibilityIdentifier("files.row.\(node.path)")
    }
}

/// 文件操作（新建/重命名/删除三分；删除进废纸篓——UI 不提供永久删除入口）。
public enum FilesOperation: Equatable {
    case createFile(path: String)
    case createDirectory(path: String)
    case rename(path: String)
    case delete(path: String)
}

/// 操作表单（Action 4：FileOperationSheet——输入名＋提交；无效名由宿主拒绝，
/// 客户端不做路径校验替代）。
public struct FileOperationSheet: View {
    let operation: FilesOperation
    public var onSubmit: (_ parentPath: String, _ name: String) -> Void
    public var onCancel: () -> Void
    @State private var name = ""

    public init(
        operation: FilesOperation,
        onSubmit: @escaping (_ parentPath: String, _ name: String) -> Void,
        onCancel: @escaping () -> Void
    ) {
        self.operation = operation
        self.onSubmit = onSubmit
        self.onCancel = onCancel
    }

    private var parentPath: String {
        switch operation {
        case let .createFile(path), let .createDirectory(path): path
        case let .rename(path), let .delete(path): path
        }
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title)
                .font(.headline)
            TextField("名称", text: $name)
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("files.operation.name")
            HStack {
                Button("确定") { onSubmit(parentPath, name) }
                    .disabled(name.isEmpty)
                    .accessibilityIdentifier("files.operation.submit")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("files.operation.cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 320)
        .accessibilityIdentifier("files.operation.sheet")
    }

    private var title: String {
        switch operation {
        case .createFile: "新建文件"
        case .createDirectory: "新建文件夹"
        case .rename: "重命名"
        case .delete: "删除（进废纸篓）"
        }
    }
}

/// 搜索面板（Action 4：搜索结果支持 cursor——结果分页由宿主 limit 承载，
/// 客户端只展示与触发下一页）。
public struct SearchPanel: View {
    public struct Match: Identifiable {
        public var id: String { "\(path):\(line)" }
        public var path: String
        public var line: UInt64
        public var excerpt: String

        public init(path: String, line: UInt64, excerpt: String) {
            self.path = path
            self.line = line
            self.excerpt = excerpt
        }
    }

    let matches: [Match]
    public var onQuery: ((_ query: String) -> Void)?
    @State private var query = ""

    public init(matches: [Match], onQuery: ((_ query: String) -> Void)? = nil) {
        self.matches = matches
        self.onQuery = onQuery
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("搜索…", text: $query)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { onQuery?(query) }
                    .accessibilityIdentifier("files.search.query")
                Button("搜索") { onQuery?(query) }
                    .accessibilityIdentifier("files.search.submit")
            }
            .padding(8)
            if matches.isEmpty {
                // AP8 2026-10-13: 与连接面板同一缺陷——空结果与"渲染失败"在视觉上
                // 无法区分。`Files` 工作区给 SearchPanel 传的是硬编码的 `matches: []`
                // （见 TaskWorkspaceView），所以这里**总是**空的，必须说清是"还没有结果"。
                VStack(spacing: 8) {
                    Image(systemName: "magnifyingglass")
                        .font(.system(size: 24))
                        .foregroundStyle(AeroricColor.textTertiary)
                    Text("没有搜索结果")
                        .font(.callout)
                        .foregroundStyle(AeroricColor.textSecondary)
                    Text("输入关键词后按回车或点「搜索」")
                        .font(.caption)
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("files.search.empty")
            } else {
                List(matches) { match in
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(match.path):\(match.line)")
                            .font(.caption)
                            .foregroundStyle(AeroricColor.textTertiary)
                        Text(match.excerpt)
                            .font(.caption.monospaced())
                            .lineLimit(2)
                    }
                    .accessibilityIdentifier("files.search.result.\(match.id)")
                }
                .accessibilityIdentifier("files.search.results")
            }
        }
        // No panel-level identifier here: a container identifier **replaces**
        // every descendant identifier in the AX tree, so `files.search.query`,
        // `files.search.empty` and `files.search.results` would all read as the
        // container id and none would be addressable. Measured 2026-10-14;
        // same rule as `WorkspaceDetail` / `BootstrapInfoBlock` / `DebugPanel`.
        // Identifiers live on the leaves only. See G51.
    }
}

/// 替换预览单（Action 4：替换先 preview，再提交匹配版本的改动——stale
/// 区间由宿主拒绝覆盖，UI 显示 skip 计数）。
public struct ReplacePreviewSheet: View {
    public let preview: FilesClient.ReplacePreview
    public var onApply: () -> Void
    public var onCancel: () -> Void

    public init(preview: FilesClient.ReplacePreview, onApply: @escaping () -> Void, onCancel: @escaping () -> Void) {
        self.preview = preview
        self.onApply = onApply
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("替换预览：\(preview.query) → \(preview.replacement)")
                .font(.headline)
            List(preview.files, id: \.path) { file in
                VStack(alignment: .leading, spacing: 2) {
                    Text(file.path)
                        .font(.callout)
                    Text("\(file.matches.count) 处命中")
                        .font(.caption)
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                .accessibilityIdentifier("files.replace.preview.\(file.path)")
            }
            HStack {
                Button("应用替换") { onApply() }
                    .accessibilityIdentifier("files.replace.apply")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("files.replace.cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 420, minHeight: 300)
        .accessibilityIdentifier("files.replace.sheet")
    }
}
