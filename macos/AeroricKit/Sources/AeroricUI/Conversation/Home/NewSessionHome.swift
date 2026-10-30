import SwiftUI
import AppKit

/// New-session home (Q12): the Propel motto, the floating composer with the
/// host-sourced pickers, and the scenario tabs with recipe cards right below
/// it, the whole group centred in the window. Every action is injected; this view cannot discover profiles,
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
    /// D: the project's branches (nil when it is not a git repository).
    private let branches: BranchList?
    private let onSwitchBranch: (String) -> Void
    /// Settings → Agents → 新会话默认 Harness.
    private let preferredHarness: String

    public struct BranchList: Equatable, Sendable {
        public var current: String?
        public var names: [String]
        public init(current: String?, names: [String]) { self.current = current; self.names = names }
    }

    @State private var scenario: Scenario = .build
    @State private var showingRecipes = false
    @Environment(BuiltinSkillBook.self) private var skillBook: BuiltinSkillBook?
    @State private var pendingRecipe: ScenarioRecipe?
    @State private var focusRequest = 0
    @State private var selectingAttachment = false

    public static let heading = PropelMotto.text
    static let columnWidth: CGFloat = 760

    public init(draft: Binding<NewSessionDraft>, projectID: String?, projects: [TaskStore.ProjectEntry],
                configurations: [SessionConfiguration], blockingReasons: [String],
                onOpenProjectDraft: @escaping (String) -> Void,
                onSubmit: @escaping () -> Void, onCheckSubmission: @escaping () -> Void,
                branches: BranchList? = nil, onSwitchBranch: @escaping (String) -> Void = { _ in },
                preferredHarness: String = "codex") {
        _draft = draft
        self.projectID = projectID
        self.projects = projects
        self.configurations = configurations
        self.blockingReasons = blockingReasons
        self.onOpenProjectDraft = onOpenProjectDraft
        self.onSubmit = onSubmit
        self.onCheckSubmission = onCheckSubmission
        self.branches = branches
        self.onSwitchBranch = onSwitchBranch
        self.preferredHarness = preferredHarness
    }

    private var availability: [String: Bool] { NewSessionLayout.availability(configurations) }

    /// The slider's harness: the draft's when it has one in the slider, else the default.
    private var harness: String {
        let chosen = draft.selectedHarness
        if NewSessionLayout.harnessOrder.contains(chosen), draft.configurationID != nil || availability[chosen] == true {
            return chosen
        }
        return NewSessionLayout.defaultHarness(preferred: preferredHarness, availability: availability)
    }

    private func selectHarness(_ value: String) {
        guard draft.isEditable else { return }
        draft.selectHarness(value)
        if let first = NewSessionLayout.firstConfiguration(for: value, in: configurations) {
            draft.apply(first)
        }
    }

    /// A fresh draft gets the default harness and its first usable entry; a
    /// choice the user made is never replaced.
    private func adoptDefaults() {
        guard draft.isEditable, draft.configurationID == nil, !configurations.isEmpty else { return }
        selectHarness(NewSessionLayout.defaultHarness(preferred: preferredHarness, availability: availability))
    }

    private var project: TaskStore.ProjectEntry? { projects.first { $0.id == projectID } }
    private var selected: SessionConfiguration? {
        ComposerPickerOptions.selectedConfiguration(configurations, draft: draft)
    }

    public var body: some View {
        GeometryReader { viewport in
            ScrollView {
                VStack(spacing: 0) {
                    // 2026-10-28 (第五轮): motto, the row above the input, the
                    // input and the scenarios move as one group, centred a
                    // little above the middle of the window.
                    PropelMotto().padding(.bottom, AeroricSpace.huge)

                    aboveComposer.padding(.bottom, AeroricSpace.s)
                    composer
                    status.padding(.top, AeroricSpace.m)
                        .frame(maxWidth: .infinity, alignment: .leading)

                    CapsuleSlider(selection: $scenario, groupTitle: "场景", size: .large,
                                  identifier: "new-session.scenario",
                                  segments: Scenario.allCases.map { .init(value: $0, title: $0.title, systemImage: $0.symbol) })
                    .fixedSize()
                    .padding(.top, AeroricSpace.xxxl + AeroricSpace.xs) // 36 below the composer with the status padding
                    .padding(.bottom, AeroricSpace.l)

                    FlowLayout(spacing: AeroricSpace.s, centered: true) {
                        ForEach(scenario.recipes) { recipe in
                            RecipeChip(recipe: recipe, skilled: invocation(for: recipe) != nil) { choose(recipe) }
                                .disabled(!draft.isEditable)
                        }
                    }
                }
                .frame(maxWidth: Self.columnWidth)
                .padding(.horizontal, AeroricSpace.huge)
                .padding(.top, AeroricSpace.huge)
                .padding(.bottom, AeroricSpace.huge + 2 * AeroricSpace.xxl)
                .frame(maxWidth: .infinity, minHeight: viewport.size.height)
            }
        }
        .onAppear(perform: adoptDefaults)
        .onChange(of: configurations) { _, _ in adoptDefaults() }
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
                HStack(spacing: AeroricSpace.xxs) { addMenu; configurationPicker; modelPicker; permissionPicker; effortPicker }
                HStack(spacing: AeroricSpace.xxs) { addMenu; configurationPicker; modelPicker; permissionPicker }
                HStack(spacing: AeroricSpace.xxs) { addMenu; configurationPicker; modelPicker }
            }
            .disabled(!draft.isEditable)
        }
    }

    /// D: project and branch on the left, the harness slider on the right.
    private var aboveComposer: some View {
        HStack(spacing: AeroricSpace.xs) {
            workspacePicker
            if let branches {
                BranchPicker(current: branches.current, branches: branches.names, onSelect: onSwitchBranch)
                    .disabled(!draft.isEditable)
            }
            Spacer(minLength: AeroricSpace.l)
            HarnessSlider(selection: harness, availability: availability, onSelect: selectHarness)
                .disabled(!draft.isEditable)
        }
    }

    /// References plus the combined capabilities. The capabilities are not
    /// wired to the host yet, so they can only be switched off (a restored
    /// preference), never on.
    private var addMenu: some View {
        Menu {
            Button("添加资料引用…", systemImage: "paperclip") { selectingAttachment = true }
            if skillBook != nil {
                Button("工作配方…", systemImage: "sparkles") { showingRecipes = true }
            }
            Toggle("开工前澄清", isOn: Binding(get: { draft.capabilities.clarify },
                                           set: { draft.capabilities.clarify = $0 }))
                .help("第一轮只列出推断的需求和验收条款，你确认后才开工")
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
        .popover(isPresented: $showingRecipes, arrowEdge: .bottom) {
            if let skillBook {
                BuiltinRecipePanel(book: skillBook) { recipe in
                    showingRecipes = false
                    choose(recipe)
                }
                .task { if !skillBook.loaded { await skillBook.refresh() } }
            }
        }
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
                       menu: ComposerPickerOptions.configurations(configurations, draft: draft, harness: harness,
                                                                  remote: project?.path.hasPrefix("ssh://") == true),
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

    /// 2026-10-27: an empty prompt is shown by the disabled send button, not
    /// spelled out under the composer.
    private var shownReasons: [String] {
        blockingReasons.filter { $0 != TaskDraft.emptyPromptMessage }
    }

    @ViewBuilder private var status: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            if let reason = shownReasons.first {
                Label(shownReasons.count > 1 ? "\(reason)（另有 \(shownReasons.count - 1) 项）" : reason,
                      systemImage: "info.circle")
                    .foregroundStyle(AeroricColor.textSecondary)
                    .help(shownReasons.joined(separator: "\n"))
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

    /// How this draft's harness calls the card's skill; nil keeps the plain template.
    private func invocation(for recipe: ScenarioRecipe) -> String? {
        guard let skill = recipe.skill, let skillBook else { return nil }
        return skillBook.invocation(skill: skill, harness: harness, configurationID: draft.configurationID)
    }

    private func insert(_ recipe: ScenarioRecipe, _ insertion: ScenarioCatalog.Insertion) {
        draft.task.prompt = ScenarioCatalog.merged(draft.task.prompt, with: recipe, as: insertion,
                                                   invocation: invocation(for: recipe))
        pendingRecipe = nil
        focusRequest += 1
    }
}
