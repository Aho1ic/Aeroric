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
}

public enum ChangeScopeChoice: String, CaseIterable, Hashable, Sendable {
    case turn, seen, session

    public var title: String {
        switch self {
        case .turn: "本轮"
        case .seen: "自上次已看"
        case .session: "整个会话"
        }
    }
}

public enum ChangesText {
    /// Files with more changed lines than this start folded.
    public static let foldAt = 400

    public static func state(_ state: String) -> String? {
        switch state {
        case "accepted": "已接受"
        case "rejected": "已拒绝"
        case "stale": "已过期，无法单独拒绝"
        default: nil
        }
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

    public init(taskID: String, actions: ChangesActions) {
        self.taskID = taskID
        self.actions = actions
    }

    public func show(turn: String?) async {
        scope = .turn
        turnID = turn
        await load()
    }

    public func load() async {
        loading = true
        defer { loading = false }
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
                HStack(spacing: AeroricSpace.s) {
                    CapsuleSlider(selection: $store.scope, groupTitle: "对比范围", identifier: "changes.scope",
                                  segments: ChangeScopeChoice.allCases.map { .init(value: $0, title: $0.title) })
                        .fixedSize()
                    Spacer(minLength: AeroricSpace.s)
                    Button("标为已看") { Task { await store.markSeen() } }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("changes.seen")
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
                    Button("全部接受") { Task { await store.all("changes.accept", in: file) } }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true)).fixedSize()
                    if !file.binary {
                        Button("全部拒绝") { Task { await store.all("changes.reject", in: file) } }
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
                if let label = ChangesText.state(hunk.state) {
                    CheckpointBadge(text: label, tint: hunk.state == "accepted" ? AeroricColor.success
                                    : hunk.state == "rejected" ? AeroricColor.textTertiary : AeroricColor.warning)
                } else {
                    small("接受", id: "accept") { Task { await store.act("changes.accept", hunk: hunk.id) } }
                    if !binary {
                        small("拒绝", id: "reject") { Task { await store.act("changes.reject", hunk: hunk.id) } }
                    }
                }
                small(hunk.comment == nil ? "评论" : "改评论", id: "comment") {
                    draft = hunk.comment ?? ""
                    editing = hunk.id
                }
            }
            .padding(.horizontal, AeroricSpace.l)
            .frame(height: 28)
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
}
