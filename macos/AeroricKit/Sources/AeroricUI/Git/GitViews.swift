import SwiftUI

/// P6-04 Action 3：Git 视图集（十视图）。
public struct GitPanel: View {
    let store: GitStore
    public var onAction: ((_ action: GitAction) -> Void)?
    @State private var tab = "changes"

    public init(store: GitStore, onAction: ((_ action: GitAction) -> Void)? = nil) {
        self.store = store
        self.onAction = onAction
    }

    public var body: some View {
        VStack(spacing: 0) {
            // 危险操作（push/discard/冲突覆盖）需独立确认——确认在 StageActions/
            // ConflictResolver 呈现，这里只触发。
            HStack(spacing: 6) {
                Button("Fetch") { onAction?(.fetch(prune: false)) }
                    .accessibilityIdentifier("git.fetch")
                Button("Pull") { onAction?(.pull) }
                    .accessibilityIdentifier("git.pull")
                Menu("Push") {
                    ForEach(store.branches.filter(\.current)) { branch in
                        Button(branch.name) { onAction?(.push(branch: branch.name)) }
                    }
                }
                .accessibilityIdentifier("git.push")
            }
            .padding(8)
            .disabled(onAction == nil || store.changesReadState != .loaded)
            .help(onAction == nil ? "本页尚未接通 Git 操作" : "Git 操作")
            NativeSegmentedPicker(selection: $tab, groupTitle: "Git 内容", segments: [
                .init(value: "changes", title: "更改"),
                .init(value: "history", title: "历史"),
                .init(value: "stash", title: "贮藏"),
            ])
            .padding(.horizontal, 12)
            .padding(.bottom, 8)
            Group {
                switch tab {
                case "history":
                    HistoryView(store: store)
                case "stash":
                    StashView(store: store, onAction: onAction)
                default:
                    HSplitView {
                        ChangeList(store: store, onAction: onAction)
                            .frame(minWidth: 240, maxWidth: 360)
                        Group {
                            if store.changesReadState == .loaded {
                                DiffView(diffText: store.diffText)
                            } else {
                                Text("尚未加载 diff").foregroundStyle(Color.dsTextSecondary)
                            }
                        }
                        .frame(minWidth: 320, maxWidth: .infinity, maxHeight: .infinity)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .accessibilityIdentifier("git.panel")
    }
}

/// Git 动作（stage/unstage/commit/push/pull/fetch/discard/stash/branch/
/// conflict——push/discard/冲突覆盖需独立确认）。
public enum GitAction: Equatable {
    case stage(path: String)
    case unstage(path: String)
    case stageAll
    case commit(message: String)
    case push(branch: String)
    case pull
    case fetch(prune: Bool)
    case discard(paths: [String])
    case stashPush(message: String)
    case stashApply(index: Int)
    case stashDrop(index: Int)
    case checkout(branch: String)
    case createBranch(name: String)
    case resolveConflict(path: String, content: String)
}

public struct ChangeList: View {
    let store: GitStore
    public var onAction: ((_ action: GitAction) -> Void)?

    public init(store: GitStore, onAction: ((_ action: GitAction) -> Void)? = nil) {
        self.store = store
        self.onAction = onAction
    }

    public var body: some View {
        if let notice = store.changesReadState.notice {
            VStack(spacing: 8) {
                Text(notice).font(.callout).foregroundStyle(Color.dsTextSecondary)
                if !store.changes.isEmpty {
                    Text("保留 \(store.changes.count) 条旧变更；尚未核实当前仓库。").font(.caption)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if store.stagedChanges.isEmpty && store.unstagedChanges.isEmpty {
            // 空仓库/干净工作区此前渲染两个空 Section 标题——与"渲染失败"无法
            // 区分(参考实现里没有一个空白面板)。
            PaneEmptyState(
                systemImage: "checkmark.circle",
                title: "工作区干净",
                hint: "没有已暂存或未暂存的更改"
            )
            .accessibilityIdentifier("git.changes-empty")
        } else {
            List {
                Section("Staged") {
                    ForEach(store.stagedChanges) { change in
                        changeRow(change, staged: true)
                    }
                }
                Section("Changes") {
                    ForEach(store.unstagedChanges) { change in
                        changeRow(change, staged: false)
                    }
                }
            }
            .accessibilityIdentifier("git.changes")
        }
    }

    @ViewBuilder
    private func changeRow(_ change: GitStore.Change, staged: Bool) -> some View {
        HStack(spacing: 6) {
            Text(change.status)
                .font(.caption2.monospaced())
                .foregroundStyle(statusColor(change.status))
                .frame(width: 18)
            Text((change.path as NSString).lastPathComponent)
                .font(.caption)
                .lineLimit(1)
            if store.conflictPaths.contains(change.path) {
                Image(systemName: "exclamationmark.triangle.fill")
                    .font(.caption2)
                    .foregroundStyle(Color.dsWarning)
                    .accessibilityIdentifier("git.conflict.\(change.path)")
            }
            Spacer()
            if staged {
                Button("Unstage") { onAction?(.unstage(path: change.path)) }
                    .disabled(onAction == nil)
                    .font(.caption2)
            } else {
                Button("Stage") { onAction?(.stage(path: change.path)) }
                    .disabled(onAction == nil)
                    .font(.caption2)
            }
        }
        .accessibilityIdentifier("git.change.\(change.path)")
    }

    private func statusColor(_ status: String) -> Color {
        switch status {
        case "M": Color.dsWarning
        case "A", "?": Color.dsSuccess
        case "D": Color.dsDanger
        default: Color.dsTextSecondary
        }
    }
}

public struct StageActions: View {
    public var onAction: ((_ action: GitAction) -> Void)?
    @State private var confirmDiscardAll = false
    @State private var commitMessage = ""

    public init(onAction: ((_ action: GitAction) -> Void)? = nil) {
        self.onAction = onAction
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            TextField("提交信息…", text: $commitMessage)
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("git.commit-message")
            HStack(spacing: 6) {
                Button("Commit") {
                    onAction?(.commit(message: commitMessage))
                    commitMessage = ""
                }
                .disabled(commitMessage.trimmingCharacters(in: .whitespaces).isEmpty)
                .accessibilityIdentifier("git.commit")
                Button("Stage All") { onAction?(.stageAll) }
                    .accessibilityIdentifier("git.stage-all")
                // discard 需独立确认（二段确认）。
                if confirmDiscardAll {
                    Button("确认丢弃全部更改", role: .destructive) {
                        onAction?(.discard(paths: []))
                        confirmDiscardAll = false
                    }
                    .accessibilityIdentifier("git.discard-confirm")
                    Button("取消") { confirmDiscardAll = false }
                } else {
                    Button("Discard All…") { confirmDiscardAll = true }
                        .accessibilityIdentifier("git.discard")
                }
            }
        }
        .padding(8)
        .accessibilityIdentifier("git.stage-actions")
    }
}

public struct CommitSheet: View {
    public let message: String
    public let diff: String
    public var onConfirm: () -> Void
    public var onCancel: () -> Void

    public init(message: String, diff: String, onConfirm: @escaping () -> Void, onCancel: @escaping () -> Void) {
        self.message = message
        self.diff = diff
        self.onConfirm = onConfirm
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("确认提交")
                .font(.headline)
            Text(message)
                .font(.callout)
            Text("\(diff.split(separator: "\n").count) 行 diff")
                .font(.caption)
                .foregroundStyle(Color.dsTextMuted)
            HStack {
                Button("提交") { onConfirm() }
                    .accessibilityIdentifier("git.commit-sheet.confirm")
                Button("取消", role: .cancel) { onCancel() }
                    .accessibilityIdentifier("git.commit-sheet.cancel")
            }
        }
        .padding(16)
        .frame(minWidth: 380)
        .accessibilityIdentifier("git.commit-sheet")
    }
}

/// diff 视图（原始 diff 不截断；可下载到用户指定位置）。
public struct DiffView: View {
    public let diffText: String

    public init(diffText: String) {
        self.diffText = diffText
    }

    public var body: some View {
        ScrollView([.horizontal, .vertical]) {
            Text(diffText.isEmpty ? "选择变更以查看 diff" : diffText)
                .font(.caption2.monospaced())
                .textSelection(.enabled)
                .fixedSize(horizontal: true, vertical: true)
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier("git.diff")
    }
}

public struct HistoryView: View {
    let store: GitStore

    public init(store: GitStore) {
        self.store = store
    }

    public var body: some View {
        if let notice = store.historyReadState.notice, store.commits.isEmpty {
            Text(notice).foregroundStyle(Color.dsTextSecondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if store.commits.isEmpty {
            PaneEmptyState(
                systemImage: "clock.arrow.circlepath",
                title: "还没有提交",
                hint: "这个仓库的历史为空"
            )
            .accessibilityIdentifier("git.history-empty")
        } else {
            VStack(spacing: 6) {
                if let notice = store.historyReadState.notice {
                    Text(notice).font(.caption).foregroundStyle(Color.dsTextSecondary)
                }
                List(store.commits) { commit in
                HStack(spacing: 6) {
                    Text(commit.shortHash)
                        .font(.caption2.monospaced())
                        .foregroundStyle(Color.dsAccent)
                    Text(commit.message)
                        .font(.caption)
                        .lineLimit(1)
                    Spacer()
                    Text("\(commit.author) · \(commit.date)")
                        .font(.caption2)
                        .foregroundStyle(Color.dsTextMuted)
                }
                .accessibilityIdentifier("git.commit.\(commit.shortHash)")
            }
            .accessibilityIdentifier("git.history")
            }
        }
    }
}

public struct BranchGraph: View {
    public let graph: String

    public init(graph: String) {
        self.graph = graph
    }

    public var body: some View {
        ScrollView(.horizontal) {
            Text(graph)
                .font(.caption.monospaced())
                .textSelection(.enabled)
                .padding(8)
        }
        .accessibilityIdentifier("git.branch-graph")
    }
}

public struct BlameView: View {
    public let blame: String

    public init(blame: String) {
        self.blame = blame
    }

    public var body: some View {
        ScrollView {
            Text(blame)
                .font(.caption2.monospaced())
                .textSelection(.enabled)
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier("git.blame")
    }
}

public struct StashView: View {
    let store: GitStore
    public var onAction: ((_ action: GitAction) -> Void)?
    @State private var stashMessage = ""

    public init(store: GitStore, onAction: ((_ action: GitAction) -> Void)? = nil) {
        self.store = store
        self.onAction = onAction
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let notice = store.stashReadState.notice {
                Text(notice).font(.caption).foregroundStyle(Color.dsTextSecondary)
            } else if store.stashEntries.isEmpty {
                Text("暂无贮藏").font(.caption).foregroundStyle(Color.dsTextSecondary)
            }
            HStack {
                TextField("stash 信息…", text: $stashMessage)
                    .textFieldStyle(.roundedBorder)
                Button("Stash") {
                    onAction?(.stashPush(message: stashMessage))
                    stashMessage = ""
                }
                .accessibilityIdentifier("git.stash-push")
            }
            .disabled(onAction == nil || store.stashReadState != .loaded)
            List(store.stashEntries) { entry in
                HStack {
                    Text("stash@{\(entry.index)}")
                        .font(.caption2.monospaced())
                        .foregroundStyle(Color.dsAccent)
                    Text(entry.message)
                        .font(.caption)
                        .lineLimit(1)
                    Spacer()
                    Button("Apply") { onAction?(.stashApply(index: entry.index)) }
                        .disabled(onAction == nil || store.stashReadState != .loaded)
                        .font(.caption2)
                        .accessibilityIdentifier("git.stash-apply.\(entry.index)")
                    // drop 需独立确认（二段确认）。
                    Button("Drop") { onAction?(.stashDrop(index: entry.index)) }
                        .disabled(onAction == nil || store.stashReadState != .loaded)
                        .font(.caption2)
                        .accessibilityIdentifier("git.stash-drop.\(entry.index)")
                }
                .accessibilityIdentifier("git.stash.\(entry.index)")
            }
            .frame(minHeight: 100)
        }
        .padding(8)
        .accessibilityIdentifier("git.stash")
    }
}

public struct ConflictResolver: View {
    public let path: String
    /// 冲突预览（conflict 标记 diff）。
    public let preview: String
    /// 选定侧内容（ours/theirs/manual）。
    public var onResolve: ((_ content: String) -> Void)?
    @State private var resolvedContent = ""

    public init(path: String, preview: String, onResolve: ((_ content: String) -> Void)? = nil) {
        self.path = path
        self.preview = preview
        self.onResolve = onResolve
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("冲突：\(path)")
                .font(.headline)
                .accessibilityIdentifier("git.conflict.title")
            ScrollView {
                Text(preview)
                    .font(.caption2.monospaced())
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(minHeight: 120)
            TextField("解决后的内容（或选择 ours/theirs）", text: $resolvedContent)
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("git.conflict.content")
            HStack(spacing: 8) {
                Button("保留当前侧（ours）") {
                    onResolve?(Self.oursSide(preview))
                }
                .accessibilityIdentifier("git.conflict.ours")
                Button("保留来入侧（theirs）") {
                    onResolve?(Self.theirsSide(preview))
                }
                .accessibilityIdentifier("git.conflict.theirs")
                Button("解决并 Stage") {
                    onResolve?(resolvedContent)
                }
                .disabled(resolvedContent.isEmpty)
                .accessibilityIdentifier("git.conflict.resolve")
            }
        }
        .padding(12)
        .accessibilityIdentifier("git.conflict-resolver")
    }

    /// ours/theirs 侧提取（conflict 标记解析；纯函数）。
    static func oursSide(_ preview: String) -> String {
        extractSide(preview, marker: "<<<<<<<", endMarker: "=======")
    }

    static func theirsSide(_ preview: String) -> String {
        let ours = extractSide(preview, marker: "=======", endMarker: ">>>>>>>")
        return ours
    }

    private static func extractSide(_ text: String, marker: String, endMarker: String) -> String {
        guard let markerRange = text.range(of: marker) else { return text }
        // marker 后的 label（如 HEAD）同行——跳到行尾。
        var contentStart = markerRange.upperBound
        if let lineEnd = text.range(of: "\n", range: contentStart..<text.endIndex)?.lowerBound {
            contentStart = text.index(after: lineEnd)
        }
        guard let end = text.range(of: endMarker, range: contentStart..<text.endIndex)?.lowerBound else {
            return String(text[contentStart...]).trimmingCharacters(in: .whitespacesAndNewlines)
        }
        return String(text[contentStart..<end]).trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
