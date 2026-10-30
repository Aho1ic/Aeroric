import SwiftUI

/// The Git panel's words and status marks (2026-10-30), kept out of the view
/// so they can be tested.
public enum GitPageText {
    public static func commitTitle(stagedCount: Int) -> String {
        stagedCount > 0 ? "提交 \(stagedCount) 个文件" : "提交"
    }

    public static func tabTitle(_ base: String, count: Int) -> String {
        count > 0 ? "\(base) \(count)" : base
    }

    public enum Tone: Equatable { case modified, added, deleted, renamed, untracked, conflicted, other }

    /// The one-letter mark before a file and its colour family.
    public static func mark(_ status: String) -> (letter: String, tone: Tone) {
        switch status {
        case "M": ("M", .modified)
        case "A": ("A", .added)
        case "D": ("D", .deleted)
        case "R", "C": ("R", .renamed)
        case "?": ("U", .untracked)
        case "U": ("!", .conflicted)
        default: (String(status.prefix(1)), .other)
        }
    }

    public static func discardMessage(path: String, untracked: Bool) -> String {
        untracked
            ? "「\(path)」还没有被 Git 跟踪，放弃会把这个文件删掉，无法撤销。"
            : "「\(path)」会回到上次提交时的样子，未暂存的改动会丢失，无法撤销。"
    }

    public static func pushQuestion(count: Int, upstream: String) -> String {
        "推送 \(count) 个提交到 \(upstream)？"
    }

    public static func pullQuestion(count: Int, upstream: String) -> String {
        "从 \(upstream) 拉取 \(count) 个提交？"
    }

    public static func publishQuestion(branch: String) -> String {
        "发布分支 \(branch)？"
    }

    public static func publishDetail(branch: String) -> String {
        "会在 origin 上新建同名分支 \(branch)，并把它设为这个分支的上游。之后就能直接推送和拉取。"
    }

    /// The commits a push sends: the newest `ahead` of the history.
    public static func outgoing(_ commits: [GitStore.Commit], ahead: Int) -> [GitStore.Commit] {
        Array(commits.prefix(max(0, ahead)))
    }

    /// The path inside a `GitStore.Change` id (`<path>:<staged>`).
    public static func path(ofChangeID id: String) -> String {
        guard let colon = id.lastIndex(of: ":") else { return id }
        return String(id[..<colon])
    }

    /// `Sources/App/` and `Main.swift` for `Sources/App/Main.swift`.
    public static func split(_ path: String) -> (directory: String, name: String) {
        guard let slash = path.lastIndex(of: "/") else { return ("", path) }
        return (String(path[...slash]), String(path[path.index(after: slash)...]))
    }
}

/// Git in the session's side panel (2026-10-30, design approved; replaces
/// the Git page): branch capsule and sync capsules on top, then the commit
/// box, staged and unstaged files, and folded 贮藏 / 历史. A file opens its
/// diff in the centre column.
public struct GitSidebarPanel: View {
    let store: GitStore
    let onAction: (GitAction) -> Void
    let onOpenDiff: (GitStore.Change) -> Void
    /// Asks the host for a commit message from the staged diff.
    let onGenerate: () async -> String?

    @State private var selected: String?
    @State private var message = ""
    @State private var generating = false
    @State private var newBranch = ""
    @State private var creatingBranch = false
    @State private var stashNote = ""
    @State private var discarding: GitStore.Change?
    @State private var dropping: GitStore.StashEntry?
    @State private var confirmPush = false
    @State private var confirmPull = false
    @State private var confirmPublish = false
    @State private var showsStash = false
    @State private var showsHistory = false

    public init(store: GitStore, onAction: @escaping (GitAction) -> Void,
                onOpenDiff: @escaping (GitStore.Change) -> Void, onGenerate: @escaping () async -> String?) {
        self.store = store
        self.onAction = onAction
        self.onOpenDiff = onOpenDiff
        self.onGenerate = onGenerate
    }

    private var busy: Bool { store.working != nil }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            if let error = store.actionError { errorLine(error) }
            HairlineDivider()
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.s) {
                    commitBox
                    changes
                    fold("贮藏", count: store.stashEntries.count, isOpen: $showsStash) { stash }
                    fold("历史", count: nil, isOpen: $showsHistory) { history }
                }
                .padding(AeroricSpace.m)
            }
            .scrollIndicators(.hidden)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .alert("放弃这个文件的改动？", isPresented: Binding(get: { discarding != nil }, set: { if !$0 { discarding = nil } }),
               presenting: discarding) { change in
            Button("放弃改动", role: .destructive) {
                onAction(.discardFile(path: change.path, untracked: change.status == "?"))
            }
            Button("取消", role: .cancel) {}
        } message: { change in
            Text(GitPageText.discardMessage(path: change.path, untracked: change.status == "?"))
        }
        .alert("删除这条贮藏？", isPresented: Binding(get: { dropping != nil }, set: { if !$0 { dropping = nil } }),
               presenting: dropping) { entry in
            Button("删除", role: .destructive) { onAction(.stashDrop(index: entry.index)) }
            Button("取消", role: .cancel) {}
        } message: { entry in
            Text("「\(entry.message)」里的改动会被删掉，无法撤销。")
        }
        .accessibilityIdentifier("git.panel")
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: AeroricSpace.s) {
            branchMenu
            Spacer(minLength: AeroricSpace.xs)
            if let working = store.working {
                ProgressView().controlSize(.small).help(working)
            }
            remoteButtons
        }
        .padding(.horizontal, AeroricSpace.m)
        .frame(height: 40)
    }

    /// 获取 · ↓ N · ↑ N, or 发布 before the branch has a remote.
    @ViewBuilder private var remoteButtons: some View {
        let ahead = store.ahead ?? 0
        let behind = store.behind ?? 0
        HStack(spacing: AeroricSpace.xs) {
            NativeIconButton(systemImage: "arrow.clockwise", label: "获取：从远端取回最新状态，不改动你的文件",
                             identifier: "git.fetch") { onAction(.fetch(prune: false)) }
                .disabled(busy || store.upstream == nil)
            if let upstream = store.upstream {
                countButton("\(behind)", systemImage: "arrow.down",
                            help: behind > 0 ? "从 \(upstream) 拉取 \(behind) 个提交" : "已是最新") { confirmPull = true }
                    .disabled(busy || behind == 0)
                countButton("\(ahead)", systemImage: "arrow.up",
                            help: ahead > 0 ? "推送 \(ahead) 个提交到 \(upstream)" : "没有要推送的提交") { confirmPush = true }
                    .disabled(busy || ahead == 0)
            } else if store.currentBranch != nil {
                countButton("发布", systemImage: "arrow.up", help: "在 origin 上新建同名分支") { confirmPublish = true }
                    .disabled(busy || store.commits.isEmpty)
            }
        }
        .alert(GitPageText.pushQuestion(count: ahead, upstream: store.upstream ?? ""), isPresented: $confirmPush) {
            Button("推送") { onAction(.push(branch: store.currentBranch ?? "")) }
            Button("取消", role: .cancel) {}
        } message: {
            Text((["别人拉取后就能看到这些提交："]
                  + GitPageText.outgoing(store.commits, ahead: ahead).map { "\($0.shortHash)  \($0.message)" })
                .joined(separator: "\n"))
        }
        .alert(GitPageText.pullQuestion(count: behind, upstream: store.upstream ?? ""), isPresented: $confirmPull) {
            Button("拉取") { onAction(.pull) }
            Button("取消", role: .cancel) {}
        } message: {
            Text("会合并到当前分支 \(store.currentBranch ?? "")。如果和本地改动冲突，Git 会停下并说明原因，不会覆盖你的改动。")
        }
        .alert(GitPageText.publishQuestion(branch: store.currentBranch ?? ""), isPresented: $confirmPublish) {
            Button("发布") { onAction(.publish(branch: store.currentBranch ?? "")) }
            Button("取消", role: .cancel) {}
        } message: {
            Text(GitPageText.publishDetail(branch: store.currentBranch ?? ""))
        }
    }

    private func countButton(_ title: String, systemImage: String, help: String,
                             action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, systemImage: systemImage).monospacedDigit()
        }
        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
        .fixedSize()
        .help(help)
        .accessibilityLabel(help)
    }

    private var branchMenu: some View {
        Menu {
            let others = store.branches.filter { !$0.current }
            if !others.isEmpty {
                Section("切换到") {
                    ForEach(others) { branch in
                        Button(branch.name) { onAction(.checkout(branch: branch.name)) }
                    }
                }
            }
            Button("新建分支…", systemImage: "plus") { creatingBranch = true }
            if store.upstream == nil, store.currentBranch != nil {
                Button("发布分支…", systemImage: "arrow.up") { confirmPublish = true }
                    .disabled(store.commits.isEmpty)
            }
        } label: {
            HStack(spacing: AeroricSpace.xs) {
                Image(systemName: "arrow.triangle.branch")
                Text(store.currentBranch ?? "无分支").fontWeight(.medium)
                    .lineLimit(1).truncationMode(.middle)
                Image(systemName: "chevron.down").font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .font(.system(size: AeroricType.secondary))
            .padding(.horizontal, AeroricSpace.m)
            .frame(height: 26)
            .glassSurface(.floatingCard, in: Capsule(style: .continuous))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .frame(maxWidth: 150, alignment: .leading)
        .fixedSize(horizontal: false, vertical: true)
        .disabled(busy)
        .help(store.currentBranch ?? "无分支")
        .popover(isPresented: $creatingBranch, arrowEdge: .bottom) { newBranchForm }
        .accessibilityIdentifier("git.branch")
    }

    private var newBranchForm: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Text("从 \(store.currentBranch ?? "当前提交") 新建并切换")
                .font(.system(size: AeroricType.secondary, weight: .semibold))
                .lineLimit(1)
            CapsuleField(systemImage: "arrow.triangle.branch", placeholder: "分支名", text: $newBranch)
                .frame(width: 240)
                .onSubmit(createBranch)
            HStack {
                Spacer()
                Button("取消") { creatingBranch = false }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                Button("创建", action: createBranch)
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                    .disabled(newBranch.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(AeroricSpace.l)
    }

    private func createBranch() {
        let name = newBranch.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        onAction(.createBranch(name: name))
        newBranch = ""
        creatingBranch = false
    }

    private func errorLine(_ error: String) -> some View {
        HStack(spacing: AeroricSpace.s) {
            Image(systemName: "exclamationmark.triangle")
            Text(error).lineLimit(1).truncationMode(.tail)
            Spacer(minLength: AeroricSpace.xs)
            Button { store.actionError = nil } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)) }
                .buttonStyle(.plain)
                .accessibilityLabel("关闭")
        }
        .font(.system(size: AeroricType.caption))
        .foregroundStyle(AeroricColor.danger)
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 26)
        .background(AeroricColor.danger.opacity(0.12), in: Capsule(style: .continuous))
        .padding(.horizontal, AeroricSpace.s)
        .padding(.bottom, AeroricSpace.xs)
        .help(error)
        .accessibilityIdentifier("git.error")
    }

    // MARK: - Changes

    private var commitBox: some View {
        let staged = store.stagedChanges.count
        let canCommit = staged > 0 && !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !busy
        return VStack(alignment: .leading, spacing: AeroricSpace.s) {
            TextField("提交说明", text: $message, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: AeroricType.secondary))
                .lineLimit(1...5)
                .accessibilityIdentifier("git.commit-message")
            HStack(spacing: AeroricSpace.s) {
                Spacer(minLength: 0)
                Button {
                    generating = true
                    Task { @MainActor in
                        if let text = await onGenerate() { message = text }
                        generating = false
                    }
                } label: {
                    HStack(spacing: AeroricSpace.xs) {
                        if generating { ProgressView().controlSize(.mini) } else { Image(systemName: "sparkles") }
                        Text("生成")
                    }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .fixedSize()
                .disabled(staged == 0 || generating || busy)
                .help("让 Codex 根据已暂存的改动写提交说明")
                Button(GitPageText.commitTitle(stagedCount: staged)) {
                    onAction(.commit(message: message.trimmingCharacters(in: .whitespacesAndNewlines)))
                    message = ""
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                .disabled(!canCommit)
                .fixedSize()
                .accessibilityIdentifier("git.commit")
            }
        }
        .padding(AeroricSpace.m)
        .background(AeroricColor.layer2, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).stroke(AeroricColor.separator2))
    }

    @ViewBuilder private var changes: some View {
        if let notice = store.changesReadState.notice, store.changes.isEmpty {
            Text(notice).font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary).lineLimit(2)
                .padding(.horizontal, AeroricSpace.s)
        } else if store.changes.isEmpty {
            Label("工作区干净，没有要提交的改动", systemImage: "checkmark.circle")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .lineLimit(1)
                .padding(.horizontal, AeroricSpace.s)
                .padding(.vertical, AeroricSpace.xs)
        } else {
            VStack(alignment: .leading, spacing: AeroricSpace.xxs) {
                group("已暂存", store.stagedChanges, all: "全部取消", allAction: .unstageAll)
                group("未暂存", store.unstagedChanges, all: "全部暂存", allAction: .stageAll)
            }
        }
    }

    @ViewBuilder
    private func group(_ title: String, _ items: [GitStore.Change], all: String, allAction: GitAction) -> some View {
        if !items.isEmpty {
            HStack {
                Text("\(title) \(items.count)").foregroundStyle(AeroricColor.textSecondary)
                Spacer()
                Button(all) { onAction(allAction) }
                    .buttonStyle(.plain)
                    .foregroundStyle(AeroricColor.textTertiary)
                    .disabled(busy)
            }
            .font(.system(size: AeroricType.caption))
            .lineLimit(1)
            .padding(.horizontal, AeroricSpace.s)
            .padding(.top, AeroricSpace.s)
            ForEach(items) { change in
                GitChangeRow(change: change, selected: selected == change.id, busy: busy,
                             onSelect: {
                                 selected = change.id
                                 onOpenDiff(change)
                             },
                             onToggle: { onAction(change.staged ? .unstage(path: change.path) : .stage(path: change.path)) },
                             onDiscard: change.staged ? nil : { discarding = change })
            }
        }
    }

    // MARK: - Stash and history

    private func fold<Content: View>(_ title: String, count: Int?, isOpen: Binding<Bool>,
                                     @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            HairlineDivider().padding(.vertical, AeroricSpace.xs)
            Button { withAnimation(.snappy) { isOpen.wrappedValue.toggle() } } label: {
                HStack(spacing: AeroricSpace.xs) {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .rotationEffect(.degrees(isOpen.wrappedValue ? 90 : 0))
                        .frame(width: 10)
                    Text(count.map { GitPageText.tabTitle(title, count: $0) } ?? title)
                    Spacer()
                }
                .font(.system(size: AeroricType.secondary, weight: .medium))
                .foregroundStyle(AeroricColor.textSecondary)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .padding(.horizontal, AeroricSpace.s)
            if isOpen.wrappedValue { content() }
        }
    }

    private var stash: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            HStack(spacing: AeroricSpace.s) {
                CapsuleField(placeholder: "贮藏说明（可选）", text: $stashNote, height: 28)
                Button("贮藏") {
                    onAction(.stashPush(message: stashNote.trimmingCharacters(in: .whitespaces)))
                    stashNote = ""
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                .disabled(store.changes.isEmpty || busy)
                .fixedSize()
                .help("把当前改动先收起来，稍后再恢复")
            }
            if store.stashEntries.isEmpty {
                Text("没有贮藏").font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary).padding(.horizontal, AeroricSpace.s)
            }
            ForEach(store.stashEntries) { entry in
                HStack(spacing: AeroricSpace.xs) {
                    Text(entry.message.isEmpty ? "贮藏 \(entry.index)" : entry.message)
                        .lineLimit(1).truncationMode(.tail)
                    Spacer(minLength: AeroricSpace.xs)
                    Button("恢复") { onAction(.stashApply(index: entry.index)) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                    Button("删除") { dropping = entry }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                }
                .font(.system(size: AeroricType.secondary))
                .padding(.horizontal, AeroricSpace.s)
                .frame(height: 30)
                .help(entry.message)
                .disabled(busy)
            }
        }
    }

    @ViewBuilder private var history: some View {
        if let notice = store.historyReadState.notice, store.commits.isEmpty {
            Text(notice).font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary).padding(.horizontal, AeroricSpace.s)
        } else if store.commits.isEmpty {
            Text("还没有提交").font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary).padding(.horizontal, AeroricSpace.s)
        }
        LazyVStack(alignment: .leading, spacing: AeroricSpace.xs) {
            ForEach(store.commits) { commit in
                VStack(alignment: .leading, spacing: 2) {
                    Text(commit.message.split(separator: "\n").first.map(String.init) ?? commit.message)
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.text)
                        .lineLimit(1)
                    HStack(spacing: AeroricSpace.s) {
                        Text(commit.shortHash).font(.system(size: AeroricType.caption).monospaced())
                            .foregroundStyle(AeroricColor.accent)
                        Text("\(commit.author) · \(commit.date)")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .lineLimit(1)
                    }
                }
                .padding(.horizontal, AeroricSpace.s)
                .help(commit.message)
            }
        }
    }
}

/// One changed file: its mark, name and folder. Stage / unstage and discard
/// (unstaged only) show on hover and stay on the selected row; the context
/// menu offers the same, since hover alone was hard to reach (2026-10-30
/// acceptance report).
struct GitChangeRow: View {
    let change: GitStore.Change
    let selected: Bool
    let busy: Bool
    let onSelect: () -> Void
    let onToggle: () -> Void
    let onDiscard: (() -> Void)?
    @State private var hovered = false

    var body: some View {
        let mark = GitPageText.mark(change.status)
        let parts = GitPageText.split(change.path)
        HStack(spacing: AeroricSpace.s) {
            Text(mark.letter)
                .font(.system(size: AeroricType.caption, weight: .bold).monospaced())
                .foregroundStyle(Self.color(mark.tone))
                .frame(width: 14)
            Text(parts.name).lineLimit(1)
            Text(parts.directory).foregroundStyle(AeroricColor.textTertiary)
                .font(.system(size: AeroricType.caption)).lineLimit(1).truncationMode(.head)
            Spacer(minLength: 0)
            if (hovered || selected) && !busy {
                if let onDiscard {
                    icon("arrow.uturn.backward", help: "放弃改动", action: onDiscard)
                }
                icon(change.staged ? "minus" : "plus", help: change.staged ? "取消暂存" : "暂存", action: onToggle)
            }
        }
        .font(.system(size: AeroricType.secondary))
        .padding(.horizontal, AeroricSpace.s)
        .frame(height: 26)
        .background {
            if selected || hovered {
                Capsule(style: .continuous).fill(selected ? AeroricColor.selection : AeroricColor.hover)
            }
        }
        .contentShape(Capsule(style: .continuous))
        .onTapGesture(perform: onSelect)
        .onHover { hovered = $0 }
        .contextMenu {
            Button("查看改动", systemImage: "plusminus", action: onSelect)
            Button(change.staged ? "取消暂存" : "暂存", systemImage: change.staged ? "minus" : "plus", action: onToggle)
                .disabled(busy)
            if let onDiscard {
                Divider()
                Button("放弃改动…", systemImage: "arrow.uturn.backward", role: .destructive, action: onDiscard)
                    .disabled(busy)
            }
        }
        .help(change.path)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    private func icon(_ name: String, help: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: name).font(.system(size: 11, weight: .semibold))
                .frame(width: 20, height: 20).contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(AeroricColor.textSecondary)
        .help(help)
        .accessibilityLabel(help)
    }

    static func color(_ tone: GitPageText.Tone) -> Color {
        switch tone {
        case .modified: AeroricColor.warning
        case .added, .untracked: AeroricColor.success
        case .deleted, .conflicted: AeroricColor.danger
        case .renamed: AeroricColor.accent
        case .other: AeroricColor.textTertiary
        }
    }
}
