import AeroricClient
import SwiftUI

/// Narrow locator only. No editor, diff, mutation toolbar or history lives
/// here; file/change selection forwards one typed target to the centre.
public struct ProjectResourceInspector: View {
    let book: ProjectResourceBook
    let projectID: String
    let projectName: String
    let onOpen: (ResourcePreviewTarget) -> Void
    let onLoadDirectory: (String) -> Void
    let onRefresh: () -> Void

    public init(book: ProjectResourceBook, projectID: String, projectName: String,
                onOpen: @escaping (ResourcePreviewTarget) -> Void,
                onLoadDirectory: @escaping (String) -> Void, onRefresh: @escaping () -> Void) {
        self.book = book; self.projectID = projectID; self.projectName = projectName
        self.onOpen = onOpen; self.onLoadDirectory = onLoadDirectory; self.onRefresh = onRefresh
    }
    private var scene: ProjectResourceScene { book.scene(projectID) }
    public var body: some View {
        VStack(spacing: 6) {
            NativeSegmentedPicker(
                selection: Binding(get: { scene.section }, set: { section in
                    book.edit(projectID) { $0.section = section }
                }),
                groupTitle: "项目资源", segments: [
                    .init(value: .files, title: "文件"), .init(value: .git, title: "Git"),
                ]
            )
            .controlSize(.small)
            .accessibilityIdentifier("resources.section")
            HStack(spacing: 4) {
                Text(projectName).lineLimit(1).truncationMode(.middle).help(projectName)
                Spacer(minLength: 2)
                NativeIconButton(systemImage: "arrow.clockwise", label: "刷新\(projectName)资源",
                                 identifier: "resources.refresh", action: onRefresh)
            }
            .font(AeroricFont.mono(AeroricFont.meta))
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
        .padding(8)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

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
    private var fileSelection: Binding<String?> {
        Binding(get: { scene.target?.kind == .file ? scene.target?.path : nil }, set: { path in
            guard let path, rows.contains(where: { $0.id == path && !$0.node.isDir }) else { return }
            onOpen(.init(projectID: projectID, path: path, kind: .file))
        })
    }
    private var fileList: some View {
        VStack(alignment: .leading, spacing: 4) {
            readNotice(scene.directoryStates[""] ?? .notLoaded, hasCache: scene.directories[""] != nil)
            if scene.directoryStates[""] == .loaded, scene.directories[""]?.isEmpty == true {
                Text("目录为空").font(AeroricFont.mono(AeroricFont.meta))
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
                                HStack(spacing: 5) {
                                    Image(systemName: scene.expandedPaths.contains(row.id) ? "chevron.down" : "chevron.right")
                                        .frame(width: 10)
                                    resourceName(row.node.name, symbol: row.node.isSymlink ? "link" : "folder")
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.borderless)
                            .help(row.node.path)
                            .accessibilityLabel("\(row.node.name)，目录")
                            if scene.expandedPaths.contains(row.id) {
                                if let status = scene.directoryStates[row.id], status != .loaded {
                                    readNotice(status, hasCache: scene.directories[row.id] != nil)
                                    Button("重试此目录") { onLoadDirectory(row.id) }.buttonStyle(.borderless)
                                } else if scene.directories[row.id]?.isEmpty == true {
                                    Text("空目录").font(AeroricFont.mono(AeroricFont.meta))
                                }
                            }
                        }
                        .tag(Optional<String>.none)
                        .listRowInsets(insets(row.depth))
                    } else {
                        resourceName(row.node.name, symbol: row.node.isSymlink ? "link" : "doc")
                            .tag(Optional(row.id))
                            .help(row.node.path)
                            .accessibilityLabel("\(row.node.name)，只读预览，\(row.node.path)")
                            .listRowInsets(insets(row.depth))
                    }
                }
                .listRowSeparator(.hidden)
            }
            .listStyle(.sidebar)
            .scrollContentBackground(.hidden)
            .environment(\.defaultMinListRowHeight, NativePanelGeometry.mainShell.resourceRowHeight)
            .accessibilityIdentifier("resources.files")
        }
    }
    private var gitSelection: Binding<String?> {
        Binding(get: {
            guard let target = scene.target, target.kind != .file else { return nil }
            return "\(target.kind == .stagedDiff):\(target.path)"
        }, set: { id in
            guard let change = scene.changes.first(where: { $0.id == id }) else { return }
            onOpen(.init(projectID: projectID, path: change.path,
                         kind: change.status == "?" ? .untracked : change.staged ? .stagedDiff : .unstagedDiff))
        })
    }
    private var gitList: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 4) {
                Image(systemName: "arrow.triangle.branch")
                Text(scene.branches.first(where: \.current)?.name ?? "分支未知").lineLimit(1)
            }
            .font(AeroricFont.mono(AeroricFont.meta))
            readNotice(scene.branchState, hasCache: !scene.branches.isEmpty)
            readNotice(scene.gitState, hasCache: !scene.changes.isEmpty)
            if scene.gitState == .loaded, scene.changes.isEmpty {
                Text("此快照没有变更").font(AeroricFont.mono(AeroricFont.meta))
            }
            NativeSelectionList(selection: gitSelection) {
                changeGroup("已暂存", changes: scene.changes.filter(\.staged))
                changeGroup("未暂存", changes: scene.changes.filter { !$0.staged && $0.status != "?" })
                changeGroup("未跟踪", changes: scene.changes.filter { !$0.staged && $0.status == "?" })
            }
            .listStyle(.sidebar)
            .scrollContentBackground(.hidden)
            .environment(\.defaultMinListRowHeight, NativePanelGeometry.mainShell.resourceRowHeight)
            .accessibilityIdentifier("resources.git")
        }
    }
    private func changeGroup(_ title: String, changes: [ResourceGitChange]) -> some View {
        Section {
            ForEach(changes) { change in
                HStack(spacing: 5) {
                    Text(change.status).foregroundStyle(Color.dsTextSecondary).frame(width: 15)
                    Text((change.path as NSString).lastPathComponent).lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: 0)
                }
                .font(AeroricFont.mono(AeroricFont.file))
                .frame(minHeight: NativePanelGeometry.mainShell.resourceRowHeight)
                .help(change.path)
                .accessibilityLabel("\(title)，\(change.status)，\(change.path)")
                .tag(Optional(change.id))
                .listRowInsets(insets(0))
                .listRowSeparator(.hidden)
            }
        } header: {
            Text("\(title) · \(changes.count)").font(AeroricFont.mono(AeroricFont.meta))
        }
    }
    private func resourceName(_ name: String, symbol: String) -> some View {
        HStack(spacing: 5) {
            Image(systemName: symbol).frame(width: 14).foregroundStyle(Color.dsTextSecondary)
            Text(name).lineLimit(1).truncationMode(.middle)
            Spacer(minLength: 0)
        }
        .font(AeroricFont.mono(AeroricFont.file))
        .frame(minHeight: NativePanelGeometry.mainShell.resourceRowHeight)
    }
    private func insets(_ depth: Int) -> EdgeInsets {
        EdgeInsets(top: 0, leading: CGFloat(depth) * 12 + 2, bottom: 0, trailing: 2)
    }
    @ViewBuilder private func readNotice(_ status: ResourceReadStatus, hasCache: Bool) -> some View {
        if let notice = status.notice {
            Text(notice + (hasCache ? "（保留缓存，版本未知）" : ""))
                .font(AeroricFont.mono(AeroricFont.meta)).foregroundStyle(Color.dsTextSecondary)
                .textSelection(.enabled)
        } else {
            Text("宿主快照 · 版本未知")
                .font(AeroricFont.mono(AeroricFont.meta)).foregroundStyle(Color.dsTextMuted)
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
    public init(book: ProjectResourceBook, target: ResourcePreviewTarget, projectName: String,
                projectAvailable: Bool, onRetry: @escaping () -> Void, onOpenFile: @escaping () -> Void) {
        self.book = book; self.target = target; self.projectName = projectName
        self.projectAvailable = projectAvailable; self.onRetry = onRetry; self.onOpenFile = onOpenFile
    }
    private var scene: ProjectResourceScene { book.scene(target.projectID) }
    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 5) {
                HStack {
                    Text("\(projectName) · \(target.kind.label)")
                    Spacer()
                    NativeIconButton(systemImage: "arrow.clockwise", label: "重新读取此预览",
                                     identifier: "preview.retry", action: onRetry)
                        .disabled(!projectAvailable)
                }
                Text(target.path).font(AeroricFont.mono(AeroricFont.file)).textSelection(.enabled)
                Text("只读 · 宿主快照 · 版本未知").foregroundStyle(Color.dsTextMuted)
                if target.kind != .file {
                    Text("只读 · 宿主完整 diff（超过 1 MiB 会明确失败，不截断）")
                        .foregroundStyle(Color.dsTextSecondary)
                }
            }
            .font(AeroricFont.mono(AeroricFont.meta))
            .padding(12)
            Divider()
            if !projectAvailable {
                PaneEmptyState(systemImage: "folder.badge.questionmark", title: "原项目不可用",
                               hint: "保留原预览身份，不会选择其他项目代替。")
            } else {
                if let notice = scene.previewState.notice {
                    Text(notice + (scene.previewText == nil ? "" : "（以下为缓存，版本未知）"))
                        .font(AeroricFont.mono(AeroricFont.meta)).textSelection(.enabled).padding(12)
                }
                if scene.previewState == .loading { ProgressView().controlSize(.small).padding(12) }
                if let text = scene.previewText {
                    if text.isEmpty {
                        Text(target.kind == .file ? "空文件" : "此文件没有可显示的文本 diff")
                            .font(AeroricFont.mono(AeroricFont.meta)).padding(12)
                    }
                    sourceLines
                } else {
                    Spacer(minLength: 0)
                }
                if target.kind == .untracked {
                    Button("只读查看原文件", action: onOpenFile).buttonStyle(.borderless).padding(12)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color.dsTextBackground)
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
                            .font(AeroricFont.mono(AeroricFont.code))
                            .foregroundStyle(line.kind == .added ? Color.dsSuccess
                                             : line.kind == .removed ? Color.dsDanger : Color.dsText)
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
            .font(AeroricFont.mono(AeroricFont.meta)).foregroundStyle(Color.dsTextMuted)
            .frame(minWidth: 32, alignment: .trailing)
            .accessibilityHidden(true)
    }
}
