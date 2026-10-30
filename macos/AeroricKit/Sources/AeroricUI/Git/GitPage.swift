import SwiftUI

/// The Git page's words and status marks (2026-10-30), kept out of the view
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

/// Git page (2026-10-30, design approved): branch capsule, 更改 / 历史 /
/// 贮藏, staged and unstaged files with hover actions, a commit box and the
/// selected file's diff. Push and pull come in the next batch.
public struct GitPage: View {
    let store: GitStore
    let onAction: (GitAction) -> Void
    let onSelect: (GitStore.Change) -> Void
    /// Asks the host for a commit message from the staged diff.
    let onGenerate: () async -> String?

    @State private var tab = "changes"
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

    public init(store: GitStore, onAction: @escaping (GitAction) -> Void,
                onSelect: @escaping (GitStore.Change) -> Void, onGenerate: @escaping () async -> String?) {
        self.store = store
        self.onAction = onAction
        self.onSelect = onSelect
        self.onGenerate = onGenerate
    }

    private var busy: Bool { store.working != nil }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            header
            if let error = store.actionError {
                errorBanner(error)
            }
            Group {
                switch tab {
                case "history": history
                case "stash": stash
                default: changes
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .padding(AeroricSpace.l)
        .background(AeroricColor.canvas)
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
        // After a write the list reloads: keep the picked file (it may have
        // moved between 已暂存 and 未暂存) and reload its diff.
        .onChange(of: store.changes) { _, changes in
            guard let id = selected else { return }
            let path = changes.first(where: { $0.id == id })?.path ?? GitPageText.path(ofChangeID: id)
            if let kept = changes.first(where: { $0.id == id }) ?? changes.first(where: { $0.path == path }) {
                selected = kept.id
                onSelect(kept)
            } else {
                selected = nil
            }
        }
        .accessibilityIdentifier("git.page")
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: AeroricSpace.m) {
            branchMenu
            CapsuleSlider(selection: $tab, groupTitle: "Git 内容", segments: [
                .init(value: "changes", title: GitPageText.tabTitle("更改", count: store.changes.count), systemImage: "plusminus"),
                .init(value: "history", title: "历史", systemImage: "clock.arrow.circlepath"),
                .init(value: "stash", title: GitPageText.tabTitle("贮藏", count: store.stashEntries.count),
                      systemImage: "tray.and.arrow.down"),
            ])
            .fixedSize()
            Spacer(minLength: AeroricSpace.m)
            if let working = store.working {
                HStack(spacing: AeroricSpace.s) {
                    ProgressView().controlSize(.small)
                    Text(working).foregroundStyle(AeroricColor.textSecondary)
                }
                .font(.system(size: AeroricType.caption))
                .lineLimit(1)
                .fixedSize()
            }
            remoteButtons
        }
    }

    /// 获取 · 拉取 N · 推送 N, or 发布分支 before the branch has a remote.
    @ViewBuilder private var remoteButtons: some View {
        let ahead = store.ahead ?? 0
        let behind = store.behind ?? 0
        HStack(spacing: AeroricSpace.s) {
            remoteButton("获取", systemImage: "arrow.clockwise", help: "从远端取回最新状态，不改动你的文件") {
                onAction(.fetch(prune: false))
            }
            .disabled(busy || store.upstream == nil)
            if let upstream = store.upstream {
                remoteButton(behind > 0 ? "拉取 \(behind)" : "拉取", systemImage: "arrow.down",
                             help: behind > 0 ? "从 \(upstream) 拉取 \(behind) 个提交" : "已是最新") { confirmPull = true }
                    .disabled(busy || behind == 0)
                remoteButton(ahead > 0 ? "推送 \(ahead)" : "推送", systemImage: "arrow.up",
                             help: ahead > 0 ? "推送 \(ahead) 个提交到 \(upstream)" : "没有要推送的提交") { confirmPush = true }
                    .disabled(busy || ahead == 0)
            } else if store.currentBranch != nil {
                remoteButton("发布分支", systemImage: "arrow.up", help: "在 origin 上新建同名分支") { confirmPublish = true }
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

    private func remoteButton(_ title: String, systemImage: String, help: String,
                              action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Label(title, systemImage: systemImage).monospacedDigit()
        }
        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
        .fixedSize()
        .help(help)
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
        } label: {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: "arrow.triangle.branch")
                Text(store.currentBranch ?? "无分支").fontWeight(.medium)
                Image(systemName: "chevron.down").font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .font(.system(size: AeroricType.secondary))
            .lineLimit(1)
            .padding(.horizontal, AeroricSpace.l)
            .frame(height: 28)
            .glassSurface(.floatingCard, in: Capsule(style: .continuous))
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .disabled(busy)
        .popover(isPresented: $creatingBranch, arrowEdge: .bottom) { newBranchForm }
        .accessibilityIdentifier("git.branch")
    }

    private var newBranchForm: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Text("从 \(store.currentBranch ?? "当前提交") 新建并切换")
                .font(.system(size: AeroricType.secondary, weight: .semibold))
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

    private func errorBanner(_ error: String) -> some View {
        HStack(spacing: AeroricSpace.s) {
            Image(systemName: "exclamationmark.triangle")
            Text(error).lineLimit(1).truncationMode(.tail)
            Spacer(minLength: AeroricSpace.s)
            Button { store.actionError = nil } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)) }
                .buttonStyle(.plain)
                .accessibilityLabel("关闭")
        }
        .font(.system(size: AeroricType.caption))
        .foregroundStyle(AeroricColor.danger)
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 28)
        .background(AeroricColor.danger.opacity(0.12), in: Capsule(style: .continuous))
        .help(error)
        .accessibilityIdentifier("git.error")
    }

    // MARK: - Changes

    private var changes: some View {
        HStack(spacing: AeroricSpace.m) {
            VStack(spacing: AeroricSpace.m) {
                ScrollView {
                    VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                        if let notice = store.changesReadState.notice, store.changes.isEmpty {
                            Text(notice).font(.system(size: AeroricType.secondary))
                                .foregroundStyle(AeroricColor.textSecondary).padding(AeroricSpace.m)
                        } else if store.changes.isEmpty {
                            PaneEmptyState(systemImage: "checkmark.circle", title: "工作区干净", hint: "没有要提交的改动")
                                .padding(.top, AeroricSpace.xxl)
                        } else {
                            group("已暂存", store.stagedChanges, all: "全部取消", allAction: .unstageAll)
                            group("未暂存", store.unstagedChanges, all: "全部暂存", allAction: .stageAll)
                        }
                    }
                    .padding(AeroricSpace.s)
                }
                .scrollIndicators(.hidden)
                commitBox
            }
            .frame(width: 320)
            .padding(AeroricSpace.s)
            .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.l))
            diffPane
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
            .padding(.horizontal, AeroricSpace.m)
            .padding(.top, AeroricSpace.s)
            ForEach(items) { change in
                GitChangeRow(change: change, selected: selected == change.id, busy: busy,
                             onSelect: {
                                 selected = change.id
                                 onSelect(change)
                             },
                             onToggle: { onAction(change.staged ? .unstage(path: change.path) : .stage(path: change.path)) },
                             onDiscard: change.staged ? nil : { discarding = change })
            }
        }
    }

    private var commitBox: some View {
        let staged = store.stagedChanges.count
        let canCommit = staged > 0 && !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !busy
        return VStack(alignment: .leading, spacing: AeroricSpace.s) {
            TextField("提交信息", text: $message, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: AeroricType.secondary))
                .lineLimit(1...5)
                .accessibilityIdentifier("git.commit-message")
            HStack(spacing: AeroricSpace.s) {
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
                .disabled(staged == 0 || generating || busy)
                .help("让 Codex 根据已暂存的改动写提交信息")
                Spacer()
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
        .background(AeroricColor.layer2, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).stroke(AeroricColor.separator2))
    }

    private var diffPane: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            if let change = store.changes.first(where: { $0.id == selected }) {
                Text(change.path)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(1).truncationMode(.middle)
                GitDiffText(text: store.diffText)
            } else {
                PaneEmptyState(systemImage: "doc.text.magnifyingglass", title: "选一个文件", hint: "这里显示它的改动")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .padding(AeroricSpace.m)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.l))
    }

    // MARK: - History and stash

    private var history: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                if let notice = store.historyReadState.notice, store.commits.isEmpty {
                    Text(notice).foregroundStyle(AeroricColor.textSecondary).padding(AeroricSpace.l)
                } else if store.commits.isEmpty {
                    PaneEmptyState(systemImage: "clock.arrow.circlepath", title: "还没有提交", hint: "这个仓库的历史为空")
                        .padding(.top, AeroricSpace.xxl)
                }
                ForEach(store.commits) { commit in
                    HStack(spacing: AeroricSpace.m) {
                        Text(commit.shortHash).font(.system(size: AeroricType.caption).monospaced())
                            .foregroundStyle(AeroricColor.accent)
                        Text(commit.message).lineLimit(1)
                        Spacer(minLength: AeroricSpace.m)
                        Text("\(commit.author) · \(commit.date)").foregroundStyle(AeroricColor.textTertiary)
                            .font(.system(size: AeroricType.caption)).lineLimit(1).fixedSize()
                    }
                    .font(.system(size: AeroricType.secondary))
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(height: 32)
                }
            }
            .padding(AeroricSpace.s)
        }
        .scrollIndicators(.hidden)
        .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.l))
    }

    private var stash: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack(spacing: AeroricSpace.s) {
                CapsuleField(systemImage: "text.cursor", placeholder: "贮藏说明（可选）", text: $stashNote)
                    .frame(maxWidth: 320)
                Button("贮藏当前更改") {
                    onAction(.stashPush(message: stashNote.trimmingCharacters(in: .whitespaces)))
                    stashNote = ""
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.tinted))
                .disabled(store.changes.isEmpty || busy)
                .fixedSize()
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if store.stashEntries.isEmpty {
                        PaneEmptyState(systemImage: "tray", title: "没有贮藏", hint: "把手头的改动先收起来，稍后再恢复")
                            .padding(.top, AeroricSpace.xxl)
                    }
                    ForEach(store.stashEntries) { entry in
                        HStack(spacing: AeroricSpace.m) {
                            Text(entry.message.isEmpty ? "贮藏 \(entry.index)" : entry.message).lineLimit(1)
                            Spacer(minLength: AeroricSpace.m)
                            Button("恢复") { onAction(.stashApply(index: entry.index)) }
                                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            Button("删除") { dropping = entry }
                                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        }
                        .font(.system(size: AeroricType.secondary))
                        .padding(.horizontal, AeroricSpace.l)
                        .frame(height: 36)
                        .disabled(busy)
                    }
                }
                .padding(AeroricSpace.s)
            }
            .scrollIndicators(.hidden)
            .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.l))
        }
    }
}

/// One changed file: its mark, name and folder; on hover, discard (unstaged
/// only) and stage / unstage.
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
            if hovered && !busy {
                if let onDiscard {
                    icon("arrow.uturn.backward", help: "放弃改动", action: onDiscard)
                }
                icon(change.staged ? "minus" : "plus", help: change.staged ? "取消暂存" : "暂存", action: onToggle)
            }
        }
        .font(.system(size: AeroricType.secondary))
        .padding(.horizontal, AeroricSpace.m)
        .frame(height: 28)
        .background {
            if selected || hovered {
                Capsule(style: .continuous).fill(selected ? AeroricColor.selection : AeroricColor.hover)
            }
        }
        .contentShape(Capsule(style: .continuous))
        .onTapGesture(perform: onSelect)
        .onHover { hovered = $0 }
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

/// A unified diff with added and removed lines tinted.
struct GitDiffText: View {
    let text: String

    var body: some View {
        let lines = text.isEmpty ? [] : text.components(separatedBy: "\n")
        ScrollView([.vertical, .horizontal]) {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                    Text(line.isEmpty ? " " : line)
                        .font(.system(size: AeroricType.caption).monospaced())
                        .foregroundStyle(Self.color(line))
                        .padding(.horizontal, AeroricSpace.s)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Self.tint(line))
                }
            }
            .textSelection(.enabled)
            .fixedSize(horizontal: true, vertical: false)
        }
        .scrollIndicators(.hidden)
        .accessibilityIdentifier("git.diff")
    }

    static func color(_ line: String) -> Color {
        if line.hasPrefix("+++") || line.hasPrefix("---") { return AeroricColor.textTertiary }
        if line.hasPrefix("+") { return AeroricColor.success }
        if line.hasPrefix("-") { return AeroricColor.danger }
        if line.hasPrefix("@@") { return AeroricColor.accent }
        return AeroricColor.textSecondary
    }

    static func tint(_ line: String) -> Color {
        if line.hasPrefix("+++") || line.hasPrefix("---") { return .clear }
        if line.hasPrefix("+") { return AeroricColor.success.opacity(0.10) }
        if line.hasPrefix("-") { return AeroricColor.danger.opacity(0.10) }
        return .clear
    }
}
