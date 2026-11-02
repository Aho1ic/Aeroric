import Foundation
import SwiftUI
import Observation
import AeroricClient
import AeroricProtocol

/// B5.5-1 (2026-10-31): what the Wiki page asks of the host.
@MainActor
public protocol WikiService {
    func pages(projectID: String) async throws -> LiveWikiPageList
    func page(projectID: String, slug: String) async throws -> LiveWikiPage?
    func compile(projectID: String) async throws -> LiveWikiReport
    func edit(projectID: String, slug: String, markdown: String) async throws -> LiveWikiPage?
    func revisions(projectID: String, slug: String) async throws -> [LiveWikiRevision]
    func revert(projectID: String, slug: String, n: Int) async throws -> LiveWikiPage?
    func issues(projectID: String) async throws -> [LiveWikiIssue]
    func issueAction(projectID: String, id: String, accept: Bool) async throws
    func exportPlan(projectID: String, dir: String) async throws -> LiveWikiExportPlan
    func export(projectID: String, dir: String) async throws -> [String]
}

@MainActor
@Observable
public final class WikiStore {
    public var scope: String?
    public var selected = "index"
    public private(set) var pages: [LiveWikiPageSummary] = []
    public private(set) var page: LiveWikiPage?
    public private(set) var revisions: [LiveWikiRevision] = []
    /// R1-1: what the last compile could not read.
    public private(set) var gaps = LiveWikiSourceGaps()
    /// Pages read this visit, so going back to one shows it at once.
    @ObservationIgnored private var cache: [String: LiveWikiPage] = [:]
    public private(set) var issues: [LiveWikiIssue] = []
    public private(set) var lastReport: LiveWikiReport?
    public private(set) var busy = false
    public var errorText: String?
    /// Why the export sheet's directory was refused, shown inside the sheet.
    public var exportError: String?
    public var notice: String?

    @ObservationIgnored private let service: (any WikiService)?

    public init(service: (any WikiService)?) {
        self.service = service
    }

    public var openIssues: [LiveWikiIssue] { issues.filter(\.open) }

    /// The tree: index, runbook, then decisions grouped by category.
    public var tree: (top: [LiveWikiPageSummary], decisions: [(category: String, pages: [LiveWikiPageSummary])]) {
        let top = pages.filter { $0.kind != "decision" }
        var groups: [String: [LiveWikiPageSummary]] = [:]
        for p in pages where p.kind == "decision" { groups[p.category ?? "其他", default: []].append(p) }
        return (top, groups.keys.sorted().map { ($0, groups[$0] ?? []) })
    }

    public func show(scope: String?, slug: String = "index") {
        self.scope = scope
        selected = slug
    }

    /// The tree, what could not be read, the open page and the issues.
    public func reload() async {
        guard let service, let scope else { return }
        cache = [:]
        do {
            let list = try await service.pages(projectID: scope)
            guard self.scope == scope else { return }
            pages = list.pages
            gaps = list.gaps
            if !pages.contains(where: { $0.slug == selected }) { selected = pages.first?.slug ?? "index" }
            await loadPage()
            issues = try await service.issues(projectID: scope)
        } catch where AgentConfigStore.isCancelled(error) {
            return
        } catch {
            errorText = "读取 Wiki 失败：\(AgentConfigStore.describe(error))"
        }
    }

    /// Show a page: at once from what was read before, then fresh.
    public func open(_ slug: String) async {
        selected = slug
        if let cached = cache[slug] { page = cached }
        await loadPage()
    }

    private func loadPage() async {
        guard let service, let scope else { return }
        let wanted = selected
        if pages.isEmpty {
            page = nil
            revisions = []
            return
        }
        do {
            let next = try await service.page(projectID: scope, slug: wanted)
            let past = try await service.revisions(projectID: scope, slug: wanted)
            if let next { cache[wanted] = next }
            // The user moved on while this was reading: that read owns the page now.
            guard selected == wanted, self.scope == scope else { return }
            page = next
            revisions = past
        } catch where AgentConfigStore.isCancelled(error) {
            return
        } catch {
            errorText = "读取 Wiki 失败：\(AgentConfigStore.describe(error))"
        }
    }

    private func run(_ what: String, _ body: @MainActor (any WikiService, String) async throws -> Void) async {
        guard let service, let scope else { return }
        busy = true
        defer { busy = false }
        do {
            try await body(service, scope)
        } catch {
            errorText = "\(what)失败：\(AgentConfigStore.describe(error))"
        }
        await reload()
    }

    public func compile() async {
        await run("重新编译") { service, scope in
            let report = try await service.compile(projectID: scope)
            self.lastReport = report
            self.notice = WikiText.report(report)
        }
    }

    public func save(_ markdown: String) async {
        let slug = selected
        await run("保存") { _ = try await $0.edit(projectID: $1, slug: slug, markdown: markdown) }
    }

    public func revert(_ n: Int) async {
        let slug = selected
        await run("回到旧版本") { _ = try await $0.revert(projectID: $1, slug: slug, n: n) }
    }

    public func resolve(_ issue: LiveWikiIssue, accept: Bool) async {
        await run(accept ? "采纳建议" : "关闭 issue") { try await $0.issueAction(projectID: $1, id: issue.id, accept: accept) }
    }

    /// R1-1: this decision page shows what was last compiled.
    public func sourceUnavailable(slug: String, kind: String) -> Bool {
        kind == "decision" && (gaps.all || gaps.slugs.contains(slug))
    }

    public func exportPlan(_ dir: String) async -> LiveWikiExportPlan? {
        guard let service, let scope else { return nil }
        exportError = nil
        do { return try await service.exportPlan(projectID: scope, dir: dir) } catch {
            exportError = AgentConfigStore.describe(error)
            return nil
        }
    }

    public func export(_ dir: String) async {
        await run("导出") { service, scope in
            let files = try await service.export(projectID: scope, dir: dir)
            self.notice = "已导出 \(files.count) 页到 \(dir)/"
        }
    }
}

/// B5.5-1: the Wiki page's words.
public enum WikiText {
    /// The line under the page header; nil when every source was read.
    public static func gapNotice(_ gaps: LiveWikiSourceGaps) -> String? {
        if gaps.all { return "读不到项目文件夹，显示的是上次的内容。恢复后自动更新" }
        if !gaps.slugs.isEmpty { return "有 \(gaps.slugs.count) 篇笔记读不到，相关页面保持上次内容" }
        return nil
    }

    /// A section heading's words, symbol and colour, so a page reads at a glance.
    public static func section(_ heading: String) -> (title: String, symbol: String, tint: Color) {
        switch heading.trimmingCharacters(in: .whitespaces).lowercased() {
        case "problem", "问题": ("问题", "exclamationmark.circle", AeroricColor.danger)
        case "decision", "决定": ("决定", "checkmark.circle", AeroricColor.success)
        case "proposal", "提议": ("提议", "lightbulb", AeroricColor.accent)
        case "alternatives considered", "考虑过的方案": ("考虑过的方案", "arrow.triangle.branch", Color.purple)
        case "consequences", "影响与代价": ("影响与代价", "scalemass", AeroricColor.warning)
        case "acceptance criteria", "验收": ("验收", "checklist", AeroricColor.success)
        case "risks", "风险": ("风险", "exclamationmark.triangle", AeroricColor.warning)
        default: (heading, "text.alignleft", AeroricColor.accent)
        }
    }

    public static func statusTint(_ status: String) -> Color {
        switch status {
        case "implemented": AeroricColor.success
        case "proposed": AeroricColor.accent
        case "rejected": AeroricColor.danger
        default: AeroricColor.textSecondary
        }
    }

    /// "数据真实性（R1-1）：部分成功、…" → the name, then what it covers.
    public static func splitTitle(_ title: String) -> (main: String, detail: String?) {
        guard let cut = title.firstIndex(where: { $0 == "：" || $0 == ":" }) else { return (title, nil) }
        let main = title[..<cut].trimmingCharacters(in: .whitespaces)
        let detail = title[title.index(after: cut)...].trimmingCharacters(in: .whitespaces)
        return main.isEmpty || detail.isEmpty ? (title, nil) : (main, detail)
    }

    public static func exportLine(_ plan: LiveWikiExportPlan) -> String {
        let total = "将写入 \(plan.files.count) 个文件"
        return plan.overwrites.isEmpty ? total : "\(total)，其中 \(plan.overwrites.count) 个会覆盖已有文件"
    }

    public static func kind(_ kind: String) -> String {
        switch kind {
        case "index": "索引"
        case "runbook": "操作手册"
        default: "决策"
        }
    }

    public static func symbol(_ kind: String) -> String {
        switch kind {
        case "index": "list.bullet.indent"
        case "runbook": "wrench.and.screwdriver"
        default: "checkmark.seal"
        }
    }

    public static func report(_ r: LiveWikiReport) -> String {
        var parts = ["\(r.pages) 页"]
        if r.added > 0 { parts.append("新增 \(r.added)") }
        if r.updated > 0 { parts.append("更新 \(r.updated)") }
        if r.removed > 0 { parts.append("删除 \(r.removed)") }
        if r.issues > 0 { parts.append("新 issue \(r.issues)") }
        if r.added + r.updated + r.removed == 0 { parts.append("没有变化") }
        return "已编译：" + parts.joined(separator: " · ")
    }

    public static func reason(_ reason: String) -> String {
        switch reason {
        case "edit": "你编辑"
        case "revert": "回到旧版"
        case "issue": "采纳建议"
        default: "编译"
        }
    }

    public static func issueKind(_ kind: String) -> String {
        switch kind {
        case "sourceChanged": "来源有更新"
        case "sourceRemoved": "来源已删除"
        default: "Agent 报告"
        }
    }
}
