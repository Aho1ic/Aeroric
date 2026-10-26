import SwiftUI
import AppKit

/// New-session home (Q12): a question that types itself in, scenario tabs
/// with recipe cards, and the floating composer with the host-sourced
/// pickers. Every action is injected; this view cannot discover profiles,
/// start agents or send on its own.
public struct NewSessionHome: View {
    @Binding private var draft: NewSessionDraft
    private let projectID: String?
    private let projects: [TaskStore.ProjectEntry]
    private let configurations: [SessionConfiguration]
    private let blockingReasons: [String]
    private let onOpenProjectDraft: (String) -> Void
    private let onSubmit: () -> Void
    private let onCheckSubmission: () -> Void

    @State private var scenario: Scenario = .build
    @State private var pendingRecipe: ScenarioRecipe?
    @State private var focusRequest = 0
    @State private var selectingAttachment = false

    public static let heading = "你想做什么？"
    static let columnWidth: CGFloat = 760

    public init(draft: Binding<NewSessionDraft>, projectID: String?, projects: [TaskStore.ProjectEntry],
                configurations: [SessionConfiguration], blockingReasons: [String],
                onOpenProjectDraft: @escaping (String) -> Void,
                onSubmit: @escaping () -> Void, onCheckSubmission: @escaping () -> Void) {
        _draft = draft
        self.projectID = projectID
        self.projects = projects
        self.configurations = configurations
        self.blockingReasons = blockingReasons
        self.onOpenProjectDraft = onOpenProjectDraft
        self.onSubmit = onSubmit
        self.onCheckSubmission = onCheckSubmission
    }

    private var project: TaskStore.ProjectEntry? { projects.first { $0.id == projectID } }
    private var selected: SessionConfiguration? {
        ComposerPickerOptions.selectedConfiguration(configurations, draft: draft)
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                VStack(alignment: .leading, spacing: AeroricSpace.s) {
                    TypewriterText(Self.heading, size: AeroricType.hero)
                        .accessibilityIdentifier("new-session.title")
                    Text(project.map { "在 \($0.name) 中开始新会话" } ?? "先在下方选择一个项目")
                        .font(.system(size: AeroricType.body))
                        .foregroundStyle(AeroricColor.textSecondary)
                }
                .padding(.bottom, AeroricSpace.xxxl)

                composer
                status.padding(.top, AeroricSpace.m)

                CapsuleSlider(selection: $scenario, groupTitle: "场景", size: .large,
                              identifier: "new-session.scenario",
                              segments: Scenario.allCases.map { .init(value: $0, title: $0.title, systemImage: $0.symbol) })
                .fixedSize()
                .padding(.top, AeroricSpace.huge)
                .padding(.bottom, AeroricSpace.l)

                LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: AeroricSpace.m)],
                          spacing: AeroricSpace.m) {
                    ForEach(scenario.recipes) { recipe in
                        RecipeCard(recipe: recipe) { choose(recipe) }
                            .disabled(!draft.isEditable)
                    }
                }
            }
            .frame(maxWidth: Self.columnWidth)
            .padding(.horizontal, AeroricSpace.huge)
            .padding(.top, 72)
            .padding(.bottom, AeroricSpace.huge)
            .frame(maxWidth: .infinity)
        }
        .background(AeroricColor.canvas)
        .foregroundStyle(AeroricColor.text)
        .confirmationDialog("输入框里已有内容", isPresented: Binding(
            get: { pendingRecipe != nil }, set: { if !$0 { pendingRecipe = nil } }
        ), presenting: pendingRecipe) { recipe in
            Button("替换为「\(recipe.title)」模板") { insert(recipe, .replace) }
            Button("追加到末尾") { insert(recipe, .append) }
            Button("取消", role: .cancel) {}
        }
        .fileImporter(isPresented: $selectingAttachment, allowedContentTypes: [.item],
                      allowsMultipleSelection: true) { result in
            // Explicit selection grants a reference only: no reads, uploads or sending.
            guard case let .success(urls) = result, draft.isEditable else { return }
            SessionAttachmentReferences.append(urls, to: &draft.task.attachmentPaths)
        }
        .accessibilityIdentifier("new-session.home")
    }

    // MARK: - Composer

    private var composer: some View {
        ComposerCard(
            text: $draft.task.prompt, attachments: $draft.task.attachmentPaths,
            placeholder: "描述你想完成的事…", minLines: 3, isEditable: draft.isEditable,
            acceptsAttachments: true,
            sendState: blockingReasons.first.map(ComposerSendState.blocked) ?? .ready,
            identifierPrefix: "new-session", focusRequest: focusRequest, onSend: onSubmit
        ) {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: AeroricSpace.xxs) { addMenu; workspacePicker; configurationPicker; permissionPicker; modelPicker; effortPicker }
                HStack(spacing: AeroricSpace.xxs) { addMenu; workspacePicker; configurationPicker; permissionPicker; modelPicker }
                HStack(spacing: AeroricSpace.xxs) { addMenu; configurationPicker; modelPicker }
            }
            .disabled(!draft.isEditable)
        }
    }

    /// References plus the combined capabilities. The capabilities are not
    /// wired to the host yet, so they can only be switched off (a restored
    /// preference), never on.
    private var addMenu: some View {
        Menu {
            Button("添加资料引用…", systemImage: "paperclip") { selectingAttachment = true }
            Section("组合能力 · 即将支持") {
                capabilityToggle("团队协作", \.team)
                capabilityToggle("独立审查", \.review)
                capabilityToggle("任务工具进化", \.evolution)
            }
        } label: {
            Image(systemName: "plus")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(AeroricColor.textSecondary)
                .frame(width: 26, height: 26)
                .contentShape(Circle())
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .help("添加")
        .accessibilityLabel("添加")
        .accessibilityIdentifier("new-session.add")
    }

    private func capabilityToggle(_ title: String, _ key: WritableKeyPath<SessionCapabilities, Bool>) -> some View {
        Toggle(title, isOn: Binding(get: { draft.capabilities[keyPath: key] },
                                    set: { draft.capabilities[keyPath: key] = $0 }))
            .disabled(!draft.capabilities[keyPath: key])
    }

    private var workspacePicker: some View {
        ComposerPicker(title: project?.name ?? "选择项目", systemImage: "folder",
                       menu: ComposerPickerOptions.workspaces(projects, selectedID: projectID),
                       identifier: "new-session.project") { id in
            if id != projectID { onOpenProjectDraft(id) }
        }
    }

    private var configurationPicker: some View {
        ComposerPicker(title: selected?.title ?? "Agent", systemImage: AgentGlyph.symbol(for: selected?.agent),
                       menu: ComposerPickerOptions.configurations(configurations, draft: draft),
                       identifier: "new-session.configuration") { id in
            guard let configuration = configurations.first(where: { "\($0.id)@\($0.version)" == id }) else { return }
            draft.selectHarness(configuration.harness)
            draft.apply(configuration)
        }
    }

    private var permissionPicker: some View {
        ComposerPicker(title: draft.task.permissionMode.isEmpty ? "权限"
                           : ComposerPickerOptions.permissionTitle(draft.task.permissionMode),
                       systemImage: "lock.shield",
                       menu: ComposerPickerOptions.permissions(selected, draft: draft),
                       identifier: "new-session.permission") { draft.selectPermission($0) }
    }

    private var modelPicker: some View {
        ComposerPicker(title: draft.task.model ?? "模型", systemImage: "cpu",
                       menu: ComposerPickerOptions.models(selected, draft: draft),
                       identifier: "new-session.model") { draft.task.model = $0 }
    }

    @ViewBuilder private var effortPicker: some View {
        if let menu = ComposerPickerOptions.efforts(selected, draft: draft) {
            ComposerPicker(title: draft.task.effort.map(ComposerPickerOptions.effortTitle) ?? "思考强度",
                           systemImage: "brain", menu: menu,
                           identifier: "new-session.effort") { draft.task.effort = $0 }
        }
    }

    // MARK: - Status

    @ViewBuilder private var status: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            if let reason = blockingReasons.first {
                Label(blockingReasons.count > 1 ? "\(reason)（另有 \(blockingReasons.count - 1) 项）" : reason,
                      systemImage: "info.circle")
                    .foregroundStyle(AeroricColor.textSecondary)
                    .help(blockingReasons.joined(separator: "\n"))
                    .accessibilityIdentifier("new-session.blocking-reason")
            }
            switch draft.submission {
            case .editing: EmptyView()
            case .sending: ProgressView("正在交办…").controlSize(.small)
            case let .failed(reason):
                Text(reason).foregroundStyle(AeroricColor.danger)
            case let .unknown(reason):
                Text("接收结果未知，草稿已保留，请先核对原交办。\(reason)")
                    .foregroundStyle(AeroricColor.warning)
                Button("核对原交办", action: onCheckSubmission)
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .accessibilityIdentifier("new-session.check-submission")
            }
            if draft.task.permissionMode == "full_access" {
                Text("完全访问不会关闭监督、受保护规则、停止或技能变更的人为同意。")
                    .foregroundStyle(AeroricColor.warning)
            }
        }
        .font(.system(size: AeroricType.secondary))
        .padding(.horizontal, AeroricSpace.s)
    }

    // MARK: - Recipes

    private func choose(_ recipe: ScenarioRecipe) {
        guard draft.isEditable else { return }
        if draft.task.prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            insert(recipe, .replace)
        } else {
            pendingRecipe = recipe
        }
    }

    private func insert(_ recipe: ScenarioRecipe, _ insertion: ScenarioCatalog.Insertion) {
        draft.task.prompt = ScenarioCatalog.merged(draft.task.prompt, with: recipe, as: insertion)
        pendingRecipe = nil
        focusRequest += 1
    }
}

/// Recipe card: icon, name, one-line description. Click only fills the input.
private struct RecipeCard: View {
    let recipe: ScenarioRecipe
    let action: () -> Void
    @State private var hovered = false
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.l)
        Button(action: action) {
            HStack(alignment: .top, spacing: AeroricSpace.l) {
                Image(systemName: recipe.symbol)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .frame(width: 30, height: 30)
                    .background(AeroricColor.layer2, in: AeroricRadius.shape(AeroricRadius.s))
                VStack(alignment: .leading, spacing: 2) {
                    Text(recipe.title)
                        .font(.system(size: AeroricType.body, weight: .semibold))
                        .foregroundStyle(AeroricColor.text)
                    Text(recipe.detail)
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            .padding(AeroricSpace.l)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(hovered && isEnabled ? AeroricColor.hover : AeroricColor.layer1, in: shape)
            .overlay { shape.strokeBorder(AeroricColor.separator1, lineWidth: 1) }
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help("填入模板，不会自动发送")
        .accessibilityIdentifier("new-session.recipe.\(recipe.id)")
    }
}
