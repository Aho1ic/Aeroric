import SwiftUI
import AppKit

/// Redesign 01 new-session page (codeg / Agents-Anywhere centred composer,
/// DSH colour roles). Project + Harness sit above the input; configuration
/// chips sit inside it; the combined capabilities are first-class cards.
/// Every action is injected; this view cannot discover profiles or start agents.
public struct NewSessionComposer: View {
    @Binding private var draft: NewSessionDraft
    private let projectName: String?
    private let projects: [TaskStore.ProjectEntry]
    private let configurations: [SessionConfiguration]
    private let blockingReasons: [String]
    private let onOpenProjectDraft: (String) -> Void
    private let onSubmit: () -> Void
    private let onCheckSubmission: () -> Void
    @State private var selectingAttachment = false
    @FocusState private var promptFocused: Bool
    @Environment(\.colorSchemeContrast) private var contrast

    /// codeg / Agents-Anywhere composer column (`max-w-3xl`).
    static let columnWidth: CGFloat = 768

    public init(draft: Binding<NewSessionDraft>, projectName: String?,
                projects: [TaskStore.ProjectEntry], configurations: [SessionConfiguration],
                blockingReasons: [String], onOpenProjectDraft: @escaping (String) -> Void,
                onSubmit: @escaping () -> Void, onCheckSubmission: @escaping () -> Void) {
        _draft = draft
        self.projectName = projectName
        self.projects = projects
        self.configurations = configurations
        self.blockingReasons = blockingReasons
        self.onOpenProjectDraft = onOpenProjectDraft
        self.onSubmit = onSubmit
        self.onCheckSubmission = onCheckSubmission
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                heading
                    .padding(.bottom, 20)
                contextRow
                    .padding(.horizontal, 4)
                    .padding(.bottom, 8)
                input
                attachmentHint
                    .padding(.horizontal, 4)
                capabilities
                    .padding(.top, 28)
                submissionStatus
                    .padding(.top, 16)
            }
            .frame(maxWidth: Self.columnWidth)
            .padding(.horizontal, 32)
            .padding(.top, 56)
            .padding(.bottom, 40)
            .frame(maxWidth: .infinity)
        }
        .background(SessionPalette.canvas)
        .font(AeroricFont.ui(AeroricFont.body))
        .foregroundStyle(SessionPalette.text)
        .fileImporter(isPresented: $selectingAttachment, allowedContentTypes: [.item],
                      allowsMultipleSelection: true) { result in
            guard case let .success(urls) = result else { return }
            // Explicit selection grants a reference only. No file reads,
            // uploads, bookmarks or implicit submission.
            guard draft.isEditable else { return }
            SessionAttachmentReferences.append(urls, to: &draft.task.attachmentPaths)
        }
    }

    // MARK: - Heading and context

    private var heading: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(projectName.map { "在 \($0) 中开始新会话" } ?? "开始新会话")
                .font(AeroricFont.ui(22, weight: .semibold))
                .accessibilityIdentifier("new-session.title")
            Text("描述目标，选择执行者与需要叠加的能力。")
                .font(AeroricFont.ui(AeroricFont.body))
                .foregroundStyle(SessionPalette.secondary)
        }
    }

    private var contextRow: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .center, spacing: 12) {
                projectMenu
                Spacer(minLength: 8)
                harness
            }
            VStack(alignment: .leading, spacing: 8) {
                projectMenu
                harness
            }
        }
    }

    private var projectMenu: some View {
        Menu {
            if projectName != nil {
                Text("原草稿保留在当前项目；项目归属迁移规则待确认。")
            }
            ForEach(projects) { project in
                Button("打开 \(project.name) 的草稿") { onOpenProjectDraft(project.id) }
            }
            if projects.isEmpty { Text("尚无可用项目，请先打开项目目录。") }
        } label: {
            Label {
                Text(projectName ?? "选择项目")
                    .font(AeroricFont.ui(AeroricFont.body, weight: .medium))
            } icon: {
                Image(systemName: "folder")
            }
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityLabel(projectName.map { "项目：\($0)" } ?? "选择项目")
        .accessibilityIdentifier("new-session.project")
    }

    private var harness: some View {
        HStack(spacing: 8) {
            if !draft.task.agent.isEmpty,
               !NativeHarnessPicker.harnesses.contains(where: { $0.id == draft.task.agent }) {
                Text("自定义 Agent：\(draft.task.agent)")
                    .font(AeroricFont.ui(AeroricFont.meta))
                    .foregroundStyle(SessionPalette.secondary)
            }
            CapsuleHarnessPicker(selection: Binding(
                get: { draft.selectedHarness },
                set: { draft.selectHarness($0) }
            ))
            .fixedSize()
            .disabled(!draft.isEditable)
        }
    }

    // MARK: - Composer

    private var input: some View {
        let shape = RoundedRectangle(cornerRadius: ShellRadius.composer, style: .continuous)
        return VStack(alignment: .leading, spacing: 0) {
            ZStack(alignment: .topLeading) {
                if draft.task.prompt.isEmpty {
                    Text("描述你想完成的事…")
                        .foregroundStyle(SessionPalette.muted)
                        .padding(.horizontal, 5)
                        .padding(.top, 1)
                        .allowsHitTesting(false)
                        .accessibilityHidden(true)
                }
                TextEditor(text: $draft.task.prompt)
                    .scrollContentBackground(.hidden)
                    .focused($promptFocused)
                    .frame(minHeight: 96, maxHeight: 280)
                    .accessibilityLabel("会话目标")
                    .accessibilityIdentifier("new-session.prompt")
                    .disabled(!draft.isEditable)
            }
            .font(AeroricFont.ui(14))
            .padding(.horizontal, 14)
            .padding(.top, 14)
            attachmentReferences
                .padding(.horizontal, 16)
            HStack(alignment: .center, spacing: 6) {
                configurationChips
                Spacer(minLength: 6)
                actions
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 10)
        }
        .background(SessionPalette.input, in: shape)
        .overlay {
            shape.strokeBorder(promptFocused ? SessionPalette.accent.opacity(0.6) : SessionPalette.edge,
                               lineWidth: contrast == .increased ? 1.5 : 1)
                .allowsHitTesting(false)
        }
        .shadow(color: contrast == .increased ? .clear : SessionPalette.shadow, radius: 10, y: 3)
        .dropDestination(for: URL.self) { urls, _ in
            guard draft.isEditable else { return false }
            return SessionAttachmentReferences.append(urls, to: &draft.task.attachmentPaths)
        }
    }

    private var selectedConfiguration: SessionConfiguration? {
        configurations.first {
            $0.id == draft.configurationID && $0.version == draft.configurationVersion
                && $0.agent == draft.task.agent
        }
    }

    private var configurationChips: some View {
        HStack(spacing: 2) {
            addMenu
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 2) { configurationMenu; modelMenu; permissionMenu; effortChip }
                HStack(spacing: 2) { configurationMenu; modelMenu; permissionMenu }
                HStack(spacing: 2) { configurationMenu; modelMenu }
            }
        }
        .disabled(!draft.isEditable)
    }

    /// codeg composer add menu: references plus the recipe / scheme entries.
    /// Recipe and scheme catalogues are not wired yet, so they stay as
    /// disabled rows with an honest note rather than a fabricated list.
    private var addMenu: some View {
        CapsuleDropdown(help: "添加", identifier: "new-session.add", items: {
            [
                .action(id: "attach", title: "添加资料引用", detail: "仅保留文件引用",
                        systemImage: "paperclip") { selectingAttachment = true },
                .divider("entries"),
                .action(id: "recipe", title: "工作配方", detail: "配方目录尚未接通",
                        systemImage: "book.closed", enabled: false) {},
                .action(id: "scheme", title: "工作方案", detail: "复用能力与角色组合 · 尚未接通",
                        systemImage: "square.3.layers.3d", enabled: false) {},
            ]
        }) {
            Image(systemName: "plus")
                .font(.system(size: 13, weight: .medium))
                .frame(width: 12)
        }
        .accessibilityLabel("添加")
    }

    private var configurationMenu: some View {
        CapsuleDropdown(help: "主配置", identifier: "new-session.configuration", items: {
            let available = configurations.filter { $0.harness == draft.selectedHarness }
            var items: [DropdownItem] = [.header("主配置")]
            items += available.map { configuration in
                .option(id: configuration.id, title: configuration.title,
                        selected: configuration.id == selectedConfiguration?.id) { draft.apply(configuration) }
            }
            if available.isEmpty { items.append(.note("宿主尚未提供此 Harness 的配置目录。")) }
            return items
        }) {
            ChipLabel(systemImage: "person.crop.square", text: selectedConfiguration?.title ?? "主配置")
        }
    }

    private var modelMenu: some View {
        CapsuleDropdown(help: draft.task.model ?? "尚未取得配置模型", identifier: "new-session.model", items: {
            var items: [DropdownItem] = [.header("模型")]
            items += (selectedConfiguration?.options.models ?? []).map { model in
                .option(id: model, title: model, selected: model == draft.task.model) { draft.task.model = model }
            }
            if selectedConfiguration == nil { items.append(.note("模型能力待宿主确认。")) }
            else if selectedConfiguration?.options.models.isEmpty == true {
                items.append(.note("此配置没有已存模型目录；不会查询 API 或假设默认值。"))
            }
            return items
        }) {
            ChipLabel(systemImage: "cpu", text: draft.task.model ?? "模型")
        }
        .accessibilityLabel("模型：\(draft.task.model ?? "待配置")")
    }

    private var permissionMenu: some View {
        CapsuleDropdown(help: "权限", identifier: "new-session.permission", items: {
            var items: [DropdownItem] = [.header("权限")]
            items += (selectedConfiguration?.options.permissionModes ?? []).map { mode in
                .option(id: mode, title: mode, selected: mode == draft.task.permissionMode) {
                    draft.selectPermission(mode)
                }
            }
            if selectedConfiguration == nil { items.append(.note("权限能力待宿主确认。")) }
            return items
        }) {
            ChipLabel(systemImage: "lock.shield",
                      text: draft.task.permissionMode.isEmpty ? "权限" : draft.task.permissionMode)
        }
    }

    /// ZCode-style cycle: one click advances to the next effort the selected
    /// configuration supports. Never invents a level.
    @ViewBuilder private var effortChip: some View {
        let efforts = selectedConfiguration?.options.efforts ?? []
        if !efforts.isEmpty || draft.task.effort != nil {
            Button {
                guard !efforts.isEmpty else { return }
                let index = draft.task.effort.flatMap { efforts.firstIndex(of: $0) }
                draft.task.effort = efforts[index.map { ($0 + 1) % efforts.count } ?? 0]
            } label: {
                ChipLabel(systemImage: "brain", text: draft.task.effort ?? "思考档", showsChevron: false)
            }
            .buttonStyle(GhostCapsuleButtonStyle())
            .help("点击切换到下一档")
            .accessibilityLabel("思考档：\(draft.task.effort ?? "未选择")，点击切换")
            .accessibilityIdentifier("new-session.effort")
        }
    }

    private var actions: some View {
        HStack(spacing: 4) {
            NativeIconButton(systemImage: "paperclip", label: "添加资料引用",
                             identifier: "new-session.attach") { selectingAttachment = true }
                .foregroundStyle(SessionPalette.secondary)
                .disabled(!draft.isEditable)
            let canSend = draft.isEditable && blockingReasons.isEmpty
            Button(action: onSubmit) {
                Image(systemName: "arrow.up")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(canSend ? Color.white : SessionPalette.muted)
                    .frame(width: 28, height: 28)
                    .background(canSend ? SessionPalette.accent : SessionPalette.chip, in: Circle())
            }
            .buttonStyle(.plain)
            .keyboardShortcut(.return, modifiers: .command)
            .disabled(!canSend)
            .help(blockingReasons.first ?? "发送并开始 ⌘↩")
            .accessibilityLabel("发送并开始")
            .accessibilityIdentifier("new-session.submit")
        }
    }

    @ViewBuilder private var attachmentReferences: some View {
        if !draft.task.attachmentPaths.isEmpty {
            HStack(spacing: 6) {
                ForEach(draft.task.attachmentPaths, id: \.self) { path in
                    HStack(spacing: 4) {
                        Image(systemName: "doc")
                        Text(URL(fileURLWithPath: path).lastPathComponent).lineLimit(1)
                        Button {
                            draft.task.attachmentPaths.removeAll { $0 == path }
                        } label: { Image(systemName: "xmark").font(.system(size: 9, weight: .semibold)) }
                        .buttonStyle(.plain)
                        .accessibilityLabel("移除资料引用")
                        .accessibilityIdentifier("new-session.remove-reference.\(path)")
                        .disabled(!draft.isEditable)
                    }
                    .font(AeroricFont.ui(AeroricFont.meta))
                    .padding(.horizontal, 10).padding(.vertical, 4)
                    .background(SessionPalette.chip, in: Capsule(style: .continuous))
                }
                Spacer(minLength: 0)
            }
            .padding(.top, 6)
        }
    }

    /// Only the pending-attachment caveat remains; the drag hint was removed.
    @ViewBuilder private var attachmentHint: some View {
        if !draft.task.attachmentPaths.isEmpty {
            Text("附件交办尚未接通，发送前需处理。")
                .font(AeroricFont.ui(AeroricFont.meta))
                .foregroundStyle(SessionPalette.muted)
                .padding(.top, 8)
        }
    }

    // MARK: - Capabilities (Aeroric native features)

    private var capabilities: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline) {
                Text("组合能力").font(AeroricFont.ui(AeroricFont.body, weight: .semibold))
                Text("可叠加，不是互斥模式")
                    .font(AeroricFont.ui(AeroricFont.meta))
                    .foregroundStyle(SessionPalette.muted)
                Spacer()
            }
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .top, spacing: 10) { capabilityCards }
                VStack(spacing: 8) { capabilityCards }
            }
        }
    }

    @ViewBuilder private var capabilityCards: some View {
        CapabilityCard(title: "团队协作", detail: "共同主计划，按工作包分工。",
                       systemImage: "person.3", tone: SessionPalette.blue,
                       roles: ["协调者", "成员"],
                       isOn: capabilityBinding(\.team), editable: draft.isEditable)
        CapabilityCard(title: "独立审查", detail: "由独立角色核对变更与证据。",
                       systemImage: "checkmark.seal", tone: SessionPalette.violet,
                       roles: ["审查者"],
                       isOn: capabilityBinding(\.review), editable: draft.isEditable)
        CapabilityCard(title: "任务工具进化", detail: "主执行、阶段反思、独立治理。",
                       systemImage: "arrow.triangle.2.circlepath", tone: SessionPalette.teal,
                       roles: ["反思者", "治理者"],
                       isOn: capabilityBinding(\.evolution), editable: draft.isEditable)
    }

    private func capabilityBinding(_ key: WritableKeyPath<SessionCapabilities, Bool>) -> Binding<Bool> {
        Binding(get: { draft.capabilities[keyPath: key] },
                set: { draft.capabilities[keyPath: key] = $0 })
    }

    @ViewBuilder private var submissionStatus: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(blockingReasons, id: \.self) { reason in
                Label(reason, systemImage: "info.circle")
                    .font(AeroricFont.ui(AeroricFont.meta))
                    .foregroundStyle(SessionPalette.secondary)
            }
            switch draft.submission {
            case .editing: EmptyView()
            case .sending: ProgressView("正在交办…").controlSize(.small)
            case let .failed(reason):
                Text(reason).font(AeroricFont.ui(AeroricFont.meta)).foregroundStyle(Color.dsDanger)
            case let .unknown(reason):
                Text("接收结果未知，草稿已保留，请先核对原交办。\(reason)")
                    .font(AeroricFont.ui(AeroricFont.meta))
                    .foregroundStyle(SessionPalette.amber)
                Button("核对原交办") { onCheckSubmission() }
                    .accessibilityIdentifier("new-session.check-submission")
            }
            if draft.task.permissionMode == "full_access" {
                Text("Full Access 不关闭监督、受保护规则、Stop 或技能变更的人为同意。")
                    .font(AeroricFont.ui(AeroricFont.meta)).foregroundStyle(SessionPalette.amber)
            }
        }
    }
}

// MARK: - Pieces

private struct ChipLabel: View {
    let systemImage: String
    let text: String
    var showsChevron = true

    var body: some View {
        HStack(spacing: 5) {
            Image(systemName: systemImage).font(.system(size: 11))
            Text(text).lineLimit(1).truncationMode(.middle).frame(maxWidth: 160, alignment: .leading)
            if showsChevron {
                Image(systemName: "chevron.down").font(.system(size: 8, weight: .semibold))
                    .foregroundStyle(SessionPalette.muted)
            }
        }
        .font(AeroricFont.ui(12))
        .fixedSize()
    }
}

/// One combined-capability card. Checking it reveals the roles it adds;
/// role bindings are not yet carried by the draft, so they are listed as
/// following the main configuration rather than offering fake choices.
private struct CapabilityCard: View {
    let title: String
    let detail: String
    let systemImage: String
    let tone: Color
    let roles: [String]
    @Binding var isOn: Bool
    let editable: Bool
    @State private var hovered = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: ShellRadius.card, style: .continuous)
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: systemImage)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(tone)
                    .frame(width: 28, height: 28)
                    .background(tone.opacity(0.12), in: Circle())
                VStack(alignment: .leading, spacing: 3) {
                    Text(title).font(AeroricFont.ui(AeroricFont.body, weight: .semibold))
                    Text(detail).font(AeroricFont.ui(AeroricFont.meta))
                        .foregroundStyle(SessionPalette.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 4)
                Toggle(title, isOn: $isOn)
                    .toggleStyle(.checkbox)
                    .labelsHidden()
                    .tint(tone)
            }
            if isOn {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(roles, id: \.self) { role in
                        HStack(spacing: 6) {
                            Circle().fill(tone).frame(width: 5, height: 5)
                            Text(role).font(AeroricFont.ui(AeroricFont.meta, weight: .medium))
                            Spacer(minLength: 4)
                            Text("沿用主配置")
                                .font(AeroricFont.ui(AeroricFont.meta))
                                .foregroundStyle(SessionPalette.muted)
                        }
                    }
                    Text("独立角色的 Harness / 模型选择待宿主接通。")
                        .font(AeroricFont.ui(10))
                        .foregroundStyle(SessionPalette.muted)
                }
                .padding(.top, 8)
                .overlay(alignment: .top) { Rectangle().fill(SessionPalette.edge).frame(height: 1) }
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isOn ? tone.opacity(0.06) : SessionPalette.card, in: shape)
        .overlay {
            shape.strokeBorder(isOn ? tone.opacity(0.5) : (hovered ? SessionPalette.muted.opacity(0.5) : SessionPalette.edge),
                               lineWidth: 1)
                .allowsHitTesting(false)
        }
        .contentShape(shape)
        .onTapGesture { if editable { isOn.toggle() } }
        .onHover { hovered = $0 }
        .disabled(!editable)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isButton)
        .accessibilityValue(isOn ? "已启用" : "未启用")
        .accessibilityIdentifier("new-session.capability.\(title)")
    }
}

