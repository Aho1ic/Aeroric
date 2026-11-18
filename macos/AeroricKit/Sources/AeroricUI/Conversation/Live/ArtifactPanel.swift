import AeroricProtocol
import SwiftUI

/// B6 (2026-11-18): what the 交付物 section asks the host.
public protocol ArtifactActions: Sendable {
    func artifacts(taskId: String, cursor: String?) async throws -> LiveArtifactPage
    func artifactVersions(taskId: String, artifactId: String) async throws -> [LiveArtifactVersion]
    func readArtifact(taskId: String, artifactId: String, version: Int?, offset: UInt64,
                      limit: UInt64?) async throws -> LiveArtifactContent
    func excludeArtifact(taskId: String, artifactId: String, excluded: Bool) async throws
}

public enum ArtifactText {
    /// Rows shown before 显示全部.
    public static let shownAtFirst = 8
    /// The most a version view reads (the host's cap on one slice).
    public static let readLimit: UInt64 = 256 * 1024
    /// Unchanged lines longer than this fold in a comparison.
    public static let foldSame = 6

    public static func header(_ count: Int) -> String {
        count == 0 ? "交付物" : "交付物 \(count)"
    }

    /// What an artifact's row says about it.
    public static func state(_ artifact: LiveArtifact) -> String {
        if artifact.removed { return "已删除" }
        guard let current = artifact.current else { return "已撤回" }
        return checks(current)
    }

    /// The checks a version went through, by the last one.
    public static func checks(_ version: LiveArtifactVersion) -> String {
        if version.withdrawn { return "已撤回" }
        guard let last = version.checks.last else { return "未检查" }
        if !last.passed { return "检查未过" }
        return last.incomplete ? "检查通过（可能不完整）" : "检查通过"
    }

    public static func tone(_ text: String) -> Color {
        switch text {
        case "检查通过": AeroricColor.success
        case "检查未过": AeroricColor.danger
        case "检查通过（可能不完整）": AeroricColor.warning
        default: AeroricColor.textTertiary
        }
    }

    /// e.g. `第 2 版 · 11月18日 14:05 · 检查未过（测试 1 项失败）`.
    public static func versionLine(_ version: LiveArtifactVersion) -> String {
        var parts = ["第 \(version.version) 版"]
        if version.atMs > 0 {
            parts.append(Date(timeIntervalSince1970: Double(version.atMs) / 1000)
                .formatted(.dateTime.month().day().hour().minute()))
        }
        var verdict = checks(version)
        if !version.withdrawn, let last = version.checks.last, !last.passed, !last.summary.isEmpty {
            verdict += "（\(last.summary)）"
        }
        parts.append(verdict)
        if version.superseded { parts.append("已有新版") }
        return parts.joined(separator: " · ")
    }

    /// Every check of a version, one per line, for its tooltip.
    public static func checkHelp(_ version: LiveArtifactVersion) -> String {
        if version.checks.isEmpty { return "这一版没有经过检查" }
        return version.checks.map { check in
            var line = check.passed ? "通过" : "未通过"
            if !check.summary.isEmpty { line += "：\(check.summary)" }
            if check.incomplete { line += "（结果可能不完整）" }
            return "回合 \(check.turnId.prefix(8)) 的检查 · \(line)"
        }.joined(separator: "\n")
    }

    public static func symbol(_ kind: String) -> String {
        switch kind {
        case "code": "chevron.left.forwardslash.chevron.right"
        case "document": "doc.text"
        case "data": "tablecells"
        case "image": "photo"
        default: "doc"
        }
    }

    public static func size(_ bytes: UInt64) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    }

    public static func dropped(_ count: Int) -> String {
        "只列了最近改过的交付物，更早的 \(count) 个已不再保留"
    }

    public enum LineKind: Equatable, Sendable { case same, added, removed, folded }

    public struct Line: Equatable, Sendable, Identifiable {
        public var id: Int
        public var kind: LineKind
        public var text: String
    }

    /// Line by line, `older` against `newer`; long unchanged runs fold.
    public static func compare(_ older: String, _ newer: String) -> [Line] {
        let old = older.components(separatedBy: "\n")
        let new = newer.components(separatedBy: "\n")
        let difference = new.difference(from: old)
        var removed = Set<Int>()
        var inserted = Set<Int>()
        for change in difference {
            switch change {
            case let .remove(offset, _, _): removed.insert(offset)
            case let .insert(offset, _, _): inserted.insert(offset)
            }
        }
        var raw: [(LineKind, String)] = []
        var i = 0
        var j = 0
        while i < old.count || j < new.count {
            if i < old.count, removed.contains(i) {
                raw.append((.removed, old[i]))
                i += 1
            } else if j < new.count, inserted.contains(j) {
                raw.append((.added, new[j]))
                j += 1
            } else {
                raw.append((.same, i < old.count ? old[i] : new[j]))
                i += 1
                j += 1
            }
        }
        var lines: [Line] = []
        var run: [String] = []
        func flush() {
            if run.count > foldSame * 2 {
                for text in run.prefix(foldSame / 2) { lines.append(Line(id: lines.count, kind: .same, text: text)) }
                lines.append(Line(id: lines.count, kind: .folded,
                                  text: "… 省略 \(run.count - foldSame / 2 * 2) 行相同内容 …"))
                for text in run.suffix(foldSame / 2) { lines.append(Line(id: lines.count, kind: .same, text: text)) }
            } else {
                for text in run { lines.append(Line(id: lines.count, kind: .same, text: text)) }
            }
            run.removeAll()
        }
        for (kind, text) in raw {
            if kind == .same {
                run.append(text)
            } else {
                flush()
                lines.append(Line(id: lines.count, kind: kind, text: text))
            }
        }
        flush()
        return lines
    }
}

/// What the version sheet shows.
public struct ArtifactViewing: Identifiable, Equatable, Sendable {
    public enum Body: Equatable, Sendable {
        case text(String)
        case lines([ArtifactText.Line])
        /// Binary, unreadable, or gone: the reason.
        case note(String)
    }

    public var id: String
    public var title: String
    public var body: Body
    /// e.g. "只显示了前 256 KB".
    public var footnote: String?
}

/// The 交付物 section's state for one task.
@MainActor @Observable
public final class ArtifactStore {
    public let taskID: String
    @ObservationIgnored private let actions: ArtifactActions
    public private(set) var artifacts: [LiveArtifact] = []
    public private(set) var nextCursor: String?
    public private(set) var dropped = 0
    public private(set) var unavailable: String?
    public private(set) var versions: [String: [LiveArtifactVersion]] = [:]
    public var expanded: Set<String> = []
    public var showsAll = false
    public var showsExcluded = false
    public var viewing: ArtifactViewing?
    public var errorText: String?

    public init(taskID: String, actions: ArtifactActions) {
        self.taskID = taskID
        self.actions = actions
    }

    public var counted: [LiveArtifact] { artifacts.filter { !$0.excluded } }
    public var excluded: [LiveArtifact] { artifacts.filter(\.excluded) }

    /// Re-read the first page (and the versions of the open rows).
    public func load() async {
        do {
            let page = try await actions.artifacts(taskId: taskID, cursor: nil)
            artifacts = page.artifacts
            nextCursor = page.nextCursor
            dropped = page.dropped
            unavailable = page.unavailable
            errorText = nil
            for id in expanded { await loadVersions(id) }
        } catch {
            errorText = "读取交付物失败：\(error)"
        }
    }

    public func loadMore() async {
        guard let cursor = nextCursor else { return }
        do {
            let page = try await actions.artifacts(taskId: taskID, cursor: cursor)
            let known = Set(artifacts.map(\.id))
            artifacts += page.artifacts.filter { !known.contains($0.id) }
            nextCursor = page.nextCursor
        } catch {
            errorText = "读取交付物失败：\(error)"
        }
    }

    public func toggle(_ id: String) async {
        if expanded.remove(id) != nil { return }
        expanded.insert(id)
        await loadVersions(id)
    }

    public func setExcluded(_ id: String, _ excluded: Bool) async {
        do {
            try await actions.excludeArtifact(taskId: taskID, artifactId: id, excluded: excluded)
            if let i = artifacts.firstIndex(where: { $0.id == id }) { artifacts[i].excluded = excluded }
        } catch {
            errorText = "操作失败：\(error)"
        }
    }

    /// Show one version (nil: the one that stands).
    public func open(_ artifact: LiveArtifact, version: Int?) async {
        let label = version.map { "第 \($0) 版" } ?? artifact.current.map { "第 \($0.version) 版" } ?? "当前版本"
        let title = "\(artifact.name) · \(label)"
        do {
            let read = try await actions.readArtifact(taskId: taskID, artifactId: artifact.id, version: version,
                                                      offset: 0, limit: ArtifactText.readLimit)
            guard let text = read.text else {
                viewing = ArtifactViewing(id: "\(artifact.id)@\(read.version)", title: title,
                                          body: .note("这是二进制文件（\(ArtifactText.size(read.size))），这里不显示它的内容。"))
                return
            }
            viewing = ArtifactViewing(id: "\(artifact.id)@\(read.version)", title: title, body: .text(text),
                                      footnote: cut(read))
        } catch {
            viewing = ArtifactViewing(id: "\(artifact.id)@\(version ?? 0)", title: title, body: .note("\(error)"))
        }
    }

    /// `older` against `newer`, line by line.
    public func compare(_ artifact: LiveArtifact, older: Int, newer: Int) async {
        let title = "\(artifact.name) · 第 \(older) 版 → 第 \(newer) 版"
        let id = "\(artifact.id)@\(older)-\(newer)"
        do {
            let a = try await actions.readArtifact(taskId: taskID, artifactId: artifact.id, version: older,
                                                   offset: 0, limit: ArtifactText.readLimit)
            let b = try await actions.readArtifact(taskId: taskID, artifactId: artifact.id, version: newer,
                                                   offset: 0, limit: ArtifactText.readLimit)
            guard let old = a.text, let new = b.text else {
                viewing = ArtifactViewing(id: id, title: title, body: .note("二进制文件不能逐行比较。"))
                return
            }
            viewing = ArtifactViewing(id: id, title: title, body: .lines(ArtifactText.compare(old, new)),
                                      footnote: cut(a) ?? cut(b))
        } catch {
            viewing = ArtifactViewing(id: id, title: title, body: .note("\(error)"))
        }
    }

    private func cut(_ read: LiveArtifactContent) -> String? {
        read.nextOffset == nil ? nil : "只显示了前 \(ArtifactText.size(read.offset + UInt64(read.content.utf8.count)))，"
            + "整个文件 \(ArtifactText.size(read.size))。"
    }

    private func loadVersions(_ id: String) async {
        do {
            versions[id] = try await actions.artifactVersions(taskId: taskID, artifactId: id)
        } catch {
            errorText = "读取版本失败：\(error)"
        }
    }
}

/// The 进度 page's 交付物 section.
struct ArtifactPanelSection: View {
    @Bindable var store: ArtifactStore

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            CheckpointSectionLabel(title: ArtifactText.header(store.counted.count))
            if let errorText = store.errorText {
                note(errorText, tint: AeroricColor.danger)
            }
            if let unavailable = store.unavailable {
                note(unavailable)
            } else if store.artifacts.isEmpty {
                note("改过文件的回合结束后，这里会列出交付的文件和各自的版本。")
            }
            let counted = store.counted
            let shown = store.showsAll ? counted : Array(counted.prefix(ArtifactText.shownAtFirst))
            ForEach(shown) { artifact in row(artifact) }
            if counted.count > ArtifactText.shownAtFirst || store.nextCursor != nil {
                Button(store.showsAll ? "收起" : "显示全部 \(counted.count)\(store.nextCursor == nil ? "" : "+") 个") {
                    store.showsAll.toggle()
                    if store.showsAll { Task { await store.loadMore() } }
                }
                .buttonStyle(.plain)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityIdentifier("progress.artifacts.all")
            }
            if !store.excluded.isEmpty {
                Button(store.showsExcluded ? "隐藏不算交付物的文件" : "另有 \(store.excluded.count) 个不算交付物") {
                    store.showsExcluded.toggle()
                }
                .buttonStyle(.plain)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                if store.showsExcluded {
                    ForEach(store.excluded) { artifact in row(artifact).opacity(0.6) }
                }
            }
            if store.dropped > 0 {
                note(ArtifactText.dropped(store.dropped))
            }
        }
        .monospacedDigit()
        .sheet(item: $store.viewing) { viewing in
            ArtifactVersionSheet(viewing: viewing) { store.viewing = nil }
        }
    }

    @ViewBuilder private func row(_ artifact: LiveArtifact) -> some View {
        let state = ArtifactText.state(artifact)
        Button { Task { await store.toggle(artifact.id) } } label: {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: ArtifactText.symbol(artifact.kind))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .frame(width: 14)
                Text(artifact.name)
                    .font(.system(size: AeroricType.secondary))
                    .lineLimit(1)
                    .layoutPriority(1)
                Text(artifact.folder)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.head)
                Spacer(minLength: AeroricSpace.s)
                Text("第 \(artifact.current?.version ?? artifact.latestVersion) 版")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .fixedSize()
                Text(state)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(ArtifactText.tone(state))
                    .fixedSize()
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(artifact.path)
        .contextMenu {
            Button("打开当前版本") { Task { await store.open(artifact, version: nil) } }
                .disabled(artifact.current == nil)
            Button(artifact.excluded ? "算作交付物" : "不算交付物") {
                Task { await store.setExcluded(artifact.id, !artifact.excluded) }
            }
        }
        .accessibilityIdentifier("progress.artifact.\(artifact.path)")
        if store.expanded.contains(artifact.id) {
            let versions = store.versions[artifact.id] ?? []
            ForEach(Array(versions.enumerated()), id: \.element.id) { index, version in
                let older = versions.dropFirst(index + 1).first
                versionRow(artifact, version, older: older)
            }
        }
    }

    private func versionRow(_ artifact: LiveArtifact, _ version: LiveArtifactVersion,
                            older: LiveArtifactVersion?) -> some View {
        HStack(spacing: AeroricSpace.s) {
            Text(ArtifactText.versionLine(version))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(version.withdrawn ? AeroricColor.textTertiary : AeroricColor.textSecondary)
                .lineLimit(1)
                .truncationMode(.tail)
                .help(ArtifactText.checkHelp(version))
            Spacer(minLength: AeroricSpace.s)
            Button("打开") { Task { await store.open(artifact, version: version.version) } }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .fixedSize()
            if let older {
                Button("比较") { Task { await store.compare(artifact, older: older.version, newer: version.version) } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                    .help("和第 \(older.version) 版逐行比较")
            }
        }
        .padding(.leading, 22)
    }

    private func note(_ text: String, tint: Color = AeroricColor.textSecondary) -> some View {
        Text(text)
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(tint)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// One version's content, or two versions line by line.
struct ArtifactVersionSheet: View {
    let viewing: ArtifactViewing
    let onClose: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack {
                Text(viewing.title)
                    .font(.system(size: AeroricType.body, weight: .semibold))
                    .lineLimit(1)
                Spacer()
                Button("关闭", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .keyboardShortcut(.cancelAction)
            }
            ScrollView([.vertical, .horizontal]) {
                content
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
            }
            if let footnote = viewing.footnote {
                Text(footnote)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
        }
        .padding(AeroricSpace.l)
        .frame(minWidth: 620, minHeight: 440)
    }

    @ViewBuilder private var content: some View {
        switch viewing.body {
        case let .text(text):
            Text(text)
                .font(.system(size: AeroricType.code, design: .monospaced))
        case let .lines(lines):
            VStack(alignment: .leading, spacing: 0) {
                ForEach(lines) { line in
                    Text(prefix(line.kind) + line.text)
                        .font(.system(size: AeroricType.code, design: .monospaced))
                        .foregroundStyle(color(line.kind))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(background(line.kind))
                }
            }
        case let .note(text):
            Text(text)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
        }
    }

    private func prefix(_ kind: ArtifactText.LineKind) -> String {
        switch kind {
        case .added: "+ "
        case .removed: "- "
        case .same: "  "
        case .folded: ""
        }
    }

    private func color(_ kind: ArtifactText.LineKind) -> Color {
        switch kind {
        case .folded: AeroricColor.textTertiary
        default: AeroricColor.text
        }
    }

    private func background(_ kind: ArtifactText.LineKind) -> Color {
        switch kind {
        case .added: AeroricColor.success.opacity(0.12)
        case .removed: AeroricColor.danger.opacity(0.12)
        default: .clear
        }
    }
}
