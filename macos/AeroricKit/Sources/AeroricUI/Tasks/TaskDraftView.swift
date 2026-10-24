import SwiftUI
import AppKit

/// P5-03 Action 3：任务草稿表单。字段选项来自宿主 capability（注入的
/// `TaskDraft.Options`，空集＝宿主未回答，不伪造可用项）；提交时校验，
/// 校验错误逐字段显示；创建的 revision/idempotency 由调用方提交。
public struct TaskDraftView: View {
    public var options: TaskDraft.Options
    @Binding public var draft: TaskDraft
    public var onSubmit: (TaskDraft) -> Void
    public var onCancel: (() -> Void)?

    @State private var validationErrors: [String: String] = [:]
    @State private var showingAttachmentPicker = false

    public init(
        options: TaskDraft.Options,
        draft: Binding<TaskDraft>,
        onSubmit: @escaping (TaskDraft) -> Void,
        onCancel: (() -> Void)? = nil
    ) {
        self.options = options
        self._draft = draft
        self.onSubmit = onSubmit
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("新建任务")
                .font(.headline)
            field("Agent") {
                Picker("Agent", selection: $draft.agent) {
                    if options.agents.isEmpty {
                        Text("宿主未提供").tag(draft.agent)
                    }
                    ForEach(options.agents, id: \.self) { agent in
                        Text(agent).tag(agent)
                    }
                }
                .labelsHidden()
            }
            field("Model") {
                Picker("Model", selection: Binding(
                    get: { draft.model ?? "" },
                    set: { draft.model = $0.isEmpty ? nil : $0 }
                )) {
                    Text("默认").tag("")
                    ForEach(options.models, id: \.self) { model in
                        Text(model).tag(model)
                    }
                }
                .labelsHidden()
            }
            field("Effort") {
                Picker("Effort", selection: Binding(
                    get: { draft.effort ?? "" },
                    set: { draft.effort = $0.isEmpty ? nil : $0 }
                )) {
                    Text("默认").tag("")
                    ForEach(options.efforts, id: \.self) { effort in
                        Text(effort).tag(effort)
                    }
                }
                .labelsHidden()
            }
            field("权限") {
                Picker("权限", selection: $draft.permissionMode) {
                    if options.permissionModes.isEmpty {
                        Text("宿主未提供").tag(draft.permissionMode)
                    }
                    ForEach(options.permissionModes, id: \.self) { mode in
                        Text(mode).tag(mode)
                    }
                }
                .labelsHidden()
            }
            field("Worktree") {
                TextField("可选 worktree 路径", text: Binding(
                    get: { draft.worktree ?? "" },
                    set: { draft.worktree = $0.isEmpty ? nil : $0 }
                ))
            }
            field("提示词") {
                TextEditor(text: $draft.prompt)
                    .frame(minHeight: 72)
                    .overlay(RoundedRectangle(cornerRadius: 4).stroke(AeroricColor.separator2))
            }
            field("附件") {
                HStack {
                    Button("添加附件…") { showingAttachmentPicker = true }
                        .accessibilityIdentifier("draft.add-attachment")
                    ForEach(draft.attachmentPaths, id: \.self) { path in
                        Text((path as NSString).lastPathComponent)
                            .font(.caption)
                            .lineLimit(1)
                    }
                }
            }
            ForEach(Array(validationErrors.sorted(by: { $0.key < $1.key })), id: \.key) { field, message in
                Text("\(field): \(message)")
                    .font(.caption)
                    .foregroundStyle(AeroricColor.danger)
                    .accessibilityIdentifier("draft.error.\(field)")
            }
            HStack {
                Button("取消") { onCancel?() }
                Spacer()
                Button("创建") {
                    let errors = draft.validate(against: options)
                    validationErrors = errors
                    if errors.isEmpty { onSubmit(draft) }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .accessibilityIdentifier("draft.submit")
            }
        }
        .padding(16)
        .frame(width: 460)
        .background(NativeMaterial.underWindowBackground.ignoresSafeArea())
        .accessibilityIdentifier("task.draft")
        .fileImporter(
            isPresented: $showingAttachmentPicker,
            allowedContentTypes: [.item]
        ) { result in
            if case let .success(url) = result {
                if !draft.attachmentPaths.contains(url.path) {
                    draft.attachmentPaths.append(url.path)
                }
            }
        }
    }

    private func field(_ title: String, @ViewBuilder content: () -> some View) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title)
                .font(.callout)
                .frame(width: 64, alignment: .trailing)
            content()
        }
    }
}
