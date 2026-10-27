import AeroricClient
import SwiftUI

/// 设置 → 导入 → 选择会话 (2026-10-27): every Claude Code / Codex session on
/// disk, one card per working directory. Everything starts ticked except what
/// the user left out last time; a project's tick clears or fills all of its
/// sessions; 导入 adds the missing projects and imports the ticked sessions.
struct ImportPickerSheet: View {
    @Bindable var book: ImportBook
    @Environment(\.dismiss) private var dismiss
    /// Collapsed project cards (all open by default).
    @State private var collapsed: Set<String> = []

    private var pick: Binding<ImportPick> {
        Binding(get: { book.pick ?? ImportPick(rows: []) }, set: { book.pick = $0 })
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            header
            if book.pick != nil {
                toolbar
                list
            } else {
                VStack(spacing: AeroricSpace.m) {
                    ProgressView().controlSize(.small)
                    Text("正在读取会话…")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            footer
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 620, height: 660)
        .task { if book.pick == nil { await book.openPicker() } }
        .accessibilityIdentifier("imports.picker")
    }

    private var header: some View {
        HStack(alignment: .top, spacing: AeroricSpace.l) {
            Image(systemName: "tray.and.arrow.down.fill")
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(AeroricColor.accent)
                .frame(width: 38, height: 38)
                .background(AeroricColor.accent.opacity(0.14), in: Circle())
            VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                Text("导入已有会话").font(.system(size: AeroricType.title, weight: .semibold))
                Text(subtitle)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var subtitle: String {
        guard let value = book.pick else { return "读取 Claude Code 与 Codex 的会话目录，只读不改。" }
        let groups = value.groups
        return "在 \(groups.count) 个目录里找到 \(value.rows.count) 个会话，已按项目整理。新目录会自动添加为项目；原文件只读不改。"
    }

    private var toolbar: some View {
        let value = pick.wrappedValue
        return HStack(spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: "magnifyingglass").foregroundStyle(AeroricColor.textTertiary)
                TextField("搜索项目或会话", text: pick.query)
                    .textFieldStyle(.plain)
                    .accessibilityIdentifier("imports.picker.search")
                if !value.query.isEmpty {
                    Button { pick.wrappedValue.query = "" } label: { Image(systemName: "xmark.circle.fill") }
                        .buttonStyle(.plain)
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
            .padding(.horizontal, AeroricSpace.l)
            .padding(.vertical, AeroricSpace.s + 1)
            .background(AeroricColor.layer2, in: Capsule(style: .continuous))
            .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
            Button("全选") { pick.wrappedValue.selectAll() }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .accessibilityIdentifier("imports.picker.all")
            Button("全不选") { pick.wrappedValue.selectNone() }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .disabled(value.selected.isEmpty)
                .accessibilityIdentifier("imports.picker.none")
        }
    }

    private var list: some View {
        let value = pick.wrappedValue
        let groups = value.visibleGroups
        return ScrollView {
            LazyVStack(spacing: AeroricSpace.m) {
                ForEach(groups) { group in
                    card(group, tick: value.tick(group), selected: value.selected)
                }
                if groups.isEmpty {
                    Text(value.rows.isEmpty ? "没有找到 Claude Code 或 Codex 的会话。" : "没有匹配“\(value.query)”的项目或会话。")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, AeroricSpace.xxxl)
                }
            }
        }
        .scrollIndicators(.hidden)
        .frame(maxHeight: .infinity)
    }

    private func card(_ group: ImportPick.Group, tick: ImportPick.Tick, selected: Set<String>) -> some View {
        let open = !collapsed.contains(group.id)
        let count = group.sessions.filter { selected.contains($0.id) }.count
        return VStack(spacing: 0) {
            HStack(spacing: AeroricSpace.m) {
                Button { withAnimation(.easeOut(duration: AeroricMotion.quick)) { pick.wrappedValue.toggle(group: group) } } label: {
                    TickMark(tick: tick, size: 16)
                }
                .buttonStyle(.plain)
                .help(tick == .none ? "全选这个项目" : "取消这个项目的全部会话")
                .accessibilityLabel("全选 \(group.title)")
                .accessibilityAddTraits(tick == .all ? .isSelected : [])
                .accessibilityIdentifier("imports.picker.project.\(group.id)")
                Image(systemName: group.path == nil ? "questionmark.folder" : "folder.fill")
                    .font(.system(size: 14))
                    .foregroundStyle(group.isNew ? AeroricColor.accent : AeroricColor.textSecondary)
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: AeroricSpace.s) {
                        Text(group.title)
                            .font(.system(size: AeroricType.body, weight: .semibold))
                            .lineLimit(1)
                        if group.isNew {
                            StatusChip(text: "新项目", systemImage: "plus", tint: AeroricColor.accent)
                        } else if group.path != nil {
                            StatusChip(text: "已有项目", tint: AeroricColor.textTertiary)
                        }
                    }
                    if let path = group.path {
                        Text(ImportPresentation.abbreviated(path))
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    } else {
                        Text("会话没有记录工作目录，导入后放在“未归类导入”")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                    }
                }
                Spacer(minLength: AeroricSpace.s)
                Text("\(count)/\(group.sessions.count)")
                    .font(.system(size: AeroricType.caption).monospacedDigit())
                    .foregroundStyle(AeroricColor.textTertiary)
                Button {
                    withAnimation(.easeOut(duration: AeroricMotion.standard)) {
                        if open { collapsed.insert(group.id) } else { collapsed.remove(group.id) }
                    }
                } label: {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 10, weight: .semibold))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .rotationEffect(.degrees(open ? 90 : 0))
                        .frame(width: 22, height: 22)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(open ? "收起 \(group.title)" : "展开 \(group.title)")
            }
            .padding(.horizontal, AeroricSpace.l)
            .padding(.vertical, AeroricSpace.m)
            if open {
                HairlineDivider()
                VStack(spacing: 1) {
                    ForEach(group.sessions) { session in
                        row(session, selected: selected.contains(session.id))
                    }
                }
                .padding(AeroricSpace.xs)
            }
        }
        .configSurface(AeroricRadius.l)
    }

    private func row(_ session: ImportsClient.ImportedSession, selected: Bool) -> some View {
        HoverCapsuleRow(selected: false, height: 30, horizontalPadding: AeroricSpace.m) {
            pick.wrappedValue.toggle(session.id)
        } label: {
            HStack(spacing: AeroricSpace.m) {
                TickMark(tick: selected ? .all : .none, size: 14)
                AgentGlyph(agent: session.agent, size: 13)
                Text(session.title)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(selected ? AeroricColor.text : AeroricColor.textSecondary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Spacer(minLength: AeroricSpace.s)
                if session.tooLarge {
                    StatusChip(text: "较大", tint: AeroricColor.warning)
                        .help("会话文件较大，导入后只显示摘要")
                }
                if session.lastAtMs > 0 {
                    Text(RelativeTime.short(Date(timeIntervalSince1970: Double(session.lastAtMs) / 1000)))
                        .font(.system(size: AeroricType.caption).monospacedDigit())
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
        }
        .padding(.leading, AeroricSpace.xl)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("imports.picker.session.\(session.id)")
    }

    private var footer: some View {
        let value = book.pick
        let count = value?.selectedCount ?? 0
        let newProjects = value?.newProjectCount ?? 0
        return HStack(spacing: AeroricSpace.m) {
            if value != nil {
                Text(newProjects > 0 ? "已选 \(count) 个会话 · 将新建 \(newProjects) 个项目" : "已选 \(count) 个会话")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            Spacer()
            Button("取消") { book.pick = nil; dismiss() }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                .keyboardShortcut(.cancelAction)
            Button(count == 0 ? "确认" : "导入 \(count) 个会话") {
                Task {
                    await book.applyPick()
                    dismiss()
                }
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.primary))
            .keyboardShortcut(.defaultAction)
            .disabled(value == nil || book.busyAgents.contains("apply"))
            .accessibilityIdentifier("imports.picker.apply")
        }
    }
}

/// Round tick: filled check (all), filled dash (some), empty ring (none).
struct TickMark: View {
    let tick: ImportPick.Tick
    var size: CGFloat = 14

    var body: some View {
        Image(systemName: tick == .all ? "checkmark.circle.fill" : tick == .some ? "minus.circle.fill" : "circle")
            .font(.system(size: size))
            .foregroundStyle(tick == .none ? AeroricColor.textTertiary : AeroricColor.accent)
            .contentTransition(.symbolEffect(.replace))
    }
}
