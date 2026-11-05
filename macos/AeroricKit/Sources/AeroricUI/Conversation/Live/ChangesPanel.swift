import AeroricProtocol
import SwiftUI

/// R2-2 (2026-11-04): what the changes page asks the host.
public protocol ChangesActions: Sendable {
    func changesDiff(taskId: String, scope: String, turnId: String?) async throws -> LiveReviewDiff
    /// `changes.accept` / `changes.reject` / `changes.comment`.
    func changeAction(_ method: String, taskId: String, scope: String, turnId: String?, hunkId: String,
                      text: String?) async throws
    func sendComments(taskId: String) async throws
    func markSeen(taskId: String) async throws
    /// R2-3b: the task's isolated copy; nil when it runs in the original directory.
    func copyInfo(taskId: String) async throws -> LiveCopyInfo?
    /// R2-3b: every pending hunk brought back; (brought back, in conflict).
    func bringAll(taskId: String) async throws -> (applied: Int, conflicts: Int)
    /// R2-3b: merge the original directory in; the message to prefill.
    func resolveConflicts(taskId: String) async throws -> String
}

public extension ChangesActions {
    func copyInfo(taskId: String) async throws -> LiveCopyInfo? { nil }
    func bringAll(taskId: String) async throws -> (applied: Int, conflicts: Int) { (0, 0) }
    func resolveConflicts(taskId: String) async throws -> String { "" }
}

public enum ChangeScopeChoice: String, CaseIterable, Hashable, Sendable {
    case turn, seen, session
    /// R2-3b: an isolated copy against its starting point.
    case origin

    public var title: String {
        switch self {
        case .turn: "本轮"
        case .seen: "自上次已看"
        case .session: "整个会话"
        case .origin: "相对原目录"
        }
    }
}

public enum ChangesText {
    /// Files with more changed lines than this start folded.
    public static let foldAt = 400

    public static func state(_ state: String, origin: Bool = false) -> String? {
        switch state {
        case "accepted": "已接受"
        case "rejected": origin ? "已丢弃" : "已拒绝"
        case "stale": origin ? "已过期，无法单独丢弃" : "已过期，无法单独拒绝"
        case "applied": "已带回"
        case "conflict": "冲突"
        default: nil
        }
    }

    /// R2-3b: hunks of the copy not brought back yet.
    public static func pending(_ diff: LiveReviewDiff) -> Int {
        diff.files.flatMap(\.hunks).filter { $0.state == "pending" || $0.state == "conflict" }.count
    }

    public static func copyHelp(_ copy: LiveCopyInfo) -> String {
        let created = copy.createdMs > 0
            ? Date(timeIntervalSince1970: Double(copy.createdMs) / 1000).formatted(date: .abbreviated, time: .shortened)
            : "未知"
        return "副本：\(copy.path)\n原目录：\(copy.origin)\n起点：创建于 \(created)"
    }

    public static func folds(_ file: LiveReviewDiff.File) -> Bool {
        file.additions + file.deletions > foldAt
    }

    /// `@@ -22,7 +22,7 @@ fn x()` → `@@ -22,7 +22,7 @@` (the range only, so it fits).
    public static func range(_ header: String) -> String {
        guard header.hasPrefix("@@"), let end = header.dropFirst(2).range(of: "@@") else { return header }
        return String(header[..<end.upperBound])
    }
}

/// The changes page's state for one task.
@MainActor @Observable
public final class ChangesStore {
    public let taskID: String
    @ObservationIgnored private let actions: ChangesActions
    public var scope: ChangeScopeChoice = .turn
    /// The turn the 本轮 view shows; nil: the latest.
    public var turnID: String?
    public private(set) var diff = LiveReviewDiff()
    public private(set) var loading = false
    public var errorText: String?
    public var unfolded: Set<String> = []
    /// R2-3b: the task's isolated copy, when it has one.
    public private(set) var copy: LiveCopyInfo?

    public init(taskID: String, actions: ChangesActions) {
        self.taskID = taskID
        self.actions = actions
    }

    /// R2-3b: straight to 相对原目录 (a delegated child's 去带回).
    public func showOrigin() async {
        scope = .origin
        await load()
    }

    public func show(turn: String?) async {
        scope = .turn
        turnID = turn
        await load()
    }

    public func load() async {
        loading = true
        defer { loading = false }
        copy = try? await actions.copyInfo(taskId: taskID)
        if scope == .origin && copy == nil { scope = .turn }
        do {
            diff = try await actions.changesDiff(taskId: taskID, scope: scope.rawValue,
                                                 turnId: scope == .turn ? turnID : nil)
            errorText = nil
        } catch {
            errorText = "读不到改动：\(error.localizedDescription)"
        }
    }

    public func act(_ method: String, hunk: String, text: String? = nil) async {
        do {
            try await actions.changeAction(method, taskId: taskID, scope: scope.rawValue,
                                           turnId: scope == .turn ? turnID : nil, hunkId: hunk, text: text)
        } catch {
            errorText = error.localizedDescription
        }
        await load()
        announce()
    }

    /// R2-2 目检: the conversation's turn headers refresh their badges now.
    private func announce() {
        NotificationCenter.default.post(name: .aeroricTurnsChanged, object: nil, userInfo: ["taskID": taskID])
    }

    public func all(_ method: String, in file: LiveReviewDiff.File) async {
        for hunk in file.hunks where hunk.state == "pending" {
            do {
                try await actions.changeAction(method, taskId: taskID, scope: scope.rawValue,
                                               turnId: scope == .turn ? turnID : nil, hunkId: hunk.id, text: nil)
            } catch {
                errorText = error.localizedDescription
            }
        }
        await load()
        announce()
    }

    /// R2-3b: everything pending goes back to the original directory.
    public func bringAll() async {
        do {
            let done = try await actions.bringAll(taskId: taskID)
            errorText = done.conflicts > 0 ? "\(done.conflicts) 处和原目录冲突，没有带回" : nil
        } catch {
            errorText = error.localizedDescription
        }
        await load()
        announce()
    }

    /// R2-3b: merge the original directory into the copy and put the message
    /// for the agent into this session's composer (never sent from here).
    public func resolve() async {
        do {
            let text = try await actions.resolveConflicts(taskId: taskID)
            NotificationCenter.default.post(name: .aeroricPrefillComposer, object: nil,
                                            userInfo: ["taskID": taskID, "text": text])
            errorText = nil
        } catch {
            errorText = error.localizedDescription
        }
        await load()
    }

    public func send() async {
        do { try await actions.sendComments(taskId: taskID) } catch { errorText = error.localizedDescription }
        await load()
    }

    public func markSeen() async {
        do { try await actions.markSeen(taskId: taskID) } catch { errorText = error.localizedDescription }
        await load()
    }
}

/// The 改动 tab: three ranges, seen, comments to send, files and hunks.
public struct ChangesPanel: View {
    @Bindable var store: ChangesStore
    @State private var editing: String?
    @State private var draft = ""

    public init(store: ChangesStore) {
        self.store = store
    }

    public var body: some View {
        ScrollView(showsIndicators: false) {
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                // R2-3b: a copy adds 相对原目录; the range takes its own row.
                CapsuleSlider(selection: $store.scope, groupTitle: "对比范围", identifier: "changes.scope", fill: true,
                              segments: ChangeScopeChoice.allCases
                                .filter { $0 != .origin || store.copy != nil }
                                .map { .init(value: $0, title: $0.title) })
                if store.scope == .origin {
                    originBar
                } else {
                    HStack(spacing: AeroricSpace.s) {
                        if let copy = store.copy {
                            copyBadge(copy)
                        }
                        Spacer(minLength: AeroricSpace.s)
                        Button("标为已看") { Task { await store.markSeen() } }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            .fixedSize()
                            .accessibilityIdentifier("changes.seen")
                    }
                }
                if store.diff.unsent > 0 {
                    Button { Task { await store.send() } } label: {
                        HStack(spacing: AeroricSpace.s) {
                            Image(systemName: "text.bubble")
                            Text("\(store.diff.unsent) 条评论待发送").lineLimit(1)
                            Spacer(minLength: AeroricSpace.s)
                            Text("发给 Agent").fontWeight(.medium)
                        }
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.accent)
                        .padding(.horizontal, AeroricSpace.l)
                        .frame(height: 30)
                        .background(Capsule(style: .continuous).fill(AeroricColor.accent.opacity(0.12)))
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("changes.send")
                }
                if let error = store.errorText {
                    Text(error)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.warning)
                        .lineLimit(2)
                }
                if store.diff.files.isEmpty {
                    Text(store.loading ? "正在读取改动…" : (store.diff.emptyReason ?? "没有改动"))
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .padding(.top, AeroricSpace.l)
                }
                ForEach(store.diff.files) { file in fileCard(file) }
            }
            .padding(AeroricSpace.l)
        }
        .task(id: store.scope) { await store.load() }
        .accessibilityIdentifier("changes.panel")
    }

    /// R2-3b: what is left to bring back, and 全部带回 — side by side, or
    /// one above the other when the panel is narrow (目检 2026-11-05).
    private var originBar: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: AeroricSpace.s) {
                originLeft
                Spacer(minLength: AeroricSpace.s)
                bringAllButton
            }
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                originLeft
                bringAllButton
            }
        }
    }

    @ViewBuilder private var bringAllButton: some View {
        if store.diff.files.flatMap(\.hunks).contains(where: { $0.state == "pending" }) {
            Button("全部带回") { Task { await store.bringAll() } }
                .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                .fixedSize()
                .accessibilityIdentifier("changes.origin.bringAll")
        }
    }

    private var originLeft: some View {
        let left = ChangesText.pending(store.diff)
        return Text(left > 0 ? "副本里还有 \(left) 处没带回" : "副本里的改动都处理完了")
                .font(.system(size: AeroricType.caption, weight: .medium))
                .foregroundStyle(left > 0 ? AeroricColor.warning : AeroricColor.success)
                .lineLimit(1)
                .padding(.horizontal, AeroricSpace.m)
                .frame(height: 24)
                .background(Capsule(style: .continuous)
                    .fill((left > 0 ? AeroricColor.warning : AeroricColor.success).opacity(0.12)))
                .help(store.copy.map(ChangesText.copyHelp) ?? "")
                .fixedSize()
                .accessibilityIdentifier("changes.origin.left")
    }

    private func copyBadge(_ copy: LiveCopyInfo) -> some View {
        Label("独立副本", systemImage: "square.on.square")
            .font(.system(size: AeroricType.caption, weight: .medium))
            .foregroundStyle(AeroricColor.textSecondary)
            .lineLimit(1)
            .padding(.horizontal, AeroricSpace.m)
            .frame(height: 22)
            .background(Capsule(style: .continuous).fill(AeroricColor.layer2))
            .help(ChangesText.copyHelp(copy))
            .fixedSize()
    }

    private func fileCard(_ file: LiveReviewDiff.File) -> some View {
        let folded = ChangesText.folds(file) && !store.unfolded.contains(file.path)
        return VStack(alignment: .leading, spacing: 0) {
            // R2-2 目检: the path has its own line, so it is not cut short.
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: file.binary ? "doc" : "doc.text")
                    .font(.system(size: AeroricType.caption))
                Text(file.path)
                    .font(.system(size: AeroricType.secondary, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.head)
                    .help(file.path)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, AeroricSpace.l)
            .padding(.top, AeroricSpace.m)
            HStack(spacing: AeroricSpace.s) {
                Text("+\(file.additions) −\(file.deletions)")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .fixedSize()
                Spacer(minLength: AeroricSpace.s)
                if folded {
                    Button("展开") { store.unfolded.insert(file.path) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                } else if file.hunks.contains(where: { $0.state == "pending" }) {
                    let origin = store.scope == .origin
                    if !(origin && file.binary) {
                        Button(origin ? "全部带回" : "全部接受") { Task { await store.all("changes.accept", in: file) } }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                    }
                    if !file.binary {
                        Button(origin ? "全部丢弃" : "全部拒绝") { Task { await store.all("changes.reject", in: file) } }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                    }
                }
            }
            .padding(.horizontal, AeroricSpace.l)
            .frame(height: 34)
            if !folded {
                ForEach(file.hunks) { hunk in
                    HairlineDivider()
                    hunkView(hunk, binary: file.binary)
                }
                if file.binary {
                    HairlineDivider()
                    Text("二进制文件，只能整轮撤销")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .padding(AeroricSpace.l)
                }
            }
        }
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(AeroricColor.layer2))
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
    }

    private func hunkView(_ hunk: LiveReviewDiff.Hunk, binary: Bool) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: AeroricSpace.xs) {
                Text(ChangesText.range(hunk.header))
                    .font(.system(size: AeroricType.caption, design: .monospaced))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
                    .fixedSize()
                    .help(hunk.header)
                Spacer(minLength: AeroricSpace.s)
                let origin = store.scope == .origin
                if let label = ChangesText.state(hunk.state, origin: origin) {
                    CheckpointBadge(text: label, tint: hunk.state == "accepted" || hunk.state == "applied"
                                    ? (hunk.state == "applied" ? AeroricColor.textTertiary : AeroricColor.success)
                                    : hunk.state == "rejected" ? AeroricColor.textTertiary : AeroricColor.warning)
                } else {
                    if !(origin && binary) {
                        small(origin ? "带回" : "接受", id: origin ? "bring" : "accept") {
                            Task { await store.act("changes.accept", hunk: hunk.id) }
                        }
                    }
                    if !binary {
                        small(origin ? "丢弃" : "拒绝", id: origin ? "discard" : "reject") {
                            Task { await store.act("changes.reject", hunk: hunk.id) }
                        }
                    }
                }
                small(hunk.comment == nil ? "评论" : "改评论", id: "comment") {
                    draft = hunk.comment ?? ""
                    editing = hunk.id
                }
            }
            .padding(.horizontal, AeroricSpace.l)
            .frame(height: 28)
            // R2-3b: its own row, so a narrow panel never widens past its edge.
            if hunk.state == "conflict" {
                HStack(spacing: AeroricSpace.s) {
                    Text("原目录这里已经改过")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                    Spacer(minLength: AeroricSpace.s)
                    small("交给 Agent 处理", id: "resolve") { Task { await store.resolve() } }
                        .help("先把原目录的新内容合进副本（冲突处留标记），再在输入框里预填说明；不会自动发送")
                }
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 28)
            }
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(hunk.lines.enumerated()), id: \.offset) { _, line in lineRow(line) }
            }
            if editing == hunk.id {
                HStack(spacing: AeroricSpace.s) {
                    CapsuleField(placeholder: "写给 Agent 的意见", text: $draft, height: 26)
                        .onSubmit { save(hunk) }
                    Button("保存") { save(hunk) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true)).fixedSize()
                    Button("取消") { editing = nil }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                }
                .padding(AeroricSpace.m)
            } else if let comment = hunk.comment {
                HStack(spacing: AeroricSpace.s) {
                    Image(systemName: "text.bubble")
                    Text(comment).lineLimit(1).truncationMode(.tail)
                    Spacer(minLength: 0)
                    if hunk.commentSent { Text("已发送").foregroundStyle(AeroricColor.textTertiary).fixedSize() }
                }
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 28)
                .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(AeroricColor.layer1))
                .padding(AeroricSpace.s)
            }
        }
        .accessibilityIdentifier("changes.hunk")
    }

    private func lineRow(_ line: LiveReviewDiff.Line) -> some View {
        let tint: Color? = line.kind == "+" ? AeroricColor.success : line.kind == "-" ? AeroricColor.danger : nil
        return HStack(spacing: AeroricSpace.s) {
            Text((line.new ?? line.old).map(String.init) ?? "")
                .frame(width: 34, alignment: .trailing)
                .foregroundStyle(AeroricColor.textTertiary)
            Text("\(line.kind)\(line.text)")
                .foregroundStyle(tint ?? AeroricColor.text)
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: 0)
        }
        .font(.system(size: AeroricType.caption, design: .monospaced))
        .padding(.horizontal, AeroricSpace.s)
        .frame(height: 18)
        .background((tint ?? .clear).opacity(tint == nil ? 0 : 0.10))
    }

    private func small(_ title: String, id: String, action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .fixedSize()
            .accessibilityIdentifier("changes.hunk.\(id)")
    }

    private func save(_ hunk: LiveReviewDiff.Hunk) {
        let text = draft
        editing = nil
        Task { await store.act("changes.comment", hunk: hunk.id, text: text) }
    }
}

/// R2-2: a turn header's 查看改动 asks the workspace to show this.
public extension Notification.Name {
    static let aeroricShowChanges = Notification.Name("aeroric.showChanges")
    /// A hunk was accepted or rejected: turn badges may have changed.
    static let aeroricTurnsChanged = Notification.Name("aeroric.turnsChanged")
    /// R2-3b: text for a session's composer (`taskID`, `text`), never sent.
    static let aeroricPrefillComposer = Notification.Name("aeroric.prefillComposer")
}
