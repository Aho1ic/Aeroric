import AeroricClient
import QuickLook
import SwiftUI

/// Repository panel lists (M3): a file tree and the change list with inline
/// diffs. Read-only; selecting a file or "在中栏打开" forwards one typed
/// target to the centre preview. Files vs changes is chosen by the panel tabs.
public struct ProjectResourceInspector: View {
    let book: ProjectResourceBook
    let projectID: String
    let projectName: String
    let onOpen: (ResourcePreviewTarget) -> Void
    let onLoadDirectory: (String) -> Void
    let onRefresh: () -> Void
    let onToggleChange: (ResourceGitChange) -> Void
    /// The project's folder on this Mac; nil for remote projects, which have
    /// no local file for Quick Look.
    let localRoot: String?
    /// Space shows the system Quick Look panel for the selected file.
    @State private var quickLookURL: URL?

    public init(book: ProjectResourceBook, projectID: String, projectName: String,
                onOpen: @escaping (ResourcePreviewTarget) -> Void,
                onLoadDirectory: @escaping (String) -> Void, onRefresh: @escaping () -> Void,
                onToggleChange: @escaping (ResourceGitChange) -> Void = { _ in },
                localRoot: String? = nil) {
        self.book = book; self.projectID = projectID; self.projectName = projectName
        self.onOpen = onOpen; self.onLoadDirectory = onLoadDirectory; self.onRefresh = onRefresh
        self.onToggleChange = onToggleChange; self.localRoot = localRoot
    }

    private func localURL(_ path: String?) -> URL? {
        guard let localRoot, let path else { return nil }
        return URL(fileURLWithPath: localRoot).appendingPathComponent(path)
    }

    /// Space toggles Quick Look for `path`; ignored for remote projects.
    private func toggleQuickLook(_ path: String?) -> KeyPress.Result {
        if quickLookURL != nil { quickLookURL = nil; return .handled }
        guard let url = localURL(path) else { return .ignored }
        quickLookURL = url
        return .handled
    }

    /// Selected changed file, for Quick Look in the change list.
    private var expandedChangePath: String? {
        scene.changes.first { $0.id == scene.expandedChange }?.path
    }
    private var scene: ProjectResourceScene { book.scene(projectID) }
    static var rowHeight: CGFloat { 26 }
    static var indent: CGFloat { 14 }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            HairlineDivider()
            ZStack {
                fileList
                    .opacity(scene.section == .files ? 1 : 0)
                    .allowsHitTesting(scene.section == .files)
                    .accessibilityHidden(scene.section != .files)
                gitList
                    .opacity(scene.section == .git ? 1 : 0)
                    .allowsHitTesting(scene.section == .git)
                    .accessibilityHidden(scene.section != .git)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private var header: some View {
        HStack(spacing: AeroricSpace.s) {
            Image(systemName: scene.section == .files ? "folder" : "arrow.triangle.branch")
                .foregroundStyle(AeroricColor.textTertiary)
            Text(scene.section == .files ? projectName : (scene.branches.first(where: \.current)?.name ?? "分支未知"))
                .font(.system(size: AeroricType.secondary, weight: .medium))
                .lineLimit(1).truncationMode(.middle)
                .help(projectName)
            Spacer(minLength: AeroricSpace.xs)
            NativeIconButton(systemImage: "arrow.clockwise", label: "刷新\(projectName)资源",
                             identifier: "resources.refresh", action: onRefresh)
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 36)
    }

    // MARK: - Files

    private struct TreeRow: Identifiable {
        var id: String { node.path }
        let node: FilesClient.FileEntry
        let depth: Int
    }
    private var rows: [TreeRow] {
        func children(_ path: String, depth: Int) -> [TreeRow] {
            (scene.directories[path] ?? []).flatMap { node in
                [TreeRow(node: node, depth: depth)]
                    + (node.isDir && scene.expandedPaths.contains(node.path) ? children(node.path, depth: depth + 1) : [])
            }
        }
        return children("", depth: 0)
    }
    /// Single click only selects (2026-10-26 C); double click or Return opens
    /// a tab through the list's primary action.
    private var fileSelection: Binding<String?> {
        Binding(get: { scene.listSelection }, set: { path in
            guard let path, rows.contains(where: { $0.id == path && !$0.node.isDir }) else { return }
            book.edit(projectID) { $0.listSelection = path }
            // Quick Look follows the selection while its panel is open.
            if quickLookURL != nil { quickLookURL = localURL(path) }
        })
    }
    private var fileURLs: [URL] {
        rows.filter { !$0.node.isDir }.compactMap { localURL($0.id) }
    }
    private var fileList: some View {
        VStack(alignment: .leading, spacing: 0) {
            readNotice(scene.directoryStates[""] ?? .notLoaded, hasCache: scene.directories[""] != nil)
            if scene.directoryStates[""] == .loaded, scene.directories[""]?.isEmpty == true {
                AeroricEmptyState(systemImage: "folder", title: "目录为空")
            }
            NativeSelectionList(selection: fileSelection) {
                ForEach(rows) { row in
                    if row.node.isDir {
                        VStack(alignment: .leading, spacing: 0) {
                            Button {
                                let expanded = scene.expandedPaths.contains(row.id)
                                book.edit(projectID) {
                                    if expanded { $0.expandedPaths.remove(row.id) }
                                    else { $0.expandedPaths.insert(row.id) }
                                }
                                if !expanded, scene.directories[row.id] == nil { onLoadDirectory(row.id) }
                            } label: {
                                HStack(spacing: AeroricSpace.xs) {
                                    Image(systemName: "chevron.right")
                                        .font(.system(size: 9, weight: .semibold))
                                        .foregroundStyle(AeroricColor.textTertiary)
                                        .rotationEffect(.degrees(scene.expandedPaths.contains(row.id) ? 90 : 0))
                                        .frame(width: 10)
                                    resourceName(row.node.name, symbol: row.node.isSymlink ? "link" : "folder.fill",
                                                 tint: AeroricColor.accent.opacity(0.75))
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.borderless)
                            .help(row.node.path)
                            .accessibilityLabel("\(row.node.name)，目录")
                            if scene.expandedPaths.contains(row.id) {
                                if let status = scene.directoryStates[row.id], status != .loaded {
                                    readNotice(status, hasCache: scene.directories[row.id] != nil)
                                    Button("重试此目录") { onLoadDirectory(row.id) }.buttonStyle(.link)
                                } else if scene.directories[row.id]?.isEmpty == true {
                                    Text("空目录")
                                        .font(.system(size: AeroricType.caption))
                                        .foregroundStyle(AeroricColor.textTertiary)
                                        .padding(.leading, Self.indent + 10)
                                }
                            }
                        }
                        .tag(Optional<String>.none)
                        .listRowInsets(insets(row.depth))
                    } else {
                        resourceName(row.node.name,
                                     symbol: row.node.isSymlink ? "link" : FileSymbol.symbol(forFile: row.node.name),
                                     tint: AeroricColor.textSecondary)
                            .padding(.leading, 14)
                            .tag(Optional(row.id))
                            .help(row.node.path)
                            .accessibilityLabel("\(row.node.name)，只读预览，\(row.node.path)")
                            .listRowInsets(insets(row.depth))
                    }
                }
                .listRowSeparator(.hidden)
            }
            .contextMenu(forSelectionType: String?.self, menu: { _ in EmptyView() }, primaryAction: { paths in
                guard let path = paths.compactMap({ $0 }).first,
                      rows.contains(where: { $0.id == path && !$0.node.isDir }) else { return }
                onOpen(.init(projectID: projectID, path: path, kind: .file))
            })
            .onKeyPress(.space) { toggleQuickLook(scene.listSelection) }
            .quickLookPreview($quickLookURL, in: fileURLs)
            .listStyle(.sidebar)
            .scrollContentBackground(.hidden)
            .environment(\.defaultMinListRowHeight, Self.rowHeight)
            .accessibilityIdentifier("resources.files")
        }
    }

    // MARK: - Changes

    private var gitList: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: AeroricSpace.xxs) {
                readNotice(scene.branchState, hasCache: !scene.branches.isEmpty)
                readNotice(scene.gitState, hasCache: !scene.changes.isEmpty)
                if scene.gitState == .loaded, scene.changes.isEmpty {
                    AeroricEmptyState(systemImage: "checkmark.circle", title: "没有改动", message: "工作区与最近一次提交一致。")
                }
                changeGroup("已暂存", changes: scene.changes.filter(\.staged))
                changeGroup("未暂存", changes: scene.changes.filter { !$0.staged && $0.status != "?" })
                changeGroup("未跟踪", changes: scene.changes.filter { !$0.staged && $0.status == "?" })
            }
            .padding(.horizontal, AeroricSpace.s)
            .padding(.vertical, AeroricSpace.s)
        }
        .focusable()
        .focusEffectDisabled()
        .onKeyPress(.space) { toggleQuickLook(expandedChangePath) }
        .quickLookPreview($quickLookURL, in: scene.changes.compactMap { localURL($0.path) })
        .accessibilityIdentifier("resources.git")
    }

    @ViewBuilder private func changeGroup(_ title: String, changes: [ResourceGitChange]) -> some View {
        if !changes.isEmpty {
            SectionHeader("\(title) · \(changes.count)")
                .padding(.horizontal, AeroricSpace.s)
                .padding(.top, AeroricSpace.m)
            ForEach(changes) { change in
                changeRow(change)
                if scene.expandedChange == change.id { inlineDiff(change) }
            }
        }
    }

    private func changeRow(_ change: ResourceGitChange) -> some View {
        let badge = ChangeBadge(status: change.status)
        let name = (change.path as NSString).lastPathComponent
        let directory = (change.path as NSString).deletingLastPathComponent
        return HoverCapsuleRow(selected: scene.expandedChange == change.id, height: Self.rowHeight) {
            onToggleChange(change)
        } label: {
            HStack(spacing: AeroricSpace.s) {
                Text(badge.letter)
                    .font(.system(size: AeroricType.caption, weight: .bold, design: .monospaced))
                    .foregroundStyle(Self.badgeColor(badge))
                    .frame(width: 16, height: 16)
                    .background(Self.badgeColor(badge).opacity(0.14), in: AeroricRadius.shape(AeroricRadius.xs))
                Text(name)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.text)
                    .lineLimit(1)
                if !directory.isEmpty {
                    Text(directory)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .lineLimit(1).truncationMode(.head)
                }
                Spacer(minLength: 0)
            }
        }
        .help(change.path)
        .accessibilityLabel("\(badge.label)，\(change.path)")
        .accessibilityIdentifier("resources.change.\(change.id)")
    }

    @ViewBuilder private func inlineDiff(_ change: ResourceGitChange) -> some View {
        let diff = scene.inlineDiffs[change.id] ?? InlineDiff()
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            if let notice = diff.state.notice {
                Text(notice)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(diff.lines) { line in
                            Text(line.text.isEmpty ? " " : line.text)
                                .font(.system(size: AeroricType.code - 1, design: .monospaced))
                                .foregroundStyle(Self.lineColor(line.kind))
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(Self.lineBackground(line.kind))
                        }
                    }
                    .textSelection(.enabled)
                }
                if diff.totalLines > diff.lines.count {
                    Text("仅显示前 \(diff.lines.count) 行，共 \(diff.totalLines) 行")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
            Button("在中栏打开") {
                onOpen(.init(projectID: projectID, path: change.path,
                             kind: change.status == "?" ? .untracked : change.staged ? .stagedDiff : .unstagedDiff))
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .accessibilityIdentifier("resources.change.open-centre")
        }
        .padding(AeroricSpace.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.s))
        .overlay { AeroricRadius.shape(AeroricRadius.s).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
        .padding(.leading, AeroricSpace.xxxl)
        .padding(.vertical, AeroricSpace.xxs)
        .accessibilityIdentifier("resources.change.diff")
    }

    static func badgeColor(_ badge: ChangeBadge) -> Color {
        switch badge {
        case .modified: AeroricColor.warning
        case .added, .untracked: AeroricColor.success
        case .deleted: AeroricColor.danger
        case .renamed: AeroricColor.accent
        case .other: AeroricColor.textSecondary
        }
    }

    static func lineColor(_ kind: ResourcePreviewLine.Kind) -> Color {
        switch kind {
        case .metadata: AeroricColor.textTertiary
        default: AeroricColor.text
        }
    }

    static func lineBackground(_ kind: ResourcePreviewLine.Kind) -> Color {
        switch kind {
        case .added: AeroricColor.success.opacity(0.14)
        case .removed: AeroricColor.danger.opacity(0.14)
        default: .clear
        }
    }

    // MARK: - Shared

    private func resourceName(_ name: String, symbol: String, tint: Color) -> some View {
        HStack(spacing: AeroricSpace.s) {
            Image(systemName: symbol)
                .font(.system(size: 12))
                .frame(width: 16)
                .foregroundStyle(tint)
            Text(name)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.text)
                .lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 0)
        }
        .frame(minHeight: Self.rowHeight)
    }
    private func insets(_ depth: Int) -> EdgeInsets {
        EdgeInsets(top: 0, leading: CGFloat(depth) * Self.indent + AeroricSpace.s, bottom: 0, trailing: AeroricSpace.s)
    }
    @ViewBuilder private func readNotice(_ status: ResourceReadStatus, hasCache: Bool) -> some View {
        if let notice = status.notice {
            Text(notice + (hasCache ? "（保留缓存，版本未知）" : ""))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .textSelection(.enabled)
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.s)
        }
    }
}

public struct CentralResourcePreview: View {
    let book: ProjectResourceBook
    let target: ResourcePreviewTarget
    let projectName: String
    let projectAvailable: Bool
    let onRetry: () -> Void
    let onOpenFile: () -> Void
    /// Saves a `.file` tab's text; nil keeps the tab read-only.
    let onSave: ((String) -> Void)?
    public init(book: ProjectResourceBook, target: ResourcePreviewTarget, projectName: String,
                projectAvailable: Bool, onRetry: @escaping () -> Void, onOpenFile: @escaping () -> Void,
                onSave: ((String) -> Void)? = nil) {
        self.book = book; self.target = target; self.projectName = projectName
        self.projectAvailable = projectAvailable; self.onRetry = onRetry; self.onOpenFile = onOpenFile
        self.onSave = onSave
    }
    private var scene: ProjectResourceScene { book.scene(target.projectID) }
    /// A loaded text file with a save path is edited in place (2026-10-26 C).
    private var editable: Bool {
        target.kind == .file && onSave != nil && scene.previewState == .loaded && scene.previewText != nil
    }
    private var draft: Binding<String> {
        Binding(get: { scene.drafts[target] ?? scene.previewText ?? "" },
                set: { book.setDraft($0, for: target) })
    }
    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 5) {
                HStack {
                    Text("\(projectName) · \(editable ? "文件" : target.kind.label)")
                    Spacer()
                    if editable, book.isDirty(target) {
                        Button("保存") { onSave?(draft.wrappedValue) }
                            .keyboardShortcut("s")
                            .controlSize(.small)
                            .accessibilityIdentifier("preview.save")
                    }
                    NativeIconButton(systemImage: "arrow.clockwise", label: "重新读取此预览",
                                     identifier: "preview.retry", action: onRetry)
                        .disabled(!projectAvailable)
                }
                Text(target.path).font(AeroricType.mono(AeroricType.secondary)).textSelection(.enabled)
                Text(editable ? "可编辑 · ⌘S 保存" : "只读 · 宿主快照 · 版本未知")
                    .foregroundStyle(AeroricColor.textTertiary)
                if target.kind != .file {
                    Text("只读 · 宿主完整 diff（超过 1 MiB 会明确失败，不截断）")
                        .foregroundStyle(AeroricColor.textSecondary)
                }
            }
            .font(AeroricType.mono(AeroricType.caption))
            .padding(12)
            Divider()
            if !projectAvailable {
                PaneEmptyState(systemImage: "folder.badge.questionmark", title: "原项目不可用",
                               hint: "保留原预览身份，不会选择其他项目代替。")
            } else {
                if let notice = scene.previewState.notice {
                    Text(notice + (scene.previewText == nil ? "" : "（以下为缓存，版本未知）"))
                        .font(AeroricType.mono(AeroricType.caption)).textSelection(.enabled).padding(12)
                }
                if scene.previewState == .loading { ProgressView().controlSize(.small).padding(12) }
                if let text = scene.previewText {
                    if text.isEmpty {
                        Text(target.kind == .file ? "空文件" : "此文件没有可显示的文本 diff")
                            .font(AeroricType.mono(AeroricType.caption)).padding(12)
                    }
                    if editable {
                        TextEditor(text: draft)
                            .font(AeroricType.mono(AeroricType.code))
                            .scrollContentBackground(.hidden)
                            .padding(8)
                            .accessibilityIdentifier("preview.editor")
                    } else {
                        sourceLines
                    }
                } else {
                    Spacer(minLength: 0)
                }
                if target.kind == .untracked {
                    Button("只读查看原文件", action: onOpenFile).buttonStyle(.borderless).padding(12)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(AeroricColor.layer1)
        .accessibilityIdentifier("workspace.central-preview")
    }
    private var sourceLines: some View {
        ScrollView([.horizontal, .vertical]) {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(scene.previewLines) { line in
                    HStack(alignment: .top, spacing: 10) {
                        if target.kind != .file {
                            lineNumber(line.oldNumber)
                        }
                        lineNumber(line.newNumber)
                        Text(verbatim: line.text.isEmpty ? " " : line.text)
                            .font(AeroricType.mono(AeroricType.code))
                            .foregroundStyle(line.kind == .added ? AeroricColor.success
                                             : line.kind == .removed ? AeroricColor.danger : AeroricColor.text)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: true, vertical: true)
                    }
                    .frame(minHeight: 18, alignment: .topLeading)
                    .id(line.id)
                }
            }
            .scrollTargetLayout()
            .padding(12)
        }
        .scrollPosition(id: Binding(
            get: { scene.positions[target] },
            set: { book.rememberPosition($0, for: target) }
        ))
    }
    private func lineNumber(_ number: Int?) -> some View {
        Text(number.map(String.init) ?? "")
            .font(AeroricType.mono(AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
            .frame(minWidth: 32, alignment: .trailing)
            .accessibilityHidden(true)
    }
}
