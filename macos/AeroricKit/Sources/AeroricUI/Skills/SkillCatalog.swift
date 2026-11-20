import Foundation
import Observation
import SwiftUI
import AeroricProtocol

/// C7 (2026-11-20): the skill catalog (`skills.catalog`) and what each
/// launch of a session mounted (`skills.mounted`), for the 扩展 page and
/// 会话信息.
@MainActor
@Observable
public final class SkillCatalogBook {
    public typealias Call = @MainActor (String, [String: JSONValue]) async throws -> JSONValue

    public private(set) var skills: [LiveSkillDefinition] = []
    public private(set) var loading = false
    public var errorText: String?
    /// The project whose folders and learned skills count (nil: none).
    public var projectID: String?
    public var query = ""
    /// Each task's launches, oldest first, once read.
    public private(set) var mounts: [String: [LiveSkillLaunch]] = [:]
    public private(set) var mountErrors: [String: String] = [:]
    /// C8: each task's latest launch's tools, once read (nil inside: none kept).
    public private(set) var tools: [String: LiveToolLaunch?] = [:]
    /// H1: each task's latest launch's surfaces, from the same reply.
    public private(set) var surfaces: [String: LiveSurfaceLaunch?] = [:]

    @ObservationIgnored private var call: Call?

    public init() {}

    public func attach(_ call: Call?) {
        self.call = call
    }

    public func load() async {
        guard let call else { return }
        loading = true
        defer { loading = false }
        do {
            var params: [String: JSONValue] = [:]
            if let projectID { params["projectId"] = .string(projectID) }
            let reply = try await call("skills.catalog", params)
            skills = (reply["skills"]?.arrayValue ?? []).compactMap(LiveSkillDefinition.decode)
            errorText = nil
        } catch {
            errorText = "读取技能目录失败：\(AgentConfigStore.describe(error))"
        }
    }

    public func loadMounts(taskID: String) async {
        guard let call else { return }
        do {
            let reply = try await call("skills.mounted", ["taskId": .string(taskID)])
            mounts[taskID] = (reply["launches"]?.arrayValue ?? []).compactMap(LiveSkillLaunch.decode)
            mountErrors[taskID] = nil
        } catch {
            mountErrors[taskID] = "读取失败：\(AgentConfigStore.describe(error))"
        }
    }

    /// C8: the tools the task's latest launch had; H1: and its surfaces.
    public func loadTools(taskID: String, family: String) async {
        guard let call else { return }
        do {
            let reply = try await call("session.capabilities", ["family": .string(family), "taskId": .string(taskID)])
            tools[taskID] = .some(LiveToolLaunch.decode(reply["tools"]))
            surfaces[taskID] = .some(LiveSurfaceLaunch.decode(reply["surfaces"]))
        } catch {
            tools[taskID] = .some(nil)
            surfaces[taskID] = .some(nil)
        }
    }

    /// The catalog by source (内置 first), filtered by the search words.
    public var groups: [(source: String, skills: [LiveSkillDefinition])] {
        let words = query.lowercased().split(separator: " ").map(String.init)
        let shown = skills.filter { skill in
            words.allSatisfy { word in
                [skill.skillId, skill.name, skill.summary].contains { $0.lowercased().contains(word) }
            }
        }
        return SkillText.sources.compactMap { source in
            let these = shown.filter { $0.source == source }
            return these.isEmpty ? nil : (source, these)
        }
    }
}

/// C7: the words the catalog and 会话信息 use.
public enum SkillText {
    public static let sources = ["builtin", "learned", "user", "plugin"]

    public static func source(_ source: String) -> String {
        switch source {
        case "builtin": "内置"
        case "learned": "记忆沉淀"
        case "user": "你自己的目录"
        case "plugin": "Claude Code 插件"
        default: source
        }
    }

    public static func sourceHelp(_ source: String) -> String {
        switch source {
        case "builtin": "随应用内置，版本同应用；可在 Agent 配置 → 技能里关闭"
        case "learned": "由记忆里可信的经验沉淀、经你采纳的技能；在记忆页的技能提案里管理"
        case "user": "放在 Agent 自己的技能目录里，Agent 启动时自己读取；Aeroric 只读不改"
        case "plugin": "Claude Code 插件里的技能，由 Claude Code 自己加载，只给 Claude"
        default: ""
        }
    }

    public static func reason(_ reason: String) -> String {
        switch reason {
        case "default": "默认挂载"
        case "accepted": "你采纳的"
        case "userFolder": "你自己的目录"
        case "plugin": "Claude Code 插件"
        default: reason
        }
    }

    public static func harness(_ id: String) -> String {
        switch id {
        case "claude": "Claude"
        case "codex": "Codex"
        case "omp": "OMP"
        case "dsh": "DSH"
        default: id
        }
    }

    /// "随应用内置" / "v3" / "1.2.0 · a1b2c3"; a user's skill has only its hash.
    public static func version(_ version: String, hash: String) -> String {
        let short = String(hash.prefix(6))
        if version == "随应用内置" { return version }
        return [version, short].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// 会话信息's line: "31 个 · 9:41 启动" or why there are none.
    public static func launchSummary(_ launches: [LiveSkillLaunch]?) -> String {
        guard let launches else { return "读取中…" }
        guard let last = launches.last else { return "未记录（这个会话在记录技能之前启动）" }
        let time = Date(timeIntervalSince1970: Double(last.atMs) / 1000).formatted(date: .omitted, time: .shortened)
        var text = "\(last.skills.count) 个 · \(time) 启动"
        if launches.count > 1 { text += " · 共启动 \(launches.count) 次" }
        return text
    }

    /// One mounted skill's line: "brainstorming · 默认挂载 · 随应用内置".
    public static func mounted(_ skill: LiveMountedSkill) -> String {
        [skill.skillId, reason(skill.reason), version(skill.version, hash: skill.contentHash)]
            .filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// What changed between two launches of one session, by name.
    public static func changes(from old: LiveSkillLaunch, to new: LiveSkillLaunch) -> [String] {
        let before = Dictionary(old.skills.map { ($0.skillId, $0) }, uniquingKeysWith: { a, _ in a })
        let after = Dictionary(new.skills.map { ($0.skillId, $0) }, uniquingKeysWith: { a, _ in a })
        var out: [String] = []
        for id in after.keys.sorted() {
            guard let now = after[id] else { continue }
            if let was = before[id] {
                if was.contentHash != now.contentHash {
                    out.append("\(id) 换了版本（\(version(was.version, hash: was.contentHash)) → \(version(now.version, hash: now.contentHash))）")
                }
            } else {
                out.append("新增 \(id)")
            }
        }
        for id in before.keys.sorted() where after[id] == nil { out.append("去掉 \(id)") }
        return out
    }
}

/// C7: the 扩展 page — every skill an agent can be given, by source.
public struct SkillCatalogPage: View {
    @Bindable var book: SkillCatalogBook
    let projects: [(id: String, name: String)]
    @State private var openPath: String?

    public init(book: SkillCatalogBook, projects: [(id: String, name: String)]) {
        self.book = book
        self.projects = projects
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: AeroricSpace.m) {
                Picker("项目", selection: $book.projectID) {
                    Text("不看项目").tag(String?.none)
                    ForEach(projects, id: \.id) { project in
                        Text(project.name).tag(Optional(project.id))
                    }
                }
                .fixedSize()
                .help("选一个项目，把它自己的技能目录和项目专属的记忆技能也算进来")
                TextField("搜索技能", text: $book.query)
                    .textFieldStyle(.roundedBorder)
                    .frame(maxWidth: 240)
                Spacer()
                if book.loading { ProgressView().controlSize(.small) }
                Button("刷新") { Task { await book.load() } }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            }
            .padding(.horizontal, AeroricSpace.xl)
            .padding(.vertical, AeroricSpace.m)
            if let error = book.errorText {
                Text(error)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.danger)
                    .padding(.horizontal, AeroricSpace.xl)
            }
            List {
                ForEach(book.groups, id: \.source) { group in
                    Section {
                        ForEach(group.skills) { skill in row(skill) }
                    } header: {
                        Text("\(SkillText.source(group.source)) · \(group.skills.count)")
                            .help(SkillText.sourceHelp(group.source))
                    }
                }
            }
            .listStyle(.inset)
            .overlay {
                if book.groups.isEmpty && !book.loading {
                    AeroricEmptyState(systemImage: "puzzlepiece.extension",
                                      title: book.query.isEmpty ? "没有技能" : "没有匹配的技能",
                                      message: book.query.isEmpty ? "宿主没有返回任何技能。" : "换个词试试。")
                }
            }
        }
        .task(id: book.projectID) { await book.load() }
        .accessibilityIdentifier("skills.catalog")
    }

    private func row(_ skill: LiveSkillDefinition) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: AeroricSpace.s) {
                Text(skill.name)
                    .font(.system(size: AeroricType.secondary, weight: .medium))
                    .foregroundStyle(skill.active ? AeroricColor.text : AeroricColor.textTertiary)
                if skill.name != skill.skillId {
                    Text(skill.skillId)
                        .font(.system(size: AeroricType.caption).monospaced())
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                Spacer(minLength: AeroricSpace.s)
                CheckpointBadge(text: SkillText.version(skill.version, hash: skill.contentHash),
                                tint: skill.embedded ? AeroricColor.textSecondary : AeroricColor.accent)
                    .help(skill.embedded ? "随应用内置，版本同应用" : "内容哈希 \(skill.contentHash)")
            }
            if !skill.summary.isEmpty {
                Text(skill.summary)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(2)
            }
            HStack(spacing: AeroricSpace.s) {
                Text(skill.harnesses.map(SkillText.harness).joined(separator: "、"))
                if let note = skill.note {
                    Text(note).foregroundStyle(skill.active ? AeroricColor.textSecondary : AeroricColor.warning)
                }
                if let path = skill.path {
                    Text(path)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .foregroundStyle(AeroricColor.textTertiary)
                        .textSelection(.enabled)
                        .help(path)
                }
            }
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(AeroricColor.textTertiary)
        }
        .padding(.vertical, 2)
        .accessibilityIdentifier("skills.catalog.\(skill.source).\(skill.skillId)")
    }
}

/// C7: 会话信息's 技能 — what the session's launches were given.
struct SessionSkillsSection: View {
    let taskID: String
    @Environment(SkillCatalogBook.self) private var book: SkillCatalogBook?
    @State private var open = false

    var body: some View {
        if let book {
            let launches = book.mounts[taskID]
            VStack(alignment: .leading, spacing: 4) {
                Button { open.toggle() } label: {
                    HStack(spacing: AeroricSpace.s) {
                        Text("技能").foregroundStyle(AeroricColor.textSecondary)
                        Text(book.mountErrors[taskID] ?? SkillText.launchSummary(launches))
                            .foregroundStyle(AeroricColor.text)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        if launches?.last?.skills.isEmpty == false {
                            Image(systemName: open ? "chevron.up" : "chevron.down")
                                .font(.system(size: 9, weight: .semibold))
                                .foregroundStyle(AeroricColor.textTertiary)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("session.info.skills")
                if open, let launches, let last = launches.last {
                    if launches.count > 1 {
                        let changed = SkillText.changes(from: launches[launches.count - 2], to: last)
                        Text(changed.isEmpty ? "与上一次启动相同" : "与上一次启动相比：" + changed.joined(separator: "；"))
                            .foregroundStyle(AeroricColor.textSecondary)
                    }
                    ForEach(Array(last.skills.enumerated()), id: \.offset) { _, skill in
                        Text(SkillText.mounted(skill))
                            .foregroundStyle(AeroricColor.textSecondary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .help(SkillText.sourceHelp(skill.source))
                    }
                }
            }
            .font(.system(size: AeroricType.caption))
            .task(id: taskID) { await book.loadMounts(taskID: taskID) }
        }
    }
}

/// C8: the words 会话信息's 工具 uses.
public enum ToolText {
    public static func scope(_ scope: String) -> String {
        switch scope {
        case "readOnly": "只读"
        case "writes": "会写"
        case "sendsOut": "会对外发送"
        default: scope
        }
    }

    /// "Aeroric 工具 14/21 · Claude 自带 10 个" or why there are none.
    public static func summary(_ launch: LiveToolLaunch??) -> String {
        guard let known = launch else { return "读取中…" }
        guard let launch = known else { return "未记录（这个会话在记录工具之前启动）" }
        let own = launch.tools.filter { $0.owner == "harness" }.count
        let ownText = own > 0 ? " · \(SkillText.harness(launch.harness)) 自带 \(own) 个" : ""
        guard launch.injected else {
            return "没有 Aeroric 工具：\(launch.reason ?? "未接入")\(ownText)"
        }
        let aeroric = launch.tools.filter { $0.owner == "aeroric" }
        return "Aeroric 工具 \(aeroric.filter(\.ready).count)/\(aeroric.count)\(ownText)"
    }

    /// The tools not offered, by why: "memory_recall、memory_propose：这个项目没有开启记忆".
    public static func missing(_ launch: LiveToolLaunch) -> [String] {
        guard launch.injected else { return [] }
        var order: [String] = []
        var names: [String: [String]] = [:]
        for tool in launch.tools where tool.owner == "aeroric" && !tool.ready {
            if names[tool.basis] == nil { order.append(tool.basis) }
            names[tool.basis, default: []].append(tool.name)
        }
        return order.map { "\(names[$0]!.joined(separator: "、"))：\($0)" }
    }

    /// One offered tool's line: "delegate · 会写".
    public static func line(_ tool: LiveEffectiveTool) -> String {
        "\(tool.name) · \(scope(tool.scope))"
    }
}

/// C8: 会话信息's 工具 — what the session's latest launch could call.
struct SessionToolsSection: View {
    let taskID: String
    let family: String
    @Environment(SkillCatalogBook.self) private var book: SkillCatalogBook?
    @State private var open = false

    var body: some View {
        if let book {
            let launch = book.tools[taskID]
            VStack(alignment: .leading, spacing: 4) {
                Button { open.toggle() } label: {
                    HStack(spacing: AeroricSpace.s) {
                        Text("工具").foregroundStyle(AeroricColor.textSecondary)
                        Text(ToolText.summary(launch))
                            .foregroundStyle(launch??.injected == false ? AeroricColor.warning : AeroricColor.text)
                            .lineLimit(2)
                        Spacer(minLength: 0)
                        if launch??.tools.isEmpty == false {
                            Image(systemName: open ? "chevron.up" : "chevron.down")
                                .font(.system(size: 9, weight: .semibold))
                                .foregroundStyle(AeroricColor.textTertiary)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("session.info.tools")
                if open, let known = launch, let launch = known {
                    let aeroric = launch.tools.filter { $0.owner == "aeroric" && $0.ready }
                    if !aeroric.isEmpty {
                        Text("Aeroric 注入").foregroundStyle(AeroricColor.textTertiary)
                        ForEach(aeroric, id: \.name) { tool in
                            Text(ToolText.line(tool)).foregroundStyle(AeroricColor.textSecondary)
                                .help(tool.basis)
                        }
                    }
                    ForEach(ToolText.missing(launch), id: \.self) { line in
                        Text("没有 " + line).foregroundStyle(AeroricColor.textTertiary)
                    }
                    let own = launch.tools.filter { $0.owner == "harness" }
                    if !own.isEmpty {
                        Text("\(SkillText.harness(launch.harness)) 自带（已知的，Aeroric 不控制）")
                            .foregroundStyle(AeroricColor.textTertiary)
                        ForEach(own, id: \.name) { tool in
                            Text(ToolText.line(tool)).foregroundStyle(AeroricColor.textSecondary)
                                .help(tool.basis)
                        }
                    }
                }
            }
            .font(.system(size: AeroricType.caption))
            .task(id: taskID) { await book.loadTools(taskID: taskID, family: family) }
        }
    }
}

/// H1 (2026-11-20): the words 会话信息's 入口 uses.
public enum SurfaceText {
    public static func name(_ feature: String) -> String {
        switch feature {
        case "surfaceInstructions": "系统层"
        case "contextTools": "工具"
        case "surfaceSkills": "技能"
        case "surfaceEvents": "事件"
        case "surfaceSubagents": "子代理"
        case "surfaceCli": "命令行"
        default: feature
        }
    }

    public static func state(_ state: String) -> String {
        switch state {
        case "yes": "已接通"
        case "no": "没有"
        case "degraded": "有差异"
        case "unknown": "还没核对"
        default: state
        }
    }

    /// "接通 4/6：系统层、工具、技能、子代理" or why there is no record.
    public static func summary(_ launch: LiveSurfaceLaunch??) -> String {
        guard let known = launch else { return "读取中…" }
        guard let launch = known, !launch.surfaces.isEmpty else { return "未记录（这个会话在记录入口之前启动）" }
        let on = launch.surfaces.filter { $0.state == "yes" }
        let names = on.map { name($0.feature) }.joined(separator: "、")
        return "接通 \(on.count)/\(launch.surfaces.count)" + (names.isEmpty ? "" : "：\(names)")
    }

    /// One surface's line: "系统层 · 已接通 · 这次启动从系统层给了开场内容（312 字）".
    public static func line(_ entry: LiveCapabilityEntry) -> String {
        [name(entry.feature), state(entry.state), entry.note ?? ""].filter { !$0.isEmpty }.joined(separator: " · ")
    }
}

/// H1: 会话信息's 入口 — through which surfaces the session's latest
/// launch used Aeroric, and for those it did not, why.
struct SessionSurfacesSection: View {
    let taskID: String
    @Environment(SkillCatalogBook.self) private var book: SkillCatalogBook?
    @State private var open = false

    var body: some View {
        if let book {
            let launch = book.surfaces[taskID]
            VStack(alignment: .leading, spacing: 4) {
                Button { open.toggle() } label: {
                    HStack(spacing: AeroricSpace.s) {
                        Text("入口").foregroundStyle(AeroricColor.textSecondary)
                        Text(SurfaceText.summary(launch))
                            .foregroundStyle(AeroricColor.text)
                            .lineLimit(2)
                        Spacer(minLength: 0)
                        if launch??.surfaces.isEmpty == false {
                            Image(systemName: open ? "chevron.up" : "chevron.down")
                                .font(.system(size: 9, weight: .semibold))
                                .foregroundStyle(AeroricColor.textTertiary)
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .help("这个 Harness 通过哪些入口用到 Aeroric：系统层、工具、技能、事件、子代理、命令行")
                .accessibilityIdentifier("session.info.surfaces")
                if open, let known = launch, let launch = known {
                    ForEach(launch.surfaces, id: \.feature) { entry in
                        Text(SurfaceText.line(entry))
                            .foregroundStyle(entry.state == "yes" ? AeroricColor.textSecondary : AeroricColor.textTertiary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            .font(.system(size: AeroricType.caption))
            // Read with 工具 (`SessionToolsSection`): one `session.capabilities` reply carries both.
        }
    }
}
