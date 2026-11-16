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

    /// 2026-11-13: how the members work together; 自动 follows the topic.
    public enum FormatChoice: String, CaseIterable, Sendable {
        case auto, roundtable, review, research
    }

    /// 2026-11-13: what every post should be like, and the presets that
    /// only fill the text in.
    public static let defaultRequirements = "观点要具体、说明理由，尽量在 600 字以内。"
    public static let requirementPresets: [(title: String, text: String)] = [
        ("简洁讨论", "先给结论，再给最关键的一两条理由，每次 300 字以内。"),
        ("方案对比", "列出可选方案，逐项比较效果、代价和适用条件，最后给出推荐。"),
        ("深入分析", "讲清原理和依据，引用代码或资料佐证，篇幅不限。"),
    ]

    public static let revisions = 1...3
    public static let defaultRevisions = 2

    /// A plain rule, no model: a topic about writing something goes to
    /// draft review, anything else to the round table.
    /// Whichever kind of word comes first in the topic wins. English
    /// words match case-insensitively and only as whole words.
    public static func recommendFormat(_ topic: String) -> (format: String, reason: String) {
        let kinds: [(format: String, words: [String], reason: String)] = [
            ("review", ["起草", "写一份", "写一篇", "文档", "提案", "稿",
                        "draft", "write a", "write an", "proposal"], "一人起草，其他人评审"),
            ("research", ["为什么", "排查", "调研", "原因", "瓶颈",
                          "why", "research", "investigate", "root cause", "bottleneck"],
             "主持人拆题，各自调研，最后出报告"),
        ]
        var best: (at: String.Index, format: String, word: String, reason: String)?
        for kind in kinds {
            for word in kind.words {
                if let range = Self.find(word, in: topic), best.map({ range.lowerBound < $0.at }) ?? true {
                    best = (range.lowerBound, kind.format, word, kind.reason)
                }
            }
        }
        if let best { return (best.format, "题目里有「\(best.word)」：\(best.reason)") }
        return ("roundtable", "没有起草或调研类的词：各自作答、互相回应、最后总结")
    }

    /// Chinese words match anywhere; English ones only as whole words, so
    /// "why" does not hit "whyever" and "draft" not "redrafted".
    private static func find(_ word: String, in topic: String) -> Range<String.Index>? {
        guard word.unicodeScalars.allSatisfy(\.isASCII) else { return topic.range(of: word) }
        let pattern = "\\b" + NSRegularExpression.escapedPattern(for: word) + "\\b"
        return topic.range(of: pattern, options: [.regularExpression, .caseInsensitive])
    }

    public static func format(_ choice: FormatChoice, topic: String) -> String {
        choice == .auto ? recommendFormat(topic).format : choice.rawValue
    }

    public static func formatTitle(_ format: String) -> String {
        switch format {
        case "review": "起草评审"
        case "research": "分工调研"
        default: "圆桌"
        }
    }

    public static func isReview(_ d: LiveDiscussion) -> Bool { d.format == "review" }
    public static func isResearch(_ d: LiveDiscussion) -> Bool { d.format == "research" }

    /// The version a draft-review round writes or reviews.
    public static func version(_ round: Int) -> Int { (round + 1) / 2 }

    /// `第 2 / 3 轮 · 讨论中`; a draft review says which version is where.
    public static func state(_ d: LiveDiscussion) -> String {
        if isReview(d) { return reviewState(d) }
        if isResearch(d) { return researchState(d) }
        let round = "第 \(d.round) / \(d.maxRounds) 轮"
        switch d.state {
        case "running":
            return d.posts.contains { $0.state == "waiting" } ? "\(round) · 有成员在等你确认" : "\(round) · 讨论中"
        case "pausing": return "\(round) · 这一轮说完后暂停"
        case "paused" where replying(d): return "\(round) · 已暂停 · 有成员在回答你"
        case "paused": return "\(round) · 已暂停"
        case "summarizing": return "主持人在总结"
        case "done" where replying(d): return "已总结 · 有成员在回答你"
        case "done": return "已总结"
        case "stopped": return "已停止"
        default: return d.state
        }
    }

    private static func reviewState(_ d: LiveDiscussion) -> String {
        let at = "第 \(version(d.round)) 版"
        switch d.state {
        case "running":
            if d.posts.contains(where: { $0.state == "waiting" }) { return "\(at) · 有成员在等你确认" }
            if d.round % 2 == 0 { return "\(at) · 评审中" }
            return d.round == 1 ? "\(at) · 作者起草中" : "\(at) · 作者修订中"
        case "pausing": return "\(at) · 这一步写完后暂停"
        case "paused" where replying(d): return "\(at) · 已暂停 · 有成员在回答你"
        case "paused": return "\(at) · 已暂停"
        case "summarizing": return "写完这一步就定稿"
        case "done" where replying(d): return "已定稿 · 有成员在回答你"
        case "done": return "已定稿"
        case "stopped": return "已停止"
        default: return d.state
        }
    }

    private static func researchState(_ d: LiveDiscussion) -> String {
        let step = d.round <= 1 ? "拆题" : "调研"
        switch d.state {
        case "running":
            if d.posts.contains(where: { $0.state == "waiting" }) { return "\(step) · 有成员在等你确认" }
            return d.round <= 1 ? "主持人拆题中" : "成员各自调研中"
        case "pausing": return "\(step) · 这一步做完后暂停"
        case "paused" where replying(d): return "\(step) · 已暂停 · 有成员在回答你"
        case "paused": return "\(step) · 已暂停"
        case "summarizing": return "主持人写报告中"
        case "done" where replying(d): return "已出报告 · 有成员在回答你"
        case "done": return "已出报告"
        case "stopped": return "已停止"
        default: return d.state
        }
    }

    /// 拆题 → 调研 2/3 查完 → 报告.
    public static func researchSteps(_ d: LiveDiscussion) -> [ReviewStep] {
        var steps: [ReviewStep] = []
        if let split = answers(d, round: 1).first {
            steps.append(.init(title: "拆题", finished: split.state == "done"))
        }
        let parts = answers(d, round: 2)
        if !parts.isEmpty {
            let done = parts.filter { $0.state == "done" }.count
            let total = max(parts.count, d.members.count - 1)
            steps.append(.init(title: "调研 \(done)/\(total) 查完", finished: done == total))
        }
        if let report = d.posts.last(where: { $0.kind == "summary" }) {
            steps.append(.init(title: "报告", finished: report.state == "done"))
        }
        return steps
    }

    /// Research: a member's part, as the moderator split it.
    public static func assignment(_ d: LiveDiscussion, member: Int) -> String? {
        guard isResearch(d), d.assignments.indices.contains(member) else { return nil }
        let part = d.assignments[member].trimmingCharacters(in: .whitespacesAndNewlines)
        return part.isEmpty ? nil : part
    }

    /// After a research report only `@` works: there is no further round.
    public static func needsMention(_ d: LiveDiscussion) -> Bool {
        isResearch(d) && d.state == "done"
    }

    public static func verdict(_ verdict: String?) -> String? {
        switch verdict {
        case "pass": "通过"
        case "revise": "需修改"
        default: nil
        }
    }

    /// The final card's title: which version, whether everyone passed it.
    public static func finalTitle(_ post: LiveDiscussion.Post) -> String {
        let v = post.version ?? 1
        let passed = post.verdict == "pass" ? "全部通过" : "没有全部通过"
        return "定稿 · 第 \(v) 版 · \(passed) · 修订 \(max(0, v - 1)) 次"
    }

    /// One step on a draft review's progress line.
    public struct ReviewStep: Hashable, Sendable {
        public var title: String
        public var finished: Bool
    }

    /// 第 1 版 起草 → 评审 2/3 通过 → 第 2 版 修订 → … → 定稿.
    public static func reviewSteps(_ d: LiveDiscussion) -> [ReviewStep] {
        var steps: [ReviewStep] = []
        for round in rounds(d) {
            let said = answers(d, round: round)
            guard !said.isEmpty else { continue }
            if round % 2 == 1 {
                let title = "第 \(version(round)) 版 \(round == 1 ? "起草" : "修订")"
                steps.append(.init(title: title, finished: said.allSatisfy { $0.state == "done" }))
            } else {
                let passed = said.filter { $0.verdict == "pass" }.count
                steps.append(.init(title: "评审 \(passed)/\(said.count) 通过",
                                   finished: said.allSatisfy { $0.state == "done" }))
            }
            if summary(d, round: round)?.stage == "final" {
                steps.append(.init(title: "定稿", finished: true))
            }
        }
        return steps
    }

    /// `@` is off while the reviewers are at work: a named reviewer would
    /// see the others' reviews.
    public static func canMention(_ d: LiveDiscussion) -> Bool {
        let alone = (isReview(d) && d.round % 2 == 0) || (isResearch(d) && d.round == 2)
        return !(alone && ["running", "pausing"].contains(d.state))
    }

    /// The same members, to start another discussion with.
    public static func choices(_ d: LiveDiscussion) -> [LiveDiscussionChoice] {
        d.members.map { LiveDiscussionChoice(configurationId: $0.configurationId, model: $0.model, angle: $0.angle ?? "") }
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

    /// The rounds with anything said in them, in order.
    public static func rounds(_ d: LiveDiscussion) -> [Int] {
        Array(Set(d.posts.map(\.round))).sorted()
    }

    /// 2026-11-13: what the person said in a round and the answers to their
    /// questions, in order.
    public static func exchanges(_ d: LiveDiscussion, round: Int) -> [LiveDiscussion.Post] {
        d.posts.filter { ["user", "reply"].contains($0.kind) && $0.round == round }
    }

    /// The summary that closed a round: a retry replaces the one it retried.
    public static func summary(_ d: LiveDiscussion, round: Int) -> LiveDiscussion.Post? {
        d.posts.last { $0.kind == "summary" && $0.round == round }
    }

    /// A member asked by name is still answering.
    public static func replying(_ d: LiveDiscussion) -> Bool {
        d.posts.contains { $0.kind == "reply" && ["speaking", "waiting"].contains($0.state) }
    }

    /// Someone is talking: a round, a summary, or an answer to the person.
    public static func busy(_ d: LiveDiscussion) -> Bool {
        d.posts.contains { ["speaking", "waiting"].contains($0.state) }
    }

    /// The person can speak unless the moderator is summing up or it stopped.
    public static func canSay(_ d: LiveDiscussion) -> Bool {
        !["summarizing", "stopped"].contains(d.state)
    }

    /// 再来一轮: paused or summed up, nobody talking, something said.
    public static func canAgain(_ d: LiveDiscussion) -> Bool {
        !isResearch(d) && ["paused", "done"].contains(d.state) && !busy(d) && d.posts.contains { $0.state == "done" }
    }

    public static func sayPlaceholder(_ d: LiveDiscussion) -> String {
        if isResearch(d) {
            return d.state == "done" ? "已出报告：只能 @成员 追问；想接着聊用报告卡上的按钮"
                : "说点什么，主持人写报告时会看到；调研中不能点名"
        }
        return ["paused", "done"].contains(d.state)
            ? "说点什么会再开一轮，然后重新总结；@成员 只问他一个"
            : "说点什么，下一轮所有成员都会看到；@成员 只问他一个"
    }

    /// What members call each other in an anonymous discussion.
    public static func alias(_ index: Int) -> String {
        "成员 \(Character(UnicodeScalar(UInt8(65 + index % 26))))"
    }

    /// The members named in `text` as `@名字` (or `@成员 A` when anonymous).
    /// Longer names are taken first, so `@Codex · ab` is not also `@Codex · a`.
    public static func mentions(in text: String, _ d: LiveDiscussion) -> [Int] {
        var rest = text
        var found: Set<Int> = []
        let names = d.members.indices.flatMap { i in
            [(i, d.members[i].label)] + (d.anonymous ? [(i, alias(i))] : [])
        }.sorted { $0.1.count > $1.1.count }
        for (i, name) in names where !name.isEmpty {
            let token = "@\(name)"
            if rest.contains(token) {
                found.insert(i)
                rest = rest.replacingOccurrences(of: token, with: " ")
            }
        }
        return found.sorted()
    }

    /// A round's answers, one per member: a retry replaces the post it retried.
    public static func answers(_ d: LiveDiscussion, round: Int) -> [LiveDiscussion.Post] {
        let posts = d.posts.filter { $0.kind == "answer" && $0.round == round }
        return posts.filter { post in !posts.contains { $0.member == post.member && $0.index > post.index } }
    }

    public static func roundTitle(_ round: Int) -> String {
        round == 1 ? "第 1 轮 · 各自独立作答" : "第 \(round) 轮 · 互相回应"
    }

    public static func roundTitle(_ d: LiveDiscussion, _ round: Int) -> String {
        if isResearch(d) { return round == 1 ? "主持人拆题" : "各自调研（互相看不到）" }
        guard isReview(d) else { return roundTitle(round) }
        if round % 2 == 0 { return "第 \(version(round)) 版 · 评审（各自独立，互相看不到）" }
        return round == 1 ? "第 1 版 · 作者起草" : "第 \(version(round)) 版 · 作者修订"
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

/// 2026-11-13: a discussion to set up on the new-session page with these
/// members (围绕这份稿子讨论); the topic goes in the draft.
public struct DiscussionPrefill: Equatable, Sendable {
    public let id = UUID()
    public var members: [LiveDiscussionChoice]
    public var moderator: Int
    public var format: DiscussionText.FormatChoice

    public init(members: [LiveDiscussionChoice], moderator: Int, format: DiscussionText.FormatChoice = .roundtable) {
        self.members = members
        self.moderator = moderator
        self.format = format
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
    /// 2026-11-13: members call each other 成员 A / B.
    @Binding var anonymous: Bool
    /// 2026-11-13: round table or draft review, and the topic 自动 reads.
    var topic = ""
    @Binding var format: DiscussionText.FormatChoice
    @Binding var revisions: Int
    /// 2026-11-13: what every post should be like.
    @Binding var requirements: String
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
            formatRow
            requirementsRow
            HStack(spacing: AeroricSpace.xs) {
                if research {
                    Text("没有轮数：主持人拆题 → 各自调研 → 主持人写报告")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                } else if review {
                    Text("最多修订")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                    ForEach(Array(DiscussionText.revisions), id: \.self) { n in
                        Button("\(n) 次") { revisions = n }
                            .buttonStyle(AeroricChipStyle(selected: revisions == n))
                            .accessibilityAddTraits(revisions == n ? .isSelected : [])
                            .accessibilityIdentifier("discussion.revisions.\(n)")
                    }
                } else {
                    Text("轮数")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                    ForEach(Array(DiscussionText.rounds), id: \.self) { n in
                        Button("\(n)") { rounds = n }
                            .buttonStyle(AeroricChipStyle(selected: rounds == n))
                            .accessibilityAddTraits(rounds == n ? .isSelected : [])
                            .accessibilityIdentifier("discussion.rounds.\(n)")
                    }
                }
                Button("匿名互评") { anonymous.toggle() }
                    .buttonStyle(AeroricChipStyle(selected: anonymous))
                    .accessibilityAddTraits(anonymous ? .isSelected : [])
                    .help("成员之间只用 成员 A / B 称呼，看不到彼此是哪个 Agent 和模型；开始后不能改")
                    .accessibilityIdentifier("discussion.anonymous")
                    .padding(.leading, AeroricSpace.s)
                Spacer(minLength: AeroricSpace.s)
                Text(research ? "只读" : review ? "只读 · 全部通过或次数用完就定稿" : "只读 · 从第 2 轮起全员维持就提前总结")
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
            Button(index == moderator ? (review ? "作者" : "主持人") : (review ? "设为作者" : "设为主持人")) {
                moderator = index
            }
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

    private var review: Bool { DiscussionText.format(format, topic: topic) == "review" }
    private var research: Bool { DiscussionText.format(format, topic: topic) == "research" }

    /// 讨论要求: editable; a preset only fills the text in; empty asks nothing.
    private var requirementsRow: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xxs) {
            HStack(spacing: AeroricSpace.xs) {
                Text("讨论要求")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                ForEach(DiscussionText.requirementPresets, id: \.title) { preset in
                    Button(preset.title) { requirements = preset.text }
                        .buttonStyle(AeroricChipStyle(selected: requirements == preset.text))
                        .accessibilityIdentifier("discussion.requirements.\(preset.title)")
                }
                Button("默认") { requirements = DiscussionText.defaultRequirements }
                    .buttonStyle(AeroricChipStyle(selected: requirements == DiscussionText.defaultRequirements))
                    .accessibilityIdentifier("discussion.requirements.default")
            }
            TextField("清空就不附加要求；只读这条始终附加", text: $requirements, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: AeroricType.caption))
                .lineLimit(1...3)
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.xs + 1)
                .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous)
                    .strokeBorder(AeroricColor.separator1, lineWidth: 0.5))
                .accessibilityIdentifier("discussion.requirements")
            if review {
                Text("作者写稿不受这里的篇幅限制，只用于评审意见")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
        }
    }

    /// 方式: 自动 (by the topic's words), 圆桌 or 起草评审.
    private var formatRow: some View {
        let recommended = DiscussionText.recommendFormat(topic)
        return VStack(alignment: .leading, spacing: AeroricSpace.xxs) {
            HStack(spacing: AeroricSpace.xs) {
                Text("方式")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                ForEach(DiscussionText.FormatChoice.allCases, id: \.self) { choice in
                    Button(choice == .auto ? "自动 · \(DiscussionText.formatTitle(recommended.format))"
                           : DiscussionText.formatTitle(choice.rawValue)) {
                        animate { format = choice }
                    }
                    .buttonStyle(AeroricChipStyle(selected: format == choice))
                    .accessibilityAddTraits(format == choice ? .isSelected : [])
                    .accessibilityIdentifier("discussion.format.\(choice.rawValue)")
                }
            }
            Text(format == .auto ? recommended.reason
                 : review ? "作者写，其他成员各自评审，作者看全部意见再改"
                 : research ? "主持人拆题，每位成员只查自己那一块，主持人写报告"
                 : "各自作答、互相回应，主持人最后总结")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .accessibilityIdentifier("discussion.format.reason")
        }
        .padding(.top, AeroricSpace.xs)
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
    /// 2026-11-13: the person speaks (task, text, members asked by name).
    public var say: (String, String, [Int]) async throws -> LiveDiscussion?
    /// 2026-11-13: a new discussion in the project about this text, with
    /// these members and moderator, in this format (围绕这份稿子讨论,
    /// 据此起草一份文稿).
    public var discuss: (String, String, [LiveDiscussionChoice], Int, DiscussionText.FormatChoice) -> Void

    public init(load: @escaping (String) async throws -> LiveDiscussion?,
                act: @escaping (String, String) async throws -> LiveDiscussion?,
                open: @escaping (String) -> Void, newSession: @escaping (String, String) -> Void,
                changed: @escaping () async -> Void = {},
                discuss: @escaping (String, String, [LiveDiscussionChoice], Int, DiscussionText.FormatChoice) -> Void
                    = { _, _, _, _, _ in },
                say: @escaping (String, String, [Int]) async throws -> LiveDiscussion? = { _, _, _ in nil }) {
        self.load = load
        self.act = act
        self.open = open
        self.newSession = newSession
        self.changed = changed
        self.say = say
        self.discuss = discuss
    }
}

/// Posts by round, each member a card; the moderator's summary last.
public struct DiscussionView: View {
    let taskID: String
    let actions: DiscussionActions
    @State private var discussion: LiveDiscussion?
    @State private var error: String?
    @State private var busy = false
    @State private var draft = ""

    public init(taskID: String, actions: DiscussionActions) {
        self.taskID = taskID
        self.actions = actions
    }

    public var body: some View {
        VStack(spacing: 0) {
            page
            if let discussion, DiscussionText.canSay(discussion) {
                Divider()
                sayBar(discussion)
            }
        }
        .background(AeroricColor.canvas)
        .task(id: taskID) { await poll() }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.page")
    }

    private var page: some View {
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
                        ForEach(DiscussionText.exchanges(discussion, round: round)) { post in
                            if post.kind == "user" {
                                UserPostCard(post: post, discussion: discussion)
                            } else {
                                PostCard(post: post, member: discussion.members[safe: post.member],
                                         alias: discussion.anonymous ? DiscussionText.alias(post.member) : nil,
                                         onOpen: actions.open)
                            }
                        }
                        if let summary = DiscussionText.summary(discussion, round: round) {
                            if summary.stage == "final" {
                                finalCard(discussion, summary)
                            } else {
                                summaryCard(discussion, summary)
                            }
                        }
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
    }

    /// 2026-11-13: speak to everyone, or `@` one member.
    private func sayBar(_ d: LiveDiscussion) -> some View {
        let named = DiscussionText.mentions(in: draft, d)
        return VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            HStack(spacing: AeroricSpace.xs) {
                ForEach(Array(d.members.enumerated()), id: \.element.id) { index, member in
                    Button("@\(member.label)") { mention(member.label) }
                        .buttonStyle(AeroricChipStyle(selected: named.contains(index)))
                        .disabled(!DiscussionText.canMention(d))
                        .help(DiscussionText.canMention(d) ? "只问这一位，它单独回答，不推进轮次"
                              : "评审进行中不能点名：被点名的评审会看到别人的意见")
                        .accessibilityIdentifier("discussion.mention.\(index)")
                }
                Spacer(minLength: 0)
            }
            HStack(alignment: .bottom, spacing: AeroricSpace.s) {
                TextField(DiscussionText.sayPlaceholder(d), text: $draft, axis: .vertical)
                    .textFieldStyle(.plain)
                    .font(.system(size: AeroricType.body))
                    .lineLimit(1...5)
                    .onSubmit { send(d) }
                    .padding(.horizontal, AeroricSpace.l)
                    .padding(.vertical, AeroricSpace.s)
                    .background(AeroricColor.layer2, in: RoundedRectangle(cornerRadius: 17, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 17, style: .continuous)
                        .strokeBorder(AeroricColor.separator1, lineWidth: 1))
                    .accessibilityIdentifier("discussion.say.field")
                Button(named.isEmpty ? "发送" : "只问 \(named.count) 位") { send(d) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                    .fixedSize()
                    .disabled(busy || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                              || (DiscussionText.needsMention(d) && named.isEmpty))
                    .accessibilityIdentifier("discussion.say.send")
            }
        }
        .padding(.horizontal, AeroricSpace.xl)
        .padding(.vertical, AeroricSpace.m)
        .frame(maxWidth: 960 + 2 * AeroricSpace.xl)
        .frame(maxWidth: .infinity)
    }

    private func mention(_ label: String) {
        let token = "@\(label) "
        guard !draft.contains(token) else { return }
        draft = draft.isEmpty || draft.hasSuffix(" ") || draft.hasSuffix("\n") ? draft + token : draft + " " + token
    }

    private func send(_ d: LiveDiscussion) {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, !busy else { return }
        let mentions = DiscussionText.mentions(in: text, d)
        busy = true
        Task {
            do {
                if let updated = try await actions.say(taskID, text, mentions) { discussion = updated }
                draft = ""
                error = nil
            } catch {
                self.error = AgentConfigStore.describe(error)
            }
            busy = false
        }
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
            if DiscussionText.isReview(d) || DiscussionText.isResearch(d) {
                let steps = DiscussionText.isReview(d) ? DiscussionText.reviewSteps(d) : DiscussionText.researchSteps(d)
                FlowLayout(spacing: AeroricSpace.xs) {
                    ForEach(Array(steps.enumerated()), id: \.offset) { index, step in
                        HStack(spacing: AeroricSpace.xs) {
                            if index > 0 {
                                Image(systemName: "chevron.right")
                                    .font(.system(size: AeroricType.caption - 2))
                                    .foregroundStyle(AeroricColor.textTertiary)
                            }
                            discussionChip(step.title,
                                           tint: step.finished ? AeroricColor.textSecondary : AeroricColor.accent)
                        }
                    }
                }
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("discussion.progress")
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
            let review = DiscussionText.isReview(d)
            if DiscussionText.canAgain(d) {
                if review {
                    control("再改一版", "again", help: "作者按最新意见和你的补充再改一版，然后重新评审")
                } else {
                    control("再来一轮", "again", help: "所有成员再讨论一轮，然后重新总结")
                }
            }
            if ["running", "pausing", "paused"].contains(d.state),
               d.posts.contains(where: { $0.state == "done" }) {
                if review {
                    control("现在定稿", "summarize", help: "等正在写的写完，用最新一版定稿")
                } else if DiscussionText.isResearch(d) {
                    if d.round >= 2 {
                        control("现在汇总", "summarize", help: "打断还没查完的成员，用已有的结果写报告")
                    }
                } else {
                    control("立即总结", "summarize", help: "打断还在发言的成员，请主持人现在总结")
                }
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

    @ViewBuilder private func roundSection(_ d: LiveDiscussion, _ round: Int) -> some View {
        let answers = DiscussionText.answers(d, round: round)
        if !answers.isEmpty {
            roundGrid(d, round, answers)
        }
    }

    private func roundGrid(_ d: LiveDiscussion, _ round: Int, _ answers: [LiveDiscussion.Post]) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text(DiscussionText.roundTitle(d, round))
                .font(.system(size: AeroricType.caption, weight: .medium))
                .foregroundStyle(AeroricColor.textTertiary)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 280), spacing: AeroricSpace.m, alignment: .top)],
                      alignment: .leading, spacing: AeroricSpace.m) {
                ForEach(answers) { post in
                    PostCard(post: post, member: d.members[safe: post.member],
                             alias: d.anonymous ? DiscussionText.alias(post.member) : nil,
                             assignment: post.round == 2 ? DiscussionText.assignment(d, member: post.member) : nil,
                             onOpen: actions.open)
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.round.\(round)")
    }

    /// Draft review: the final version, and where it can go next.
    private func finalCard(_ d: LiveDiscussion, _ post: LiveDiscussion.Post) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Label(DiscussionText.finalTitle(post), systemImage: "checkmark.seal")
                .font(.system(size: AeroricType.secondary, weight: .semibold))
                .foregroundStyle(post.verdict == "pass" ? AeroricColor.success : AeroricColor.warning)
            Text("作者 · \(d.members[safe: post.member]?.label ?? "")")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            PostBody(post: post)
            HStack(spacing: AeroricSpace.s) {
                Button("从定稿新建会话") { actions.newSession(d.projectId, post.text) }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .help("用这份定稿预填新会话的输入框，不会自动开始")
                    .accessibilityIdentifier("discussion.final.newSession")
                Button("围绕这份稿子讨论") {
                    actions.discuss(d.projectId, post.text, DiscussionText.choices(d), d.moderator, .roundtable)
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .help("用同样的成员开一场圆桌讨论，题目是这份稿子；不会自动开始")
                .accessibilityIdentifier("discussion.final.discuss")
                Spacer()
            }
        }
        .padding(AeroricSpace.l)
        .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(AeroricColor.accent.opacity(0.45)))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.final")
    }

    private func summaryCard(_ d: LiveDiscussion, _ post: LiveDiscussion.Post) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                Label("\(DiscussionText.isResearch(d) ? "调研报告" : "主持人总结") · \(d.members[safe: post.member]?.label ?? "")",
                      systemImage: "doc.text")
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
                    if DiscussionText.isResearch(d) {
                        Button("围绕这份报告讨论") {
                            actions.discuss(d.projectId, post.text, DiscussionText.choices(d), d.moderator, .roundtable)
                        }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .help("用同样的成员开一场圆桌讨论，题目是这份报告；不会自动开始")
                        .accessibilityIdentifier("discussion.summary.discuss")
                    }
                    Button("据此起草一份文稿") {
                        actions.discuss(d.projectId, post.text, DiscussionText.choices(d), d.moderator, .review)
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .help("用同样的成员开一场起草评审，主持人当作者；不会自动开始")
                    .accessibilityIdentifier("discussion.summary.draft")
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
    /// What the others call it in an anonymous discussion.
    var alias: String?
    /// Research: the part the moderator gave it.
    var assignment: String?
    let onOpen: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                HarnessLogo(harness: member?.family, agent: member?.agent, size: 14)
                Text(member?.label ?? "成员")
                    .font(.system(size: AeroricType.secondary, weight: .medium))
                    .lineLimit(1)
                if let alias {
                    discussionChip(alias, tint: AeroricColor.textSecondary)
                        .help("匿名讨论里其他成员这样称呼它")
                }
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
            if let assignment {
                Text("分工：\(assignment)")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .lineLimit(3)
            }
            if let angle = member?.angle, post.round == 1, post.kind == "answer" {
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
            if post.kind == "reply" {
                discussionChip("回答你的点名", tint: AeroricColor.accent)
            } else if post.stage == "review", let verdict = DiscussionText.verdict(post.verdict) {
                discussionChip(verdict, tint: post.verdict == "pass" ? AeroricColor.success : AeroricColor.warning)
            } else if let stance = DiscussionText.stance(post.stance) {
                discussionChip(stance, tint: post.stance == "keep" ? AeroricColor.success : AeroricColor.warning)
            }
        }
    }
}

/// 2026-11-13: what the person said, and whom it asked by name.
private struct UserPostCard: View {
    let post: LiveDiscussion.Post
    let discussion: LiveDiscussion

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: "person.fill")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.accent)
                Text(post.mentions.isEmpty
                     ? "你 · 所有成员下一轮回应"
                     : "你 · 只问 " + post.mentions.compactMap { discussion.members[safe: $0]?.label }.joined(separator: "、"))
                    .font(.system(size: AeroricType.secondary, weight: .medium))
                    .lineLimit(1)
                Spacer(minLength: 0)
            }
            PostBody(post: post)
        }
        .padding(AeroricSpace.l)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(AeroricColor.accent.opacity(0.3), lineWidth: 0.5))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("discussion.user.\(post.index)")
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
