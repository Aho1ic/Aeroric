import AeroricClient
import AeroricProtocol
import Foundation
import Observation

/// Git 侧栏 → PR 审查 (D1 step 3, 2026-11-21): a project's pinned PR
/// snapshots and their read-only reviews. Nothing here writes to GitHub.
@MainActor @Observable
public final class PullReviewBook {
    public struct Calls: Sendable {
        public var pull: @Sendable (String, String) async throws -> LivePullSnapshot?
        public var snapshots: @Sendable (String) async throws -> [LivePullSnapshot]
        public var reviewStart: @Sendable (String, String, String) async throws -> Void
        public var reviews: @Sendable (String) async throws -> [LivePullReview]

        public init(pull: @escaping @Sendable (String, String) async throws -> LivePullSnapshot?,
                    snapshots: @escaping @Sendable (String) async throws -> [LivePullSnapshot],
                    reviewStart: @escaping @Sendable (String, String, String) async throws -> Void,
                    reviews: @escaping @Sendable (String) async throws -> [LivePullReview]) {
            self.pull = pull; self.snapshots = snapshots; self.reviewStart = reviewStart; self.reviews = reviews
        }
    }

    public private(set) var projectID: String?
    public private(set) var snapshots: [LivePullSnapshot] = []
    public private(set) var reviews: [LivePullReview] = []
    /// What is being done now ("正在取 PR…"), for the spinner.
    public private(set) var working: String?
    public var errorText: String?
    @ObservationIgnored private var calls: Calls?

    public init() {}

    public func attach(client: GitHubClient?) {
        guard let client else {
            attach(nil)
            return
        }
        attach(Calls(pull: { try await client.pullSnapshot(projectId: $0, url: $1) },
                     snapshots: { try await client.snapshots(projectId: $0) },
                     reviewStart: { try await client.reviewStart(projectId: $0, snapshotId: $1, agent: $2) },
                     reviews: { try await client.reviews(projectId: $0) }))
    }

    public func attach(_ calls: Calls?) {
        self.calls = calls
        snapshots = []
        reviews = []
        errorText = nil
    }

    /// Show `projectID`'s PRs (read again when it changes or is asked).
    public func show(projectID: String) async {
        if self.projectID != projectID {
            self.projectID = projectID
            snapshots = []
            reviews = []
            errorText = nil
        }
        await refresh()
    }

    public func refresh() async {
        guard let calls, let projectID else { return }
        do {
            snapshots = try await calls.snapshots(projectID)
            reviews = try await calls.reviews(projectID)
        } catch {
            errorText = "读取 PR 失败：\(error)"
        }
    }

    private func run(_ doing: String, _ what: String, _ body: (Calls, String) async throws -> Void) async -> Bool {
        guard let calls, let projectID, working == nil else { return false }
        working = doing
        defer { working = nil }
        do {
            try await body(calls, projectID)
            errorText = nil
            await refresh()
            return true
        } catch {
            errorText = "\(what)失败：\(error)"
            return false
        }
    }

    /// Pin the PR a link names; also how a snapshot is refreshed (its URL again).
    @discardableResult
    public func pin(_ link: String) async -> Bool {
        let link = link.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !link.isEmpty else { return false }
        return await run("正在取 PR…", "固定快照") { calls, project in _ = try await calls.pull(project, link) }
    }

    @discardableResult
    public func startReview(_ snapshot: LivePullSnapshot, agent: String) async -> Bool {
        await run("正在开审查…", "开始审查") { calls, project in
            try await calls.reviewStart(project, snapshot.id, agent)
        }
    }

    public func reviews(of snapshot: LivePullSnapshot) -> [LivePullReview] {
        reviews.filter { $0.snapshotId == snapshot.id }
    }
}

/// The section's words.
public enum PullText {
    public static func short(_ sha: String) -> String { String(sha.prefix(7)) }

    public static func title(_ s: LivePullSnapshot) -> String { "#\(s.number) \(s.title)" }

    /// "快照于 11月21日 12:30 · 对应提交 abc1234".
    public static func pinned(_ s: LivePullSnapshot) -> String {
        let at = Date(timeIntervalSince1970: TimeInterval(s.fetchedMs) / 1000)
            .formatted(date: .abbreviated, time: .shortened)
        return "快照于 \(at) · 对应提交 \(short(s.headSha))"
    }

    public static func branches(_ s: LivePullSnapshot) -> String {
        "\(s.owner)/\(s.repo) · \(s.headRef) → \(s.baseRef)"
    }

    public static let superseded = "PR 已更新，这份快照针对的是旧版本"

    /// "2 个文件 · +3 −1", "列表不完整" when cut.
    public static func files(_ s: LivePullSnapshot) -> String {
        let added = s.files.reduce(0) { $0 + $1.additions }
        let removed = s.files.reduce(0) { $0 + $1.deletions }
        return "\(s.files.count) 个文件 · +\(added) −\(removed)" + (s.truncated ? " · 列表不完整" : "")
    }

    /// "检查：1 失败 · 1 进行中 · 2 通过"; nil with no checks.
    public static func checks(_ s: LivePullSnapshot) -> String? {
        guard !s.checks.isEmpty else { return nil }
        var failed = 0, running = 0, passed = 0, other = 0
        for c in s.checks {
            switch c.conclusion {
            case "success": passed += 1
            case "failure", "timed_out", "cancelled", "action_required": failed += 1
            case nil: running += 1
            default: other += 1
            }
        }
        let parts = [(failed, "失败"), (running, "进行中"), (passed, "通过"), (other, "其他")]
            .filter { $0.0 > 0 }.map { "\($0.0) \($0.1)" }
        return "检查：" + parts.joined(separator: " · ")
    }

    public static func check(_ c: LivePullCheck) -> String {
        let result: String = switch c.conclusion {
        case "success": "通过"
        case "failure": "失败"
        case "timed_out": "超时"
        case "cancelled": "已取消"
        case "skipped": "跳过"
        case "neutral": "中性"
        case "action_required": "需要处理"
        case nil: c.status == "queued" ? "排队中" : "进行中"
        case let other?: other
        }
        return "\(c.name)：\(result)"
    }

    public static func comment(_ c: LivePullComment) -> String {
        let place = c.place ?? "整个 PR"
        return "\(place) · @\(c.author)" + (c.outdated ? "（针对旧版本）" : "")
    }

    /// "审查中…", "3 条结论", "没有发现问题", or why it failed.
    public static func review(_ r: LivePullReview) -> String {
        let line: String = switch r.state {
        case "running": "审查中…"
        case "done": r.findings.isEmpty ? "没有发现问题" : "\(r.findings.count) 条结论"
        default: "没有结论：\(r.reason ?? "审查失败")"
        }
        var extra: [String] = []
        if r.dropped > 0 { extra.append("另有 \(r.dropped) 条不在这个 PR 里，没收") }
        if r.outdated { extra.append("针对旧版本") }
        return ([line] + extra).joined(separator: " · ")
    }

    public static func severity(_ s: String) -> String {
        switch s {
        case "high": "高"
        case "low": "低"
        default: "中"
        }
    }

    public static func place(_ f: LivePullFinding) -> String {
        f.line.map { "\(f.path):\($0)" } ?? f.path
    }

    /// The finding's file in the review's checkout, relative to the project.
    public static func checkoutPath(_ f: LivePullFinding, snapshotID: String) -> String {
        ".aeroric/pr-reviews/\(snapshotID)/\(f.path)"
    }
}
