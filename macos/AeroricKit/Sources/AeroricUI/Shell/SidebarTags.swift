import AppKit
import AeroricClient
import Observation
import SwiftUI

/// Project and session tags (2026-10-29, T1–T4). The library lives on the
/// host (`tags.*`); this book mirrors it for the sidebar and applies a change
/// locally first so a tick in a menu shows at once.
@MainActor @Observable
public final class TagBook {
    public private(set) var library = TagLibrary()
    public var errorText: String?
    @ObservationIgnored private var client: WorkspaceClient?

    public init() {}

    public func attach(client: WorkspaceClient?) {
        self.client = client
        Task { await reload() }
    }

    public var tags: [TagRecord] { library.tags }

    public func reload() async {
        guard let client else { return }
        do { library = try await client.tagLibrary() } catch { errorText = "读取标签失败：\(error)" }
    }

    /// A row's tags in library order.
    public func tags(on row: String) -> [TagRecord] {
        Self.tags(on: row, in: library)
    }

    /// What a row may carry: 所有会话 tags, plus its own project's (a project
    /// row's own project is itself; Q1-a).
    public func available(isProject: Bool, projectID: String?) -> [TagRecord] {
        Self.available(library.tags, isProject: isProject, projectID: projectID)
    }

    public func toggle(_ tag: TagRecord, on row: String, isProject: Bool, projectID: String?) {
        var ids = library.assignments[row] ?? []
        if let index = ids.firstIndex(of: tag.id) { ids.remove(at: index) } else { ids.append(tag.id) }
        set(ids, on: row, isProject: isProject, projectID: projectID)
    }

    private func set(_ ids: [String], on row: String, isProject: Bool, projectID: String?) {
        library.assignments[row] = ids.isEmpty ? nil : ids
        guard let client else { return }
        Task {
            do {
                try await client.assignTags(target: row, isProject: isProject, projectId: projectID, tagIds: ids)
            } catch {
                errorText = "设置标签失败：\(error)"
            }
            await reload()
        }
    }

    /// Creates a tag and, when `row` is given, puts it on that row.
    public func create(name: String, color: String, projectID: String?,
                       on row: (id: String, isProject: Bool, projectID: String?)?) async -> String? {
        guard let client else { return "没有连接宿主" }
        do {
            let tag = try await client.createTag(name: name, color: color, projectId: projectID)
            library.tags.append(tag)
            if let row {
                let ids = (library.assignments[row.id] ?? []) + [tag.id]
                set(ids, on: row.id, isProject: row.isProject, projectID: row.projectID)
            }
            return nil
        } catch let ConnectionError.refused(failure) {
            return failure.error.message
        } catch {
            return "\(error)"
        }
    }

    /// How many rows lose `tag` if its scope becomes `scope` (Q2): narrowing
    /// to a project drops it from rows in any other project or in none.
    public func impact(of tag: TagRecord, scope: String?) -> Int {
        Self.impact(of: tag.id, scope: scope, in: library)
    }

    public func update(_ tag: TagRecord, name: String? = nil, color: String? = nil,
                       scope: String?? = nil) async -> String? {
        guard let client else { return "没有连接宿主" }
        do {
            try await client.updateTag(id: tag.id, name: name, color: color, scope: scope)
            await reload()
            return nil
        } catch let ConnectionError.refused(failure) {
            return failure.error.message
        } catch {
            return "\(error)"
        }
    }

    public func delete(_ tag: TagRecord) async {
        guard let client else { return }
        do { try await client.deleteTag(id: tag.id) } catch { errorText = "删除标签失败：\(error)" }
        await reload()
    }

    // MARK: Pure (tested)

    nonisolated static func tags(on row: String, in library: TagLibrary) -> [TagRecord] {
        let ids = library.assignments[row] ?? []
        return library.tags.filter { ids.contains($0.id) }
    }

    nonisolated static func available(_ tags: [TagRecord], isProject: Bool, projectID: String?) -> [TagRecord] {
        tags.filter { tag in
            guard let scope = tag.projectId else { return true }
            return scope == projectID
        }
    }

    nonisolated static func impact(of tagID: String, scope: String?, in library: TagLibrary) -> Int {
        guard let scope else { return 0 }
        return library.assignments.filter { row, ids in
            ids.contains(tagID) && library.rowProjects[row] != scope
        }.count
    }

    /// Filtering by `tag` (Q2-a): a project carrying it shows with all its
    /// sessions; otherwise only its sessions carrying it, and the project
    /// shows only when one does.
    nonisolated static func filter(project: String, sessions: [String], by tag: String,
                                   in library: TagLibrary) -> (showsProject: Bool, sessions: [String]) {
        if library.assignments[project]?.contains(tag) == true { return (true, sessions) }
        let tagged = sessions.filter { library.assignments[$0]?.contains(tag) == true }
        return (!tagged.isEmpty, tagged)
    }
}

/// Up to two tag capsules, then `+N` (Q3). Titles truncate before tags drop.
struct TagChips: View {
    let tags: [TagRecord]
    var limit = 2

    var body: some View {
        if !tags.isEmpty {
            HStack(spacing: 3) {
                ForEach(tags.prefix(limit)) { tag in TagChip(tag: tag) }
                if tags.count > limit {
                    Text("+\(tags.count - limit)")
                        .font(.system(size: AeroricType.caption - 1).monospacedDigit())
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
            .fixedSize()
            .layoutPriority(1)
            .accessibilityElement(children: .combine)
            .accessibilityLabel("标签：\(tags.map(\.name).joined(separator: "、"))")
        }
    }
}

struct TagChip: View {
    let tag: TagRecord

    var body: some View {
        let tint = ProjectColor.color(tag.color) ?? AeroricColor.accent
        Text(tag.name)
            .font(.system(size: AeroricType.caption - 1, weight: .medium))
            .lineLimit(1)
            .foregroundStyle(tint)
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .background(tint.opacity(0.16), in: Capsule(style: .continuous))
    }
}

/// A coloured dot for menus. Menu images are drawn as templates (one grey)
/// unless they say otherwise, which is why the first version's tinted
/// circles all looked alike (2026-10-29).
enum TagDot {
    static func image(_ color: String) -> NSImage {
        let fill = NSColor(ProjectColor.color(color) ?? AeroricColor.accent)
        let image = NSImage(size: NSSize(width: 10, height: 10), flipped: false) { rect in
            fill.setFill()
            NSBezierPath(ovalIn: rect.insetBy(dx: 0.5, dy: 0.5)).fill()
            return true
        }
        image.isTemplate = false
        return image
    }
}

/// Where a new tag will go: its default scope and the row to put it on.
struct TagDraftTarget: Identifiable {
    let id = UUID()
    /// The project a 仅当前项目 tag would belong to; nil when there is none.
    let projectID: String?
    let projectTitle: String?
    let row: (id: String, isProject: Bool, projectID: String?)?
    /// Opened from 管理标签 rather than from a row.
    var fromManager = false
}

/// 新建标签 (2026-10-29 redesign): live preview, name, colour, scope — the
/// scope choice is always there (Q3-a: greyed when there is no project).
struct TagEditorSheet: View {
    let target: TagDraftTarget
    let book: TagBook
    let onClose: () -> Void
    @State private var name = ""
    @State private var color = ProjectColor.all[5].id
    @State private var scope = "all"
    @State private var error: String?
    @State private var saving = false

    private var preview: TagRecord {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        return TagRecord(id: "preview", name: trimmed.isEmpty ? "标签" : trimmed, color: color)
    }

    private var scopeNote: String {
        if scope == "project", let title = target.projectTitle {
            return "只有「\(title)」和它的会话能贴这个标签。"
        }
        if target.projectID == nil {
            return target.fromManager
                ? "所有项目和会话都能贴。建好后可在列表里把范围改为某个项目。"
                : "这个会话不属于任何项目，只能建「所有会话」的标签。"
        }
        return "所有项目和会话都能贴这个标签。"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("新建标签").font(.system(size: AeroricType.title, weight: .semibold))
                Spacer()
                TagChip(tag: preview).animation(.easeOut(duration: 0.15), value: color)
            }
            section("名称") {
                CapsuleField(placeholder: "例如 重要、部署、调研", text: $name)
                    .accessibilityIdentifier("tags.editor.name")
            }
            section("颜色") { ColorSwatches(selection: $color) }
            section("可用范围") {
                VStack(alignment: .leading, spacing: AeroricSpace.s) {
                    CapsuleSlider(selection: $scope, groupTitle: "可用范围", identifier: "tags.editor.scope",
                                  segments: [
                                      .init(value: "all", title: "所有会话"),
                                      .init(value: "project", title: "仅 \(target.projectTitle ?? "当前项目")",
                                            isEnabled: target.projectID != nil,
                                            help: target.projectID == nil ? "这个会话不属于任何项目" : nil),
                                  ])
                        .fixedSize()
                    Text(scopeNote)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            if let error {
                Text(error)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.danger)
                    .padding(.top, AeroricSpace.m)
            }
            HStack(spacing: AeroricSpace.m) {
                Spacer()
                Button("取消", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                    .keyboardShortcut(.cancelAction)
                    .fixedSize()
                Button(target.row == nil ? "创建" : "创建并贴上") { save() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .keyboardShortcut(.defaultAction)
                    .disabled(saving || name.trimmingCharacters(in: .whitespaces).isEmpty)
                    .fixedSize()
            }
            .padding(.top, AeroricSpace.xxl)
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 380)
    }

    private func section(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text(title)
                .font(.system(size: AeroricType.caption, weight: .medium))
                .foregroundStyle(AeroricColor.textTertiary)
            content()
        }
        .padding(.top, AeroricSpace.l)
    }

    private func save() {
        saving = true
        let projectID = scope == "project" ? target.projectID : nil
        Task {
            if let message = await book.create(name: name, color: color, projectID: projectID, on: target.row) {
                error = message
                saving = false
            } else {
                onClose()
            }
        }
    }
}

struct ColorSwatches: View {
    @Binding var selection: String

    var body: some View {
        HStack(spacing: AeroricSpace.m + 2) {
            ForEach(ProjectColor.all) { color in
                Button { withAnimation(.easeOut(duration: 0.15)) { selection = color.id } } label: {
                    Circle().fill(color.color)
                        .frame(width: 20, height: 20)
                        .overlay {
                            if selection == color.id {
                                Circle().strokeBorder(AeroricColor.text, lineWidth: 2).padding(-4)
                            }
                        }
                        .frame(width: 28, height: 28)
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .help(color.title)
                .accessibilityLabel(color.title)
                .accessibilityAddTraits(selection == color.id ? .isSelected : [])
            }
        }
    }
}

/// 管理标签 (2026-10-29 redesign): one row per tag — colour dot, preview or
/// rename field, scope menu, edit and delete. Narrowing a scope says first
/// how many rows will lose the tag (Q2).
struct TagManagerSheet: View {
    let book: TagBook
    let projects: [(id: String, title: String)]
    let onClose: () -> Void
    @State private var renaming: String?
    @State private var renameText = ""
    @State private var deleting: TagRecord?
    @State private var narrowing: (tag: TagRecord, scope: String, count: Int)?
    @State private var creating: TagDraftTarget?
    @State private var hovered: String?
    @State private var error: String?

    private func title(of project: String?) -> String {
        guard let project else { return "所有会话" }
        return "仅 \(projects.first { $0.id == project }?.title ?? "某项目")"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            HStack {
                Text("管理标签").font(.system(size: AeroricType.title, weight: .semibold))
                Spacer()
                Button {
                    creating = TagDraftTarget(projectID: nil, projectTitle: nil, row: nil, fromManager: true)
                } label: {
                    Label("新建", systemImage: "plus")
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .fixedSize()
                .accessibilityIdentifier("tags.manager.new")
            }
            if book.tags.isEmpty {
                Text("还没有标签。点「新建」，或在会话、项目的菜单里选「设置标签 › 新建标签…」。")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .padding(.vertical, AeroricSpace.l)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(book.tags) { tag in row(tag) }
                    }
                }
                .frame(maxHeight: 340)
            }
            Text("点圆点改颜色，点铅笔改名，回车保存。范围改为「仅某项目」时，其他地方的这个标签会被摘掉，会先确认。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            if let error {
                Text(error).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.danger)
            }
            HStack {
                Spacer()
                Button("完成", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .keyboardShortcut(.defaultAction)
                    .fixedSize()
            }
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 480)
        .sheet(item: $creating) { target in
            TagEditorSheet(target: target, book: book, onClose: { creating = nil })
        }
        .confirmationDialog("删除这个标签？", isPresented: Binding(
            get: { deleting != nil }, set: { if !$0 { deleting = nil } }
        ), presenting: deleting) { tag in
            Button("删除", role: .destructive) { Task { await book.delete(tag) } }
            Button("取消", role: .cancel) {}
        } message: { tag in
            Text("「\(tag.name)」会从所有项目和会话上摘掉。项目和会话本身不受影响。")
        }
        .confirmationDialog("缩小这个标签的范围？", isPresented: Binding(
            get: { narrowing != nil }, set: { if !$0 { narrowing = nil } }
        ), presenting: narrowing.map { NarrowingPrompt(tag: $0.tag, scope: $0.scope, count: $0.count) }) { prompt in
            Button("改为\(title(of: prompt.scope))", role: .destructive) {
                Task { error = await book.update(prompt.tag, scope: .some(prompt.scope)) }
            }
            Button("取消", role: .cancel) {}
        } message: { prompt in
            Text("「\(prompt.tag.name)」会从 \(prompt.count) 个其他地方的项目或会话上摘掉。")
        }
    }

    @ViewBuilder private func row(_ tag: TagRecord) -> some View {
        HStack(spacing: AeroricSpace.m + 2) {
            Menu {
                ForEach(ProjectColor.all) { color in
                    Button {
                        Task { error = await book.update(tag, color: color.id) }
                    } label: {
                        Label { Text(color.title) } icon: { Image(nsImage: TagDot.image(color.id)) }
                    }
                }
            } label: {
                Circle().fill(ProjectColor.color(tag.color) ?? AeroricColor.accent).frame(width: 14, height: 14)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("修改颜色")
            if renaming == tag.id {
                CapsuleField(placeholder: "标签名", text: $renameText, height: 28)
                    .onSubmit { commitRename(tag) }
                    .frame(maxWidth: .infinity)
            } else {
                TagChip(tag: tag)
                Spacer(minLength: AeroricSpace.s)
            }
            Menu {
                Button("所有会话") { change(tag, to: nil) }
                if !projects.isEmpty { Divider() }
                ForEach(projects, id: \.id) { project in
                    Button("仅 \(project.title)") { change(tag, to: project.id) }
                }
            } label: {
                HStack(spacing: 3) {
                    Text(title(of: tag.projectId)).lineLimit(1)
                    Image(systemName: "chevron.down").font(.system(size: 8, weight: .semibold))
                }
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .padding(.horizontal, AeroricSpace.m)
                .frame(height: 22)
                .background(AeroricColor.layer2, in: Capsule(style: .continuous))
                .fixedSize()
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("可用范围")
            if renaming == tag.id {
                RoundIconButton(systemImage: "checkmark", tint: AeroricColor.success, help: "保存") { commitRename(tag) }
                RoundIconButton(systemImage: "xmark", tint: AeroricColor.textSecondary, help: "取消") { renaming = nil }
            } else {
                RoundIconButton(systemImage: "pencil", tint: AeroricColor.textSecondary, help: "重命名") {
                    renaming = tag.id
                    renameText = tag.name
                }
                RoundIconButton(systemImage: "trash", tint: AeroricColor.danger, help: "删除") { deleting = tag }
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(minHeight: 40)
        // Hover is a capsule, like every row highlight (owner preference).
        .background(hovered == tag.id ? AeroricColor.hover : .clear, in: Capsule(style: .continuous))
        .onHover { inside in hovered = inside ? tag.id : (hovered == tag.id ? nil : hovered) }
    }

    private func change(_ tag: TagRecord, to scope: String?) {
        guard scope != tag.projectId else { return }
        let count = book.impact(of: tag, scope: scope)
        if count > 0, let scope {
            narrowing = (tag, scope, count)
        } else {
            Task { error = await book.update(tag, scope: .some(scope)) }
        }
    }

    private func commitRename(_ tag: TagRecord) {
        let name = renameText
        Task {
            error = await book.update(tag, name: name)
            if error == nil { renaming = nil }
        }
    }
}

private struct NarrowingPrompt {
    let tag: TagRecord
    let scope: String
    let count: Int
}

/// A small round icon button with a hover fill.
struct RoundIconButton: View {
    let systemImage: String
    let tint: Color
    let help: String
    let action: () -> Void
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(tint)
                .frame(width: 26, height: 26)
                .background(hovered ? AeroricColor.layer3 : .clear, in: Circle())
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help(help)
        .accessibilityLabel(help)
    }
}
