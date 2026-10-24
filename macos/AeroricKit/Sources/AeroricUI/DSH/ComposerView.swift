import SwiftUI
import Observation

/// P5-04 Action 3：DSH composer 视图。模型/effort 控制只显示 capability
/// 允许的值；斜杠命令早跳（不回显）；附件经 fileImporter。
public struct ComposerView: View {
    public var catalog: DshComposerModel.Catalog?
    public var onSend: (_ text: String, _ attachmentPaths: [String]) -> Void
    @Binding public var draft: String
    @Binding public var attachmentPaths: [String]

    @State private var sendError: String?
    @State private var showingAttachmentPicker = false

    public init(
        catalog: DshComposerModel.Catalog?,
        draft: Binding<String>,
        attachmentPaths: Binding<[String]>,
        onSend: @escaping (_ text: String, _ attachmentPaths: [String]) -> Void
    ) {
        self.catalog = catalog
        self._draft = draft
        self._attachmentPaths = attachmentPaths
        self.onSend = onSend
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if let catalog {
                HStack(spacing: 12) {
                    Picker("模型", selection: Binding(
                        get: { "\(catalog.currentProvider):\(catalog.currentModel)" },
                        set: { newValue in
                            // 选择被 capability 门控（非法值不上 wire）。
                            let parts = newValue.split(separator: ":", maxSplits: 1)
                            guard parts.count == 2 else { return }
                            let provider = String(parts[0])
                            let model = String(parts[1])
                            guard DshComposerModel.isAllowed(
                                provider: provider, model: model, effort: catalog.currentEffort,
                                in: catalog
                            ) else { return }
                        }
                    )) {
                        ForEach(catalog.choices, id: \.model) { choice in
                            Text("\(choice.provider)/\(choice.model)")
                                .tag("\(choice.provider):\(choice.model)")
                        }
                    }
                    .labelsHidden()
                    .accessibilityIdentifier("composer.model")

                    if !catalog.efforts.isEmpty {
                        Picker("Effort", selection: Binding(
                            get: { catalog.currentEffort ?? catalog.efforts.first ?? "" },
                            set: { newValue in
                                guard catalog.efforts.contains(newValue) else { return }
                            }
                        )) {
                            ForEach(catalog.efforts, id: \.self) { effort in
                                Text(effort).tag(effort)
                            }
                        }
                        .labelsHidden()
                        .accessibilityIdentifier("composer.effort")
                    }
                    Spacer()
                }
            }
            if !attachmentPaths.isEmpty {
                ScrollView(.horizontal) {
                    HStack {
                        ForEach(attachmentPaths, id: \.self) { path in
                            HStack(spacing: 4) {
                                Image(systemName: "paperclip")
                                    .font(.caption2)
                                Text((path as NSString).lastPathComponent)
                                    .font(.caption)
                                Button("移除") {
                                    attachmentPaths.removeAll { $0 == path }
                                }
                                .font(.caption2)
                                .accessibilityIdentifier("composer.attachment.remove.\((path as NSString).lastPathComponent)")
                            }
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(AeroricColor.canvas)
                        }
                    }
                }
                .accessibilityIdentifier("composer.attachments")
            }
            HStack(alignment: .bottom, spacing: 8) {
                Button("附件") { showingAttachmentPicker = true }
                    .accessibilityIdentifier("composer.attachment.add")
                TextField("消息…（/ 为斜杠命令）", text: $draft, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...5)
                    .onSubmit { send() }
                    .accessibilityIdentifier("composer.text")
                Button("发送") { send() }
                    .accessibilityIdentifier("composer.send")
            }
            if let sendError {
                Text(sendError)
                    .font(.caption)
                    .foregroundStyle(AeroricColor.danger)
                    .accessibilityIdentifier("composer.error")
            }
        }
        .padding(8)
        .fileImporter(
            isPresented: $showingAttachmentPicker,
            allowedContentTypes: [.item],
            allowsMultipleSelection: true
        ) { result in
            if let urls = try? result.get() {
                attachmentPaths.append(contentsOf: urls.map(\.path))
            }
        }
        .accessibilityIdentifier("composer.view")
    }

    private func send() {
        // 斜杠命令早跳：命令行直接发送（不回显，回显防重复在宿主 projection）。
        if DshComposerModel.parseSlashCommand(draft) != nil {
            onSend(draft, attachmentPaths)
            draft = ""
            return
        }
        if let error = DshComposerModel.validateComposer(text: draft, attachmentPaths: attachmentPaths) {
            sendError = error
            return
        }
        sendError = nil
        onSend(draft, attachmentPaths)
        draft = ""
        attachmentPaths = []
    }
}

/// P5-04 Action 3：斜杠命令选择器（session/list_commands 的客户端镜像）。
public struct SlashCommandPicker: View {
    public var commands: [String]
    public var onPick: (_ command: String) -> Void

    public init(commands: [String], onPick: @escaping (_ command: String) -> Void) {
        self.commands = commands
        self.onPick = onPick
    }

    public var body: some View {
        Menu("命令") {
            ForEach(commands, id: \.self) { command in
                Button(command) { onPick(command) }
                    .accessibilityIdentifier("slash.command.\(command)")
            }
        }
        .accessibilityIdentifier("slash.picker")
    }
}

/// P5-04 Action 3：队列面板（session/queue 事件驱动；无数据不显示假条目）。
public struct QueuePanel: View {
    public var items: [DshTrajectoryEntry]

    public init(items: [DshTrajectoryEntry]) {
        self.items = items
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("队列")
                .font(.caption)
                .foregroundStyle(AeroricColor.textTertiary)
            ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                Text("#\(item.seq) \(item.type)")
                    .font(.caption)
                    .accessibilityIdentifier("queue.item.\(index)")
            }
        }
        .padding(8)
        .accessibilityIdentifier("queue.panel")
    }
}

/// P5-04 Action 3：目标面板（plans/goals/todos 宿主事件驱动）。
public struct GoalPanel: View {
    public var goals: [String]

    public init(goals: [String]) {
        self.goals = goals
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("目标")
                .font(.caption)
                .foregroundStyle(AeroricColor.textTertiary)
            ForEach(Array(goals.enumerated()), id: \.offset) { index, goal in
                Text(goal)
                    .font(.caption)
                    .accessibilityIdentifier("goal.item.\(index)")
            }
        }
        .padding(8)
        .accessibilityIdentifier("goal.panel")
    }
}

/// P5-04 Action 3：子代理面板（nested subagents 宿主事件驱动）。
public struct SubagentPanel: View {
    public var subagents: [String]

    public init(subagents: [String]) {
        self.subagents = subagents
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("子代理")
                .font(.caption)
                .foregroundStyle(AeroricColor.textTertiary)
            ForEach(Array(subagents.enumerated()), id: \.offset) { index, subagent in
                Text(subagent)
                    .font(.caption)
                    .accessibilityIdentifier("subagent.item.\(index)")
            }
        }
        .padding(8)
        .accessibilityIdentifier("subagent.panel")
    }
}

/// P5-04 Action 3：轨迹视图（seq 去重合并后的条目列表；未知事件保留）。
public struct TrajectoryView: View {
    public var entries: [DshTrajectoryEntry]

    public init(entries: [DshTrajectoryEntry]) {
        self.entries = entries
    }

    public var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 4) {
                ForEach(Array(entries.enumerated()), id: \.offset) { index, entry in
                    HStack(spacing: 6) {
                        Text("#\(entry.seq)")
                            .font(.caption2.monospaced())
                            .foregroundStyle(AeroricColor.textTertiary)
                        Text(entry.type)
                            .font(.caption)
                        if let time = entry.time {
                            Text(time)
                                .font(.caption2)
                                .foregroundStyle(AeroricColor.textTertiary)
                        }
                    }
                    .accessibilityIdentifier("trajectory.entry.\(index)")
                }
            }
            .padding(8)
        }
        .accessibilityIdentifier("trajectory.view")
    }
}
