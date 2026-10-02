import SwiftUI

/// P5-04 Action 2：审批单。approve/deny/cancel 明确三分；拒绝后的最终
/// 状态由 host 确认（resolved 事件清除），本地不抹掉就算处理。提交中
/// 禁重复点击。
public struct ApprovalSheet: View {
    let request: PendingRequestStore.PendingRequest
    let store: PendingRequestStore
    let onSubmit: (String, PendingRequestStore.Outcome) -> Void
    let onDismiss: () -> Void

    public init(
        request: PendingRequestStore.PendingRequest,
        store: PendingRequestStore,
        onSubmit: @escaping (String, PendingRequestStore.Outcome) -> Void,
        onDismiss: @escaping () -> Void
    ) {
        self.request = request
        self.store = store
        self.onSubmit = onSubmit
        self.onDismiss = onDismiss
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Image(systemName: "exclamationmark.shield.fill")
                    .foregroundStyle(Color.dsWarning)
                Text(request.toolName ?? "工具")
                    .font(.headline)
                    .accessibilityIdentifier("approval.tool-name")
            }
            if let reason = request.reason, !reason.isEmpty {
                Text(reason)
                    .font(.caption)
                    .foregroundStyle(Color.dsTextSecondary)
                    .textSelection(.enabled)
                    .accessibilityIdentifier("approval.reason")
            }
            HStack(spacing: 8) {
                Button("允许一次") {
                    onSubmit(request.id, .approvalAllowedOnce)
                }
                .accessibilityIdentifier("approval.allow-once")
                .disabled(store.isSubmitted(eventId: request.id))
                Button("拒绝", role: .destructive) {
                    onSubmit(request.id, .approvalRejected)
                }
                .accessibilityIdentifier("approval.reject")
                .disabled(store.isSubmitted(eventId: request.id))
                Button("关闭（交给下一个应答者）") {
                    onSubmit(request.id, .next)
                    onDismiss()
                }
                .accessibilityIdentifier("approval.next")
                .disabled(store.isSubmitted(eventId: request.id))
            }
            if store.isSubmitted(eventId: request.id) {
                Text("已提交，等待宿主确认…")
                    .font(.caption)
                    .foregroundStyle(Color.dsTextMuted)
                    .accessibilityIdentifier("approval.submitted")
            }
        }
        .padding(16)
        .frame(minWidth: 360)
        .accessibilityIdentifier("approval.sheet")
    }
}

/// P5-04 Action 2：问题单。多问题时逐题作答；串题防护＝按 eventId 路由
/// （不是队列位置），问题文本逐题显示。
public struct QuestionSheet: View {
    let request: PendingRequestStore.PendingRequest
    let store: PendingRequestStore
    let onSubmit: (String, PendingRequestStore.Outcome) -> Void
    let onDismiss: () -> Void
    @State private var selections: [String: Set<String>] = [:]
    @State private var custom: [String: String] = [:]

    public init(
        request: PendingRequestStore.PendingRequest,
        store: PendingRequestStore,
        onSubmit: @escaping (String, PendingRequestStore.Outcome) -> Void,
        onDismiss: @escaping () -> Void
    ) {
        self.request = request
        self.store = store
        self.onSubmit = onSubmit
        self.onDismiss = onDismiss
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Agent 提问")
                .font(.headline)
            ForEach(Array(request.questions.enumerated()), id: \.offset) { (index, box) in
                let question = box.value
                QuestionBlock(
                    question: question,
                    selection: Binding(
                        get: { selections[questionID(question, index)] ?? [] },
                        set: { selections[questionID(question, index)] = $0 }
                    ),
                    custom: Binding(
                        get: { custom[questionID(question, index)] ?? "" },
                        set: { custom[questionID(question, index)] = $0 }
                    ),
                    index: index
                )
            }
            HStack(spacing: 8) {
                Button("提交回答") {
                    let answers = request.questions.enumerated().map { (index, box) in
                        let question = box.value
                        let id = questionID(question, index)
                        var answer: [String: Any] = ["id": id, "selected": Array(selections[id] ?? [])]
                        if let text = custom[id], !text.isEmpty {
                            answer["custom"] = text
                        }
                        return answer
                    }
                    onSubmit(request.id, .answers(answers.map(JSONBox.init)))
                }
                .accessibilityIdentifier("question.submit")
                .disabled(store.isSubmitted(eventId: request.id))
                Button("跳过（交给下一个应答者）") {
                    onSubmit(request.id, .next)
                    onDismiss()
                }
                .accessibilityIdentifier("question.next")
                .disabled(store.isSubmitted(eventId: request.id))
            }
        }
        .padding(16)
        .frame(minWidth: 420, minHeight: 260)
        .accessibilityIdentifier("question.sheet")
    }

    private func questionID(_ question: [String: Any], _ index: Int) -> String {
        question["id"] as? String ?? "q\(index)"
    }
}

/// 单题区块：问题文本 → 选项 → 自定义输入（三步定位，React 同款串题
/// 防护：禁止按 label 全弹窗匹配）。
private struct QuestionBlock: View {
    let question: [String: Any]
    @Binding var selection: Set<String>
    @Binding var custom: String
    let index: Int

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(question["question"] as? String ?? "问题 \(index + 1)")
                .font(.callout)
                .accessibilityIdentifier("question.text.\(index)")
            ForEach(Array(options.enumerated()), id: \.offset) { _, option in
                Toggle(option, isOn: Binding(
                    get: { selection.contains(option) },
                    set: { on in
                        if on { selection.insert(option) } else { selection.remove(option) }
                    }
                ))
                .accessibilityIdentifier("question.option.\(index).\(option)")
            }
            TextField("自定义回答（可选）", text: $custom)
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("question.custom.\(index)")
        }
    }

    private var options: [String] {
        question["options"] as? [String] ?? []
    }
}
