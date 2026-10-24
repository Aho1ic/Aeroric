import SwiftUI

/// First-launch onboarding (M4): welcome → agents → projects → import → done.
/// Left text, right illustration; pages slide with a short fade, ←/→ move,
/// Reduce Motion removes the animation. Reading the user's existing sessions
/// happens only after they tick an agent and press 导入 (Q22, D6).
public struct OnboardingView: View {
    let book: ImportBook
    let projects: [TaskStore.ProjectEntry]
    let dataDirectory: String
    let onOpenProject: () -> Void
    let onFinish: () -> Void
    @State private var step: OnboardingStep = .welcome
    @State private var forward = true
    @State private var selectedAgents: Set<String> = []
    @State private var confirmingSkip = false
    @State private var importing = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(book: ImportBook, projects: [TaskStore.ProjectEntry], dataDirectory: String,
                onOpenProject: @escaping () -> Void, onFinish: @escaping () -> Void) {
        self.book = book
        self.projects = projects
        self.dataDirectory = dataDirectory
        self.onOpenProject = onOpenProject
        self.onFinish = onFinish
    }

    public var body: some View {
        VStack(spacing: 0) {
            ZStack {
                page(step)
                    .id(step)
                    .transition(reduceMotion ? .identity : .asymmetric(
                        insertion: .move(edge: forward ? .trailing : .leading).combined(with: .opacity),
                        removal: .move(edge: forward ? .leading : .trailing).combined(with: .opacity)))
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .clipped()
            HairlineDivider()
            footer
        }
        .frame(width: 760, height: 500)
        .background(AeroricColor.canvas)
        .focusable()
        .focusEffectDisabled()
        .onKeyPress(.rightArrow) { go(step.next); return .handled }
        .onKeyPress(.leftArrow) { go(step.previous); return .handled }
        .task {
            await book.detectAgents()
            for agent in ImportBook.importableAgents { await book.estimate(agent) }
        }
        .confirmationDialog("跳过引导？", isPresented: $confirmingSkip) {
            Button("跳过") { onFinish() }
            Button("继续引导", role: .cancel) {}
        } message: {
            Text("之后可以在 设置 → 通用 重新打开；导入已有会话也可以在 设置 → 导入 里进行。")
        }
        .accessibilityIdentifier("onboarding")
    }

    private func go(_ target: OnboardingStep?) {
        guard let target else { return }
        forward = target.rawValue > step.rawValue
        withAnimation(AeroricMotion.animation(0.22, reduceMotion: reduceMotion)) { step = target }
    }

    // MARK: - Pages

    @ViewBuilder private func page(_ step: OnboardingStep) -> some View {
        HStack(alignment: .center, spacing: AeroricSpace.huge) {
            VStack(alignment: .leading, spacing: AeroricSpace.xl) {
                TypewriterText(step.title, size: AeroricType.hero)
                    .accessibilityIdentifier("onboarding.title")
                content(step)
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            illustration(step)
        }
        .padding(AeroricSpace.huge + AeroricSpace.m)
    }

    @ViewBuilder private func content(_ step: OnboardingStep) -> some View {
        switch step {
        case .welcome:
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                point("rectangle.3.group", "在一个窗口里运行 Claude Code、Codex、DSH 和 oh-my-pi")
                point("eye", "会话、工具调用与审批都以原生界面呈现")
                point("lock.shield", "数据留在本机；读取你的已有会话前一定先问你")
            }
        case .agents:
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                ForEach(NativeHarnessPicker.harnesses) { harness in
                    let found = book.detected.first { $0.agent == harness.id }
                    HStack(spacing: AeroricSpace.m) {
                        AgentGlyph(agent: harness.id, size: 18)
                        Text(harness.title).font(.system(size: AeroricType.body, weight: .medium))
                        Spacer()
                        if let found, found.installed {
                            Label("已安装", systemImage: "checkmark.circle.fill")
                                .foregroundStyle(AeroricColor.success)
                                .help(found.path ?? "")
                        } else if book.detected.isEmpty {
                            ProgressView().controlSize(.small)
                        } else {
                            Text("未找到").foregroundStyle(AeroricColor.textTertiary)
                        }
                    }
                    .font(.system(size: AeroricType.secondary))
                    .padding(.vertical, AeroricSpace.xs)
                }
                Text("只检查可执行文件是否存在，不会运行它们。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .accessibilityIdentifier("onboarding.agents")
        case .projects:
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                LabeledContent("数据目录") {
                    Text(ImportPresentation.abbreviated(dataDirectory))
                        .font(.system(size: AeroricType.secondary, design: .monospaced))
                        .foregroundStyle(AeroricColor.textSecondary)
                }
                .font(.system(size: AeroricType.secondary))
                if projects.isEmpty {
                    Text("还没有打开项目。").foregroundStyle(AeroricColor.textTertiary)
                } else {
                    ForEach(projects.prefix(5)) { project in
                        Label(project.name, systemImage: "folder").font(.system(size: AeroricType.secondary))
                    }
                }
                Button("打开项目目录…", action: onOpenProject)
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted))
                    .accessibilityIdentifier("onboarding.open-project")
            }
        case .imports:
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                if !book.isAvailable {
                    Text("当前宿主不支持导入。").foregroundStyle(AeroricColor.textTertiary)
                }
                ForEach(OnboardingFlow.importRows(detected: book.detected.map(\.agent)), id: \.agent) { row in
                    importRow(row.agent, importable: row.importable && book.isAvailable)
                }
                Text("只读取、不复制、不修改；可在 设置 → 导入 中随时撤销。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                if let error = book.lastError {
                    Text(error).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.danger)
                }
            }
            .accessibilityIdentifier("onboarding.imports")
        case .done:
            Text("从侧栏的“新会话”开始第一个任务。导入的会话会出现在对应项目下，未归类的在“未归类导入”。")
                .font(.system(size: AeroricType.body))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func importRow(_ agent: String, importable: Bool) -> some View {
        let status = book.agentStatus(agent)
        let estimate = book.estimates[agent]
        return HStack(alignment: .top, spacing: AeroricSpace.m) {
            Toggle(isOn: Binding(
                get: { selectedAgents.contains(agent) || status?.granted == true },
                set: { on in if on { selectedAgents.insert(agent) } else { selectedAgents.remove(agent) } }
            )) { EmptyView() }
                .toggleStyle(.checkbox)
                .disabled(!importable || status?.granted == true || estimate?.rootExists == false)
            AgentGlyph(agent: agent, size: 16)
            VStack(alignment: .leading, spacing: 2) {
                HStack {
                    Text(ImportPresentation.agentTitle(agent)).font(.system(size: AeroricType.body, weight: .medium))
                    if !importable { ComingSoonBadge() }
                    if status?.granted == true {
                        Text("已导入 \(status?.sessions ?? 0) 个").font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.success)
                    }
                }
                if importable {
                    Text("\(ImportPresentation.abbreviated(estimate?.root ?? "")) · \(ImportPresentation.estimateText(estimate))")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
        }
        .accessibilityIdentifier("onboarding.import.\(agent)")
    }

    private func point(_ symbol: String, _ text: String) -> some View {
        Label {
            Text(text).font(.system(size: AeroricType.body)).fixedSize(horizontal: false, vertical: true)
        } icon: {
            Image(systemName: symbol).foregroundStyle(AeroricColor.accent)
        }
    }

    private func illustration(_ step: OnboardingStep) -> some View {
        ZStack {
            Circle().fill(AeroricColor.accent.opacity(0.10)).frame(width: 200, height: 200)
            Image(systemName: step.symbol)
                .font(.system(size: 64, weight: .light))
                .foregroundStyle(AeroricColor.accent)
                .padding(AeroricSpace.huge)
                .glassSurface(.floatingCard, in: AeroricRadius.shape(AeroricRadius.xl))
                .aeroricElevation(.floating, in: AeroricRadius.shape(AeroricRadius.xl))
        }
        .frame(width: 240)
        .accessibilityHidden(true)
    }

    // MARK: - Footer

    private var footer: some View {
        HStack(spacing: AeroricSpace.m) {
            if step != .done {
                Button("跳过引导") { confirmingSkip = true }
                    .buttonStyle(.link)
                    .accessibilityIdentifier("onboarding.skip")
            }
            Spacer()
            HStack(spacing: AeroricSpace.s) {
                ForEach(OnboardingStep.allCases) { item in
                    Circle()
                        .fill(item == step ? AeroricColor.text : AeroricColor.separator3)
                        .frame(width: 6, height: 6)
                }
            }
            .accessibilityHidden(true)
            Spacer()
            if let previous = step.previous {
                Button("上一步") { go(previous) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
            }
            primaryButton
        }
        .padding(.horizontal, AeroricSpace.xxxl)
        .frame(height: 56)
    }

    @ViewBuilder private var primaryButton: some View {
        switch step {
        case .imports:
            let pending = selectedAgents.filter { book.agentStatus($0)?.granted != true }
            Button(pending.isEmpty ? "下一步" : "导入所选（\(pending.count)）") {
                guard !pending.isEmpty else { return go(step.next) }
                importing = true
                Task {
                    for agent in pending.sorted() { await book.grant(agent) }
                    importing = false
                    selectedAgents.removeAll()
                    go(step.next)
                }
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.primary))
            .disabled(importing)
            .accessibilityIdentifier("onboarding.import")
        case .done:
            Button("开始使用", action: onFinish)
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("onboarding.finish")
        default:
            Button("下一步") { go(step.next) }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
                .accessibilityIdentifier("onboarding.next")
        }
    }
}
