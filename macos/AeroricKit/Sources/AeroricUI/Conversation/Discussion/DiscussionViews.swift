import AeroricProtocol
import SwiftUI

/// 2026-11-12: the words of the discussion page and its setup.
public enum DiscussionText {
    public static let minMembers = 2
    public static let maxMembers = 5
    public static let rounds = 1...5
    public static let defaultRounds = 3

    public static func active(_ d: LiveDiscussion) -> Bool {
        ["running", "pausing", "summarizing"].contains(d.state)
    }

    /// `第 2 / 3 轮 · 讨论中`.
    public static func state(_ d: LiveDiscussion) -> String {
        let round = "第 \(d.round) / \(d.maxRounds) 轮"
        switch d.state {
        case "running":
            return d.posts.contains { $0.state == "waiting" } ? "\(round) · 有成员在等你确认" : "\(round) · 讨论中"
        case "pausing": return "\(round) · 这一轮说完后暂停"
        case "paused": return "\(round) · 已暂停"
        case "summarizing": return "主持人在总结"
        case "done": return "已总结"
        case "stopped": return "已停止"
        default: return d.state
        }
    }

    public static func stance(_ stance: String?) -> String? {
        switch stance {
        case "keep": "维持"
        case "changed": "已修改"
        default: nil
        }
    }

    /// `41K tok · 52 秒 · ≈$0.012`.
    public static func usage(_ usage: LiveDiscussion.Usage?) -> String? {
        guard let usage else { return nil }
        var parts = ["\(SessionStatsText.compact(usage.tokens)) tok"]
        if let ms = usage.durationMs { parts.append(SessionStatsText.duration(ms)) }
        if let cost = usage.costUsd { parts.append("≈\(TurnBillText.money(cost))") }
        return parts.joined(separator: " · ")
    }

    /// `合计 182K tok · ≈$0.41`; nil before any post has usage.
    public static func total(_ d: LiveDiscussion) -> String? {
        let usages = d.posts.compactMap(\.usage)
        guard !usages.isEmpty else { return nil }
        let tokens = usages.reduce(UInt64(0)) { $0 + $1.tokens }
        let costs = usages.compactMap(\.costUsd)
        var text = "合计 \(SessionStatsText.compact(tokens)) tok"
        if !costs.isEmpty { text += " · ≈\(TurnBillText.money(costs.reduce(0, +)))" }
        return text
    }

    /// The rounds that have answers, in order.
    public static func rounds(_ d: LiveDiscussion) -> [Int] {
        Array(Set(d.posts.filter { $0.kind == "answer" }.map(\.round))).sorted()
    }

    /// A round's answers, one per member: a retry replaces the post it retried.
    public static func answers(_ d: LiveDiscussion, round: Int) -> [LiveDiscussion.Post] {
        let posts = d.posts.filter { $0.kind == "answer" && $0.round == round }
        return posts.filter { post in !posts.contains { $0.member == post.member && $0.index > post.index } }
    }

    public static func roundTitle(_ round: Int) -> String {
        round == 1 ? "第 1 轮 · 各自独立作答" : "第 \(round) 轮 · 互相回应"
    }

    /// Why the discussion cannot start yet; nil when it can.
    public static func blocked(topic: String, projectID: String?, members: [LiveDiscussionChoice]) -> String? {
        if projectID == nil { return "先选一个项目" }
        if members.count < minMembers { return "讨论至少要 \(minMembers) 个成员" }
        if members.count > maxMembers { return "讨论最多 \(maxMembers) 个成员" }
        if topic.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return TaskDraft.emptyPromptMessage }
        return nil
    }

    /// The harnesses the add panel can pick: those with a startable configuration.
    public static func harnesses(_ targets: [RerunTarget]) -> [String: Bool] {
        Dictionary(uniqueKeysWithValues: NewSessionLayout.harnessOrder.map { harness in
            (harness, targets.contains { $0.agent == harness })
        })
    }

    /// Where the add panel opens: the single-session harness when it can start a member.
    public static func startHarness(preferred: String, targets: [RerunTarget]) -> String {
        NewSessionLayout.defaultHarness(preferred: preferred, availability: harnesses(targets))
    }

    public static func configurations(_ targets: [RerunTarget], harness: String) -> [RerunTarget] {
        targets.filter { $0.agent == harness }
    }

    /// A configuration's models; one without a list offers its default model (nil).
    public static func models(_ target: RerunTarget) -> [String?] {
        target.models.isEmpty ? [nil] : target.models
    }

    /// How many members already use this configuration and model.
    public static func added(_ members: [LiveDiscussionChoice], configurationID: String, model: String?) -> Int {
        members.filter { $0.configurationId == configurationID && $0.model == model }.count
    }

    /// A member's name on the setup row.
    public static func member(_ choice: LiveDiscussionChoice, targets: [RerunTarget]) -> String {
        guard let target = targets.first(where: { $0.id == choice.configurationId }) else {
            return choice.configurationId
        }
        let title = RerunText.title(target, among: targets)
        return choice.model.map { "\(title) · \($0)" } ?? title
    }
}

// MARK: - Setup on the new-session page

/// 2–5 members, each a configuration and model with an optional angle; one
/// is the moderator; 1–5 rounds.
struct DiscussionSetup: View {
    let targets: [RerunTarget]
    /// The harness the add panel opens on: the one picked for a single session.
    var preferredHarness = "codex"
    @Binding var members: [LiveDiscussionChoice]
    @Binding var moderator: Int
    @Binding var rounds: Int
    var isEditable = true
    @State private var picking = false
    @State private var pickedHarness: String?
    @State private var pickedConfiguration: String?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text("成员（\(DiscussionText.minMembers)–\(DiscussionText.maxMembers)）")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            ForEach(Array(members.enumerated()), id: \.element.id) { index, member in
                row(index, member).transition(appear)
            }
            if members.count < DiscussionText.maxMembers {
                addMenu
            }
            HStack(spacing: AeroricSpace.xs) {
                Text("轮数")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                ForEach(Array(DiscussionText.rounds), id: \.self) { n in
                    Button("\(n)") { rounds = n }
                        .buttonStyle(AeroricChipStyle(selected: rounds == n))
                        .accessibilityAddTraits(rounds == n ? .isSelected : [])
                        .accessibilityIdentifier("discussion.rounds.\(n)")
                }
                Spacer(minLength: AeroricSpace.s)
                Text("只读 · 从第 2 轮起全员维持就提前总结")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
            }
            .padding(.top, AeroricSpace.xs)
        }
        .disabled(!isEditable)
        .focusEffectDisabled()
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.setup")
    }

    private func row(_ index: Int, _ member: LiveDiscussionChoice) -> some View {
        HStack(spacing: AeroricSpace.s) {
            Text(DiscussionText.member(member, targets: targets))
                .font(.system(size: AeroricType.body, weight: .medium))
                .lineLimit(1)
            TextField("角度（可选），如 反对方、关注性能", text: Binding(
                get: { members.indices.contains(index) ? members[index].angle : "" },
                set: { if members.indices.contains(index) { members[index].angle = $0 } }))
                .textFieldStyle(.plain)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityIdentifier("discussion.member.\(index).angle")
            Button(index == moderator ? "主持人" : "设为主持人") { moderator = index }
                .buttonStyle(AeroricChipStyle(selected: index == moderator))
                .accessibilityIdentifier("discussion.member.\(index).moderator")
            Button {
                animate {
                    members.remove(at: index)
                    if moderator >= members.count { moderator = max(0, members.count - 1) }
                    else if index < moderator { moderator -= 1 }
                }
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: AeroricType.caption, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("移除成员")
            .accessibilityIdentifier("discussion.member.\(index).remove")
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 28)
        .background(Capsule(style: .continuous).fill(AeroricColor.layer1))
        .overlay(Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.member.\(index)")
    }

    /// 添加成员 opens a panel below it, three steps from coarse to fine:
    /// the harness slider, that harness's configurations, then its models;
    /// a model adds a member and the panel stays open until 完成 or the
    /// fifth member. Plain buttons, not a Menu: a SwiftUI Menu does not open
    /// on AXPress while the app is in the background.
    private var addMenu: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            Button {
                animate { picking.toggle() }
            } label: {
                Label(picking ? "完成" : "添加成员", systemImage: picking ? "checkmark" : "plus")
                    .font(.system(size: AeroricType.body))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(maxWidth: .infinity, minHeight: 28, alignment: .leading)
                    .overlay(Capsule(style: .continuous)
                        .strokeBorder(AeroricColor.separator2, style: StrokeStyle(lineWidth: 0.5, dash: [3, 3])))
                    .contentShape(Capsule(style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("discussion.add")
            if picking {
                picker.transition(appear)
            }
        }
    }

    private var harness: String {
        pickedHarness ?? DiscussionText.startHarness(preferred: preferredHarness, targets: targets)
    }

    /// The picked configuration of the harness, else its first one.
    private var configuration: RerunTarget? {
        let shown = DiscussionText.configurations(targets, harness: harness)
        return shown.first { $0.id == pickedConfiguration } ?? shown.first
    }

    private var picker: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            if targets.isEmpty {
                Text("这个项目的配置目录还没读到，或没有能启动的配置")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .accessibilityIdentifier("discussion.noTargets")
            } else {
                step("Harness", height: 28) { harnessSlider }
                step("配置", height: 20) {
                    // Names are told apart within the harness: the slider already says which one.
                    let shown = DiscussionText.configurations(targets, harness: harness)
                    FlowLayout(spacing: AeroricSpace.xs) {
                        ForEach(shown) { target in
                            Button(RerunText.title(target, among: shown)) {
                                animate { pickedConfiguration = target.id }
                            }
                            .buttonStyle(AeroricChipStyle(selected: target.id == configuration?.id))
                            .accessibilityAddTraits(target.id == configuration?.id ? .isSelected : [])
                            .accessibilityIdentifier("discussion.target.\(target.id)")
                        }
                    }
                }
                if let configuration {
                    step("模型", height: 20) { models(configuration) }
                        .id(configuration.id)
                        .transition(.opacity)
                }
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .padding(.vertical, AeroricSpace.m)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(AeroricColor.layer1))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous)
            .strokeBorder(AeroricColor.separator1, lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.picker")
    }

    private var harnessSlider: some View {
        let usable = DiscussionText.harnesses(targets)
        return SlidingCapsulePicker(
            selection: Binding(get: { harness }, set: { value in
                animate {
                    pickedHarness = value
                    pickedConfiguration = nil
                }
            }),
            groupTitle: "成员 Harness", titleMode: .selectedOnly, identifier: "discussion.harness",
            segments: NewSessionLayout.harnessOrder.map { id in
                let title = AgentConfigText.harnessTitle(id)
                let enabled = usable[id] ?? false
                return .init(value: id, title: title,
                             asset: NativeHarnessPicker.harnesses.first { $0.id == id }?.asset,
                             isEnabled: enabled, help: enabled ? title : "\(title)：没有能启动的配置")
            }
        )
        .fixedSize()
    }

    /// A configuration's models as chips; one used already shows how often.
    private func models(_ target: RerunTarget) -> some View {
        FlowLayout(spacing: AeroricSpace.xs) {
            ForEach(DiscussionText.models(target), id: \.self) { model in
                let count = DiscussionText.added(members, configurationID: target.id, model: model)
                Button {
                    add(target.id, model)
                } label: {
                    Label(count > 0 ? "\(model ?? "默认模型") ×\(count)" : model ?? "默认模型",
                          systemImage: "plus")
                }
                .buttonStyle(AeroricChipStyle(selected: false))
                .disabled(members.count >= DiscussionText.maxMembers)
                .accessibilityIdentifier("discussion.model.\(target.id).\(model ?? "default")")
            }
        }
    }

    /// A step of the panel: its name on the left, centred on the first line.
    private func step(_ title: String, height: CGFloat, @ViewBuilder content: () -> some View) -> some View {
        HStack(alignment: .top, spacing: AeroricSpace.m) {
            Text(title)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .frame(width: 52, height: height, alignment: .leading)
            content()
        }
    }

    private func add(_ configurationID: String, _ model: String?) {
        guard members.count < DiscussionText.maxMembers else { return }
        animate {
            members.append(LiveDiscussionChoice(configurationId: configurationID, model: model))
            if members.count >= DiscussionText.maxMembers { picking = false }
        }
    }

    private func animate(_ change: () -> Void) {
        if reduceMotion { change() } else { withAnimation(SlidingCapsulePicker<Bool>.slide, change) }
    }

    private var appear: AnyTransition {
        reduceMotion ? .opacity : .opacity.combined(with: .offset(y: -AeroricSpace.s))
    }
}

// MARK: - The discussion page

/// What the discussion page may ask of the host.
@MainActor
public struct DiscussionActions {
    public var load: (String) async throws -> LiveDiscussion?
    /// `pause` / `resume` / `summarize` / `stop`.
    public var act: (String, String) async throws -> LiveDiscussion?
    /// Open a member's session.
    public var open: (String) -> Void
    /// A new-session draft in the project, prefilled with the text.
    public var newSession: (String, String) -> Void
    /// The discussion's state changed: the sidebar reads its record again
    /// (the host changes that record without telling the app).
    public var changed: () async -> Void

    public init(load: @escaping (String) async throws -> LiveDiscussion?,
                act: @escaping (String, String) async throws -> LiveDiscussion?,
                open: @escaping (String) -> Void, newSession: @escaping (String, String) -> Void,
                changed: @escaping () async -> Void = {}) {
        self.load = load
        self.act = act
        self.open = open
        self.newSession = newSession
        self.changed = changed
    }
}

/// Posts by round, each member a card; the moderator's summary last.
public struct DiscussionView: View {
    let taskID: String
    let actions: DiscussionActions
    @State private var discussion: LiveDiscussion?
    @State private var error: String?
    @State private var busy = false

    public init(taskID: String, actions: DiscussionActions) {
        self.taskID = taskID
        self.actions = actions
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                if let discussion {
                    header(discussion)
                    if let note = discussion.note, discussion.state == "paused" {
                        Label(note, systemImage: "pause.circle")
                            .font(.system(size: AeroricType.secondary))
                            .foregroundStyle(AeroricColor.warning)
                            .accessibilityIdentifier("discussion.note")
                    }
                    if let error { CapsuleErrorLine(error, identifier: "discussion.error") { self.error = nil } }
                    ForEach(DiscussionText.rounds(discussion), id: \.self) { round in
                        roundSection(discussion, round)
                    }
                    ForEach(discussion.posts.filter { $0.kind == "summary" }) { post in
                        summaryCard(discussion, post)
                    }
                } else if let error {
                    CapsuleErrorLine(error, identifier: "discussion.error") { self.error = nil }
                } else {
                    ProgressView().frame(maxWidth: .infinity)
                }
            }
            .padding(AeroricSpace.xl)
            .frame(maxWidth: 960, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .background(AeroricColor.canvas)
        .task(id: taskID) { await poll() }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.page")
    }

    /// Read it now and, while it goes on, every few seconds.
    private func poll() async {
        while !Task.isCancelled {
            do {
                let before = discussion?.state
                discussion = try await actions.load(taskID)
                if let before, before != discussion?.state { await actions.changed() }
            } catch {
                self.error = String(describing: error)
            }
            let wait: UInt64 = discussion.map(DiscussionText.active) == true ? 3 : 15
            try? await Task.sleep(nanoseconds: wait * 1_000_000_000)
        }
    }

    private func act(_ action: String) {
        busy = true
        Task {
            do {
                if let updated = try await actions.act(taskID, action) { discussion = updated }
                error = nil
            } catch {
                self.error = AgentConfigStore.describe(error)
            }
            busy = false
        }
    }

    private func header(_ d: LiveDiscussion) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                Text(d.topic.split(separator: "\n").first.map(String.init) ?? "讨论")
                    .font(.system(size: AeroricType.headline, weight: .semibold))
                    .lineLimit(2)
                    .textSelection(.enabled)
                Spacer(minLength: AeroricSpace.m)
                controls(d)
            }
            HStack(spacing: AeroricSpace.s) {
                discussionChip(DiscussionText.state(d), tint: DiscussionText.active(d) ? AeroricColor.accent : AeroricColor.textSecondary)
                    .accessibilityIdentifier("discussion.state")
                Text(d.members.map(\.label).joined(separator: "、"))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
                Spacer(minLength: AeroricSpace.s)
                if let total = DiscussionText.total(d) {
                    Text(total)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .accessibilityIdentifier("discussion.total")
                }
            }
        }
    }

    @ViewBuilder private func controls(_ d: LiveDiscussion) -> some View {
        HStack(spacing: AeroricSpace.xs) {
            switch d.state {
            case "running":
                control("暂停", "pause", help: "这一轮说完后暂停")
            case "pausing", "paused":
                control("继续", "resume", help: "接着讨论；这一轮没说完的成员会再问一次")
            default:
                EmptyView()
            }
            if ["running", "pausing", "paused"].contains(d.state),
               d.posts.contains(where: { $0.state == "done" }) {
                control("立即总结", "summarize", help: "打断还在发言的成员，请主持人现在总结")
            }
            if !["done", "stopped"].contains(d.state) {
                control("停止", "stop", help: "打断所有成员，结束讨论")
            }
        }
        .disabled(busy)
    }

    private func control(_ title: String, _ action: String, help: String) -> some View {
        Button(title) { act(action) }
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .help(help)
            .accessibilityIdentifier("discussion.\(action)")
    }

    private func roundSection(_ d: LiveDiscussion, _ round: Int) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text(DiscussionText.roundTitle(round))
                .font(.system(size: AeroricType.caption, weight: .medium))
                .foregroundStyle(AeroricColor.textTertiary)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 280), spacing: AeroricSpace.m, alignment: .top)],
                      alignment: .leading, spacing: AeroricSpace.m) {
                ForEach(DiscussionText.answers(d, round: round)) { post in
                    PostCard(post: post, member: d.members[safe: post.member], onOpen: actions.open)
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.round.\(round)")
    }

    private func summaryCard(_ d: LiveDiscussion, _ post: LiveDiscussion.Post) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                Label("主持人总结 · \(d.members[safe: post.member]?.label ?? "")", systemImage: "doc.text")
                    .font(.system(size: AeroricType.secondary, weight: .semibold))
                Spacer()
                if let usage = DiscussionText.usage(post.usage) {
                    Text(usage).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                }
            }
            PostBody(post: post)
            if post.state == "done" {
                HStack {
                    Button("从结论新建会话") { actions.newSession(d.projectId, post.text) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                        .help("用这份结论预填新会话的输入框，不会自动开始")
                        .accessibilityIdentifier("discussion.newSession")
                    Spacer()
                }
            }
        }
        .padding(AeroricSpace.l)
        .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(AeroricColor.accent.opacity(0.45)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.summary")
    }
}

private func discussionChip(_ text: String, tint: Color) -> some View {
    Text(text)
        .font(.system(size: AeroricType.caption, weight: .medium))
        .foregroundStyle(tint)
        .padding(.horizontal, AeroricSpace.s)
        .frame(height: 20)
        .background(Capsule(style: .continuous).fill(tint.opacity(0.14)))
}

/// One member's answer in a round.
private struct PostCard: View {
    let post: LiveDiscussion.Post
    let member: LiveDiscussion.Member?
    let onOpen: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                HarnessLogo(harness: member?.family, agent: member?.agent, size: 14)
                Text(member?.label ?? "成员")
                    .font(.system(size: AeroricType.secondary, weight: .medium))
                    .lineLimit(1)
                Spacer(minLength: AeroricSpace.xs)
                status
                if let member {
                    Button { onOpen(member.taskId) } label: {
                        Image(systemName: "arrow.up.forward.square")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                    }
                    .buttonStyle(.plain)
                    .help("打开成员会话")
                    .accessibilityLabel("打开成员会话")
                }
            }
            if let angle = member?.angle, post.round == 1 {
                Text("角度：\(angle)")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            if post.state == "waiting" {
                Text(post.waiting ?? "在等你确认")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(3)
                if let member {
                    Button("打开成员会话") { onOpen(member.taskId) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                        .accessibilityIdentifier("discussion.openWaiting")
                }
            }
            PostBody(post: post)
            if let usage = DiscussionText.usage(post.usage) {
                Text(usage)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
        }
        .padding(AeroricSpace.l)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(AeroricColor.separator1, lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.post.\(post.index)")
    }

    @ViewBuilder private var status: some View {
        switch post.state {
        case "speaking": discussionChip("发言中", tint: AeroricColor.accent)
        case "waiting": discussionChip("在等你确认", tint: AeroricColor.danger)
        case "failed": discussionChip("没说完", tint: AeroricColor.warning)
        default:
            if let stance = DiscussionText.stance(post.stance) {
                discussionChip(stance, tint: post.stance == "keep" ? AeroricColor.success : AeroricColor.warning)
            }
        }
    }
}

/// A post's words, folded past a few lines; a failed one shows why.
private struct PostBody: View {
    let post: LiveDiscussion.Post
    @State private var unfolded = false
    private static let foldedLines = 10

    /// Inline Markdown (bold, code, links) with the line breaks kept.
    static func rendered(_ text: String) -> AttributedString {
        (try? AttributedString(markdown: text, options: .init(
            interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            if let error = post.error, post.state == "failed" {
                Text(error)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.warning)
            }
            if !post.text.isEmpty {
                Text(Self.rendered(post.text))
                    .font(.system(size: AeroricType.secondary))
                    .textSelection(.enabled)
                    .lineLimit(unfolded ? nil : Self.foldedLines)
                    .fixedSize(horizontal: false, vertical: true)
                if post.text.split(separator: "\n", omittingEmptySubsequences: false).count > Self.foldedLines
                    || post.text.count > 400 {
                    Button(unfolded ? "收起" : "展开全文") { unfolded.toggle() }
                        .buttonStyle(.plain)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.accent)
                }
            } else if post.state == "speaking" {
                ProgressView().controlSize(.small)
            }
        }
    }
}

private extension Array {
    subscript(safe index: Int) -> Element? { indices.contains(index) ? self[index] : nil }
}
