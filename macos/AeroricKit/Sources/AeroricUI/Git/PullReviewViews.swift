import AeroricProtocol
import SwiftUI

/// Git 侧栏 → PR 审查 (D1 step 3, 2026-11-21): paste a link to pin a PR;
/// each snapshot opens a sheet with its files, checks, comments and
/// read-only reviews. Nothing here writes to GitHub.
public struct PullRequestsSection: View {
    let book: PullReviewBook
    let onOpenFinding: (String) -> Void
    let onOpenTask: (String) -> Void
    @State private var link = ""
    @State private var open: LivePullSnapshot?

    public init(book: PullReviewBook, onOpenFinding: @escaping (String) -> Void,
                onOpenTask: @escaping (String) -> Void) {
        self.book = book
        self.onOpenFinding = onOpenFinding
        self.onOpenTask = onOpenTask
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                TextField("粘贴 PR 链接", text: $link)
                    .textFieldStyle(.capsule)
                    .font(.system(size: AeroricType.secondary))
                    .onSubmit { pin() }
                    .accessibilityIdentifier("git.pr.link")
                Button("固定") { pin() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .fixedSize()
                    .disabled(link.trimmingCharacters(in: .whitespaces).isEmpty || book.working != nil)
                    .help("只读取 PR，不会向 GitHub 写任何东西")
                    .accessibilityIdentifier("git.pr.pin")
            }
            if let working = book.working {
                HStack(spacing: AeroricSpace.xs) {
                    ProgressView().controlSize(.mini)
                    Text(working)
                }
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            }
            if let error = book.errorText {
                Text(error)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.danger)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
            ForEach(book.snapshots) { snapshot in row(snapshot) }
        }
        .padding(.horizontal, AeroricSpace.s)
        .sheet(item: $open) { snapshot in
            PullSnapshotSheet(book: book, snapshot: book.snapshots.first { $0.id == snapshot.id } ?? snapshot,
                              onOpenFinding: { path in open = nil; onOpenFinding(path) },
                              onOpenTask: { id in open = nil; onOpenTask(id) },
                              onClose: { open = nil })
        }
    }

    private func pin() {
        let text = link
        Task { if await book.pin(text) { link = "" } }
    }

    private func row(_ snapshot: LivePullSnapshot) -> some View {
        Button { open = snapshot } label: {
            VStack(alignment: .leading, spacing: 2) {
                Text(PullText.title(snapshot))
                    .font(.system(size: AeroricType.secondary, weight: .medium))
                    .lineLimit(1)
                Text(PullText.pinned(snapshot))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                if snapshot.superseded {
                    Text("PR 已更新")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.warning)
                }
                ForEach(book.reviews(of: snapshot).prefix(1)) { review in
                    Text(PullText.review(review))
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("git.pr.snapshot.\(snapshot.id)")
    }
}

/// One snapshot: what it pins, and its reviews.
struct PullSnapshotSheet: View {
    let book: PullReviewBook
    let snapshot: LivePullSnapshot
    let onOpenFinding: (String) -> Void
    let onOpenTask: (String) -> Void
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(PullText.title(snapshot)).font(.system(size: AeroricType.body, weight: .semibold))
                    Text(PullText.branches(snapshot))
                    Text(PullText.pinned(snapshot))
                }
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                Spacer()
                Button("关闭", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .keyboardShortcut(.cancelAction)
            }
            .padding(AeroricSpace.l)
            HairlineDivider()
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.m) {
                    if snapshot.superseded {
                        Text(PullText.superseded)
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.warning)
                    }
                    actions
                    reviews
                    section(PullText.files(snapshot)) {
                        ForEach(snapshot.files.prefix(60), id: \.path) { file in
                            Text("\(file.status) \(file.path) +\(file.additions) −\(file.deletions)")
                        }
                        if snapshot.files.count > 60 { Text("另有 \(snapshot.files.count - 60) 个") }
                    }
                    if let checks = PullText.checks(snapshot) {
                        section(checks) {
                            ForEach(snapshot.checks, id: \.name) { Text(PullText.check($0)) }
                        }
                    }
                    if !snapshot.comments.isEmpty {
                        section("已有评论 \(snapshot.comments.count) 条") {
                            ForEach(Array(snapshot.comments.prefix(30).enumerated()), id: \.offset) { _, c in
                                VStack(alignment: .leading, spacing: 1) {
                                    Text(PullText.comment(c)).foregroundStyle(AeroricColor.textTertiary)
                                    Text(c.body).lineLimit(4)
                                }
                            }
                        }
                    }
                }
                .padding(AeroricSpace.l)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .frame(width: 560, height: 620)
        .background(AeroricColor.canvas)
    }

    private var actions: some View {
        HStack(spacing: AeroricSpace.s) {
            Menu("让 Agent 审查") {
                Button("Claude Code") { Task { await book.startReview(snapshot, agent: "claude") } }
                Button("Codex") { Task { await book.startReview(snapshot, agent: "codex") } }
            }
            .fixedSize()
            .disabled(snapshot.superseded || book.working != nil)
            .help(snapshot.superseded ? "这个 PR 已经有更新的快照，审最新的那份" : "在 PR 自己的检出里只读审查，结论存成草稿")
            .accessibilityIdentifier("git.pr.review")
            Button("刷新快照") { Task { await book.pin(snapshot.url) } }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .fixedSize()
                .disabled(snapshot.url.isEmpty || book.working != nil)
                .help("重新读这个 PR；有新提交时存成新快照，这份标“PR 已更新”")
            Button("更新结论") { Task { await book.refresh() } }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .fixedSize()
            if let working = book.working {
                ProgressView().controlSize(.small).help(working)
            }
        }
    }

    @ViewBuilder private var reviews: some View {
        if let error = book.errorText {
            Text(error)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.danger)
                .textSelection(.enabled)
        }
        ForEach(book.reviews(of: snapshot)) { review in
            VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                HStack {
                    Text(PullText.review(review))
                        .font(.system(size: AeroricType.secondary, weight: .medium))
                    Spacer()
                    if !review.taskId.isEmpty {
                        Button("打开审查会话") { onOpenTask(review.taskId) }
                            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                            .fixedSize()
                    }
                }
                ForEach(Array(review.findings.enumerated()), id: \.offset) { _, finding in
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: AeroricSpace.xs) {
                            Text(PullText.severity(finding.severity))
                                .foregroundStyle(finding.severity == "high" ? AeroricColor.danger : AeroricColor.textSecondary)
                            Button(PullText.place(finding)) {
                                onOpenFinding(PullText.checkoutPath(finding, snapshotID: snapshot.id))
                            }
                            .buttonStyle(.link)
                            .help("打开 PR 检出里的这个文件")
                        }
                        Text(finding.body).fixedSize(horizontal: false, vertical: true)
                        if !finding.basis.isEmpty {
                            Text(finding.basis).foregroundStyle(AeroricColor.textTertiary)
                        }
                    }
                    .font(.system(size: AeroricType.caption))
                }
            }
            .padding(AeroricSpace.m)
            .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.m))
        }
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            Text(title).font(.system(size: AeroricType.secondary, weight: .medium))
            VStack(alignment: .leading, spacing: 2) { content() }
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .textSelection(.enabled)
        }
    }
}
