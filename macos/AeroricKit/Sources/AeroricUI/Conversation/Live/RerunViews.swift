import AeroricProtocol
import SwiftUI

/// R5-2 (2026-11-11): a configuration a turn can be run again with.
public struct RerunTarget: Equatable, Sendable, Identifiable {
    public var id: String
    public var title: String
    /// The harness it runs (`codex`, `claude`, …), told apart when titles repeat.
    public var agent: String
    public var models: [String]

    public init(id: String, title: String, agent: String = "", models: [String]) {
        self.id = id
        self.title = title
        self.agent = agent
        self.models = models
    }
}

/// R5-2: the words of the rerun sheet and the comparison card.
public enum RerunText {
    public static let maxRuns = 3

    /// The session's own harness first, then the rest in catalog order.
    public static func ordered(_ targets: [RerunTarget], agent: String?) -> [RerunTarget] {
        targets.filter { $0.agent == agent } + targets.filter { $0.agent != agent }
    }

    /// A configuration's name; one sharing its name with another gets its
    /// harness after it (every harness has a 「全部第三方模型」).
    public static func title(_ target: RerunTarget, among targets: [RerunTarget]) -> String {
        guard targets.filter({ $0.title == target.title }).count > 1 else { return target.title }
        return "\(target.title) · \(harness(target.agent))"
    }

    public static func harness(_ agent: String) -> String {
        switch agent {
        case "codex": "Codex"
        case "claude": "Claude Code"
        case "omp": "omp"
        case "dsh": "DSH"
        default: agent
        }
    }

    /// `13 个模型`; `默认模型` for a configuration without a model list.
    public static func modelCount(_ target: RerunTarget) -> String {
        target.models.isEmpty ? "默认模型" : "\(target.models.count) 个模型"
    }

    public static func anyRunning(_ groups: [LiveRerunGroup]) -> Bool {
        groups.contains { $0.runs.contains { $0.result.outcome == "running" || $0.result.outcome == "pending" } }
    }

    public static func started(_ started: LiveRerunStarted) -> String {
        var text = "已开始 \(started.runs) 个重跑，各在自己的独立副本里"
        if !started.refused.isEmpty { text += "；没开成：\(started.refused.joined(separator: "；"))" }
        return text
    }

    public static func broughtBack(_ result: (applied: Int, conflicts: Int), label: String) -> String {
        result.conflicts > 0
            ? "已带回 \(label) 的 \(result.applied) 处，\(result.conflicts) 处冲突，去这个重跑的改动页处理"
            : "已带回 \(label) 的 \(result.applied) 处改动"
    }

    /// `运行中` / `完成` / `没跑成 · 登录失效` / `产品失败 · 检查没过`.
    public static func outcome(_ column: LiveRerunColumn) -> String {
        switch column.outcome {
        case "running": "运行中"
        case "pending": "等待开始"
        case "interrupted": "已中断"
        case "replaced": "重跑后已撤销"
        case "notRun": "没跑成 · \(TurnBillText.notRunReason(column.notRun?.reason ?? ""))"
        case "productFailure": column.productFailure?.undone == true ? "产品失败 · 已撤销" : "产品失败 · 检查没过"
        default: "完成"
        }
    }

    public static func tone(_ column: LiveRerunColumn) -> TurnBillText.Tone {
        switch column.outcome {
        case "notRun": .danger
        case "productFailure": .warning
        default: .quiet
        }
    }

    /// `3 个文件 +18 −4`; `没改文件`.
    public static func changes(_ column: LiveRerunColumn) -> String {
        guard column.files > 0 else { return "没改文件" }
        var parts = ["\(column.files) 个文件"]
        if column.additions > 0 { parts.append("+\(column.additions)") }
        if column.deletions > 0 { parts.append("−\(column.deletions)") }
        return parts.joined(separator: " ")
    }

    public static func check(_ column: LiveRerunColumn) -> String {
        switch column.checkPassed {
        case true?: "检查通过"
        case false?: "检查没过" + (column.checkSummary.map { "：\($0)" } ?? "")
        case nil: "没跑检查"
        }
    }

    /// `输入 12.3K · 输出 845 tok`.
    public static func usage(_ column: LiveRerunColumn) -> String {
        guard column.inputTokens != nil || column.outputTokens != nil else { return "—" }
        let input = column.inputTokens.map(SessionStatsText.compact) ?? "—"
        let output = column.outputTokens.map(SessionStatsText.compact) ?? "—"
        return "输入 \(input) · 输出 \(output) tok"
    }

    /// `用时 54 秒 · 6 步工具 · ≈$0.012`.
    public static func effort(_ column: LiveRerunColumn) -> String {
        var parts: [String] = []
        if let ms = column.durationMs { parts.append("用时 \(SessionStatsText.duration(ms))") }
        if let tools = column.tools { parts.append("\(tools) 步工具") }
        if let cost = column.costUsd { parts.append("≈\(TurnBillText.money(cost))") }
        return parts.isEmpty ? "—" : parts.joined(separator: " · ")
    }
}

/// What the sheet starts with: the turn and its message.
struct RerunRequest: Identifiable {
    var id: String { turnID }
    let turnID: String
    let message: String
    /// The original turn's changes are still in the directory.
    let originalKept: Bool
}

/// R5-2: pick up to three configurations, edit the message, start.
struct RerunSheet: View {
    let request: RerunRequest
    let targets: [RerunTarget]
    let onStart: (String, Bool, [LiveRerunChoice]) async -> String?
    @Environment(\.dismiss) private var dismiss
    @State private var message: String
    @State private var messageOnly = false
    @State private var picked: [LiveRerunChoice] = []
    @State private var expanded: Set<String> = []
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var error: String?
    @State private var starting = false

    init(request: RerunRequest, targets: [RerunTarget],
         onStart: @escaping (String, Bool, [LiveRerunChoice]) async -> String?) {
        self.request = request
        self.targets = targets
        self.onStart = onStart
        _message = State(initialValue: request.message)
    }

    private var choices: [LiveRerunChoice] { picked }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("换模型重跑这一轮").font(.system(size: AeroricType.headline, weight: .semibold))
            Text("代码回到这一轮开始前的样子，每个重跑在自己的独立副本里跑，原目录和这个会话都不动。最多选 \(RerunText.maxRuns) 个。")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            ScrollView {
                VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                    ForEach(targets) { target in row(target) }
                }
            }
            .scrollIndicators(.automatic)
            .frame(maxHeight: 260)
            // 目检 2026-11-12: the sheet opened with keyboard focus on the
            // first row, drawn as an accent (purple) ring around it.
            .focusEffectDisabled()
            Text("消息").font(.system(size: AeroricType.secondary, weight: .medium))
            TextEditor(text: $message)
                .font(.system(size: AeroricType.body))
                .scrollContentBackground(.hidden)
                .scrollIndicators(.never)
                .padding(AeroricSpace.m)
                .frame(minHeight: 90, maxHeight: 160)
                .background(AeroricRadius.shape(AeroricRadius.m).fill(AeroricColor.layer1))
                .overlay(AeroricRadius.shape(AeroricRadius.m).strokeBorder(AeroricColor.separator1, lineWidth: 1))
                .focusEffectDisabled()
                .accessibilityIdentifier("rerun.message")
            Toggle("只给消息（不带原会话已确认的决定和约束）", isOn: $messageOnly)
                .toggleStyle(.checkbox)
                .accessibilityIdentifier("rerun.messageOnly")
            if let error { CapsuleErrorLine(error, identifier: "rerun.error") { self.error = nil } }
            HStack {
                Spacer()
                Button("取消") { dismiss() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                Button(starting ? "正在开始…" : "开始重跑（\(choices.count)）") {
                    starting = true
                    Task {
                        error = await onStart(message, !messageOnly, choices)
                        starting = false
                        if error == nil { dismiss() }
                    }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.tinted))
                .disabled(choices.isEmpty || starting || message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .accessibilityIdentifier("rerun.start")
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 520)
    }

    /// One configuration as a flat capsule; opened, its models wrap below it
    /// as chips to pick (several of one configuration may be picked, three
    /// in all). A plain button opens it: a DisclosureGroup did not answer
    /// AXPress.
    private func row(_ target: RerunTarget) -> some View {
        let options: [String?] = target.models.isEmpty ? [nil] : target.models.map { Optional($0) }
        let count = picked.filter { $0.configurationId == target.id }.count
        let open = expanded.contains(target.id)
        let title = RerunText.title(target, among: targets)
        return VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Button {
                withAnimation(reduceMotion ? nil : .easeOut(duration: 0.15)) {
                    if open { expanded.remove(target.id) } else { expanded.insert(target.id) }
                }
            } label: {
                HStack(spacing: AeroricSpace.s) {
                    Text(title)
                        .font(.system(size: AeroricType.body, weight: .medium))
                        .foregroundStyle(AeroricColor.text)
                        .lineLimit(1)
                    Spacer(minLength: AeroricSpace.s)
                    if count > 0 {
                        Text("已选 \(count)")
                            .font(.system(size: AeroricType.caption, weight: .medium))
                            .foregroundStyle(AeroricColor.accent)
                            .padding(.horizontal, AeroricSpace.s)
                            .frame(height: 16)
                            .background(Capsule(style: .continuous).fill(AeroricColor.accent.opacity(0.14)))
                    }
                    Text(RerunText.modelCount(target))
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                    Image(systemName: "chevron.right")
                        .font(.system(size: AeroricType.caption, weight: .semibold))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .rotationEffect(.degrees(open ? 90 : 0))
                }
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 28)
                .background(Capsule(style: .continuous).fill(open || count > 0 ? AeroricColor.layer2 : AeroricColor.layer1))
                .overlay(Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 0.5))
                .contentShape(Capsule(style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityLabel(count > 0 ? "\(title)，已选 \(count)" : title)
            .accessibilityIdentifier("rerun.target.\(target.id)")
            if open {
                FlowLayout(spacing: AeroricSpace.xs) {
                    ForEach(options, id: \.self) { model in
                        chip(LiveRerunChoice(configurationId: target.id, model: model), title: model ?? "默认模型")
                    }
                }
                .padding(.horizontal, AeroricSpace.l)
                .padding(.bottom, AeroricSpace.xs)
            }
        }
        .help(picked.count >= RerunText.maxRuns ? "最多 \(RerunText.maxRuns) 个" : title)
    }

    /// A model to pick: a filled capsule with a check once picked.
    private func chip(_ choice: LiveRerunChoice, title: String) -> some View {
        let on = picked.contains(choice)
        return Button {
            if on { picked.removeAll { $0 == choice } } else { picked.append(choice) }
        } label: {
            Text(title).lineLimit(1)
        }
        .buttonStyle(AeroricChipStyle(selected: on))
        .disabled(!on && picked.count >= RerunText.maxRuns)
        .accessibilityAddTraits(on ? .isSelected : [])
        .accessibilityIdentifier("rerun.model.\(choice.configurationId).\(choice.model ?? "default")")
    }
}

/// R5-2: the original turn and its reruns side by side.
struct RerunCompareCard: View {
    let group: LiveRerunGroup
    /// The original turn's changes are still in the directory.
    let originalKept: Bool
    let onOpen: (String) -> Void
    let onBringBack: (LiveRerunRun, Bool) -> Void
    let onDrop: ([LiveRerunRun]) -> Void
    @State private var keep: LiveRerunRun?

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack {
                Label("换模型重跑 · \(group.runs.count) 个", systemImage: "arrow.triangle.branch")
                    .font(.system(size: AeroricType.secondary, weight: .semibold))
                Spacer()
                Text(group.withBrief ? "带了已确认的决定" : "只给了消息")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                let left = group.runs.filter { !$0.gone && $0.copy }
                if !left.isEmpty, !RerunText.anyRunning([group]) {
                    Button("全部丢弃") { onDrop(left) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .help("删掉这些重跑的独立副本；会话留在侧栏")
                        .accessibilityIdentifier("rerun.dropAll")
                }
            }
            ScrollView(.horizontal) {
                HStack(alignment: .top, spacing: AeroricSpace.m) {
                    column(title: "原来这一轮", result: group.original, run: nil)
                    ForEach(group.runs) { run in column(title: run.label, result: run.result, run: run) }
                }
            }
        }
        .padding(AeroricSpace.l)
        .background(AeroricColor.layer1, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(AeroricColor.separator1))
        // `.contain` keeps the buttons' own identifiers (R5 目检: the card's
        // id replaced them, so 全部丢弃 could not be found).
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("rerun.card")
        .confirmationDialog("带回这个重跑", isPresented: Binding(get: { keep != nil }, set: { if !$0 { keep = nil } }),
                            presenting: keep) { run in
            if originalKept {
                Button("撤销原来这一轮，再带回") { onBringBack(run, true) }
            }
            Button(originalKept ? "直接带回（和原来的改动叠在一起）" : "带回") { onBringBack(run, false) }
            Button("取消", role: .cancel) {}
        } message: { _ in
            Text(originalKept ? "原来这一轮的改动还在目录里。" : "把这个重跑副本里的改动带回原目录。")
        }
    }

    private func column(title: String, result: LiveRerunColumn, run: LiveRerunRun?) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            Text(title).font(.system(size: AeroricType.secondary, weight: .medium)).lineLimit(1)
            Text(RerunText.outcome(result))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(color(RerunText.tone(result)))
                .help(result.notRun?.message ?? result.productFailure?.check ?? "")
            Group {
                Text(RerunText.changes(result))
                Text(RerunText.check(result)).lineLimit(2)
                Text(RerunText.usage(result))
                Text(RerunText.effort(result))
            }
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(AeroricColor.textSecondary)
            if let run {
                HStack(spacing: AeroricSpace.xs) {
                    Button("打开") { onOpen(run.taskId) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .disabled(run.gone)
                    Button("带回这个") { keep = run }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .disabled(run.gone || !run.copy || result.files == 0 || result.outcome == "running")
                        .help(run.copy ? "把这个重跑的改动带回原目录" : "副本已经丢弃")
                        .accessibilityIdentifier("rerun.bringBack")
                    Button("丢弃") { onDrop([run]) }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .disabled(run.gone || !run.copy || result.outcome == "running")
                        .accessibilityIdentifier("rerun.drop")
                }
                .padding(.top, AeroricSpace.xs)
            }
        }
        .frame(width: 200, alignment: .leading)
        .padding(AeroricSpace.m)
        .background(AeroricColor.layer2, in: RoundedRectangle(cornerRadius: 8))
    }

    private func color(_ tone: TurnBillText.Tone) -> Color {
        switch tone {
        case .danger: AeroricColor.danger
        case .warning: AeroricColor.warning
        case .quiet: AeroricColor.textSecondary
        }
    }
}
