import AeroricClient
import SwiftUI

/// Looks up what canvas nodes refer to. Supplied by the App so this module
/// never reaches into stores or the host directly.
public struct CanvasResolver {
    public var task: (String) -> TaskStore.TaskEntry?
    public var imported: (String) -> ImportsClient.ImportedSession?
    public var projects: () -> [TaskStore.ProjectEntry]
    public var allTasks: () -> [TaskStore.TaskEntry]
    public var allImported: () -> [ImportsClient.ImportedSession]
    public var readFile: (String, String) async -> Result<String, Error>

    public init(task: @escaping (String) -> TaskStore.TaskEntry?,
                imported: @escaping (String) -> ImportsClient.ImportedSession?,
                projects: @escaping () -> [TaskStore.ProjectEntry],
                allTasks: @escaping () -> [TaskStore.TaskEntry],
                allImported: @escaping () -> [ImportsClient.ImportedSession],
                readFile: @escaping (String, String) async -> Result<String, Error>) {
        self.task = task
        self.imported = imported
        self.projects = projects
        self.allTasks = allTasks
        self.allImported = allImported
        self.readFile = readFile
    }
}

/// Named colours a node may carry (stored as the name).
enum CanvasPalette {
    static let names = ["yellow", "blue", "green", "pink", "purple", "gray"]

    static func color(_ name: String?) -> Color {
        switch name {
        case "yellow": AeroricColor.warning
        case "blue": AeroricColor.accent
        case "green": AeroricColor.success
        case "pink": AeroricColor.danger
        case "purple": AeroricColor.violet
        default: AeroricColor.textTertiary
        }
    }

    static func title(_ name: String) -> String {
        switch name {
        case "yellow": "黄"
        case "blue": "蓝"
        case "green": "绿"
        case "pink": "粉"
        case "purple": "紫"
        default: "灰"
        }
    }
}

/// Card chrome shared by session / note / file nodes.
struct CanvasCard<Content: View>: View {
    let selected: Bool
    let tint: Color?
    let content: Content

    init(selected: Bool, tint: Color? = nil, @ViewBuilder content: () -> Content) {
        self.selected = selected
        self.tint = tint
        self.content = content()
    }

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.m)
        content
            .padding(AeroricSpace.l)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(AeroricColor.layer1, in: shape)
            .background(tint.map { $0.opacity(0.14) } ?? .clear, in: shape)
            .overlay {
                shape.strokeBorder(selected ? AeroricColor.accent : AeroricColor.separator2,
                                   lineWidth: selected ? 2 : 1)
            }
            .clipShape(shape)
            .shadow(color: AeroricColor.shadow, radius: selected ? 8 : 3, y: 2)
    }
}

struct SessionNodeView: View {
    let node: CanvasClient.Node
    let resolver: CanvasResolver
    let selected: Bool

    var body: some View {
        CanvasCard(selected: selected) {
            if let task = node.taskId.flatMap(resolver.task) {
                content(title: task.title, agent: task.agent, status: task.status,
                        date: TaskBoard.date(of: task), imported: false)
            } else if let session = node.importedId.flatMap(resolver.imported) {
                content(title: session.title, agent: session.agent, status: "done",
                        date: Date(timeIntervalSince1970: Double(session.lastAtMs) / 1000), imported: true)
            } else {
                Label("会话已不存在", systemImage: "exclamationmark.triangle")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
        }
    }

    private func content(title: String, agent: String?, status: String, date: Date?, imported: Bool) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.m) {
                StatusDot(ActivityState(wireStatus: status), label: status)
                Text(title)
                    .font(.system(size: AeroricType.body, weight: .medium))
                    .lineLimit(2)
            }
            Spacer(minLength: 0)
            HStack(spacing: AeroricSpace.s) {
                AgentGlyph(agent: agent, size: 12)
                Text(AgentGlyph.displayName(for: agent))
                if imported { Text("· 已导入") }
                Spacer()
                if let date { Text(RelativeTime.short(date)) }
            }
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(AeroricColor.textTertiary)
        }
    }
}

struct NoteNodeView: View {
    let node: CanvasClient.Node
    let selected: Bool
    let editing: Bool
    let onCommit: (String) -> Void
    @State private var draft = ""
    @FocusState private var focused: Bool

    var body: some View {
        CanvasCard(selected: selected, tint: CanvasPalette.color(node.color ?? "yellow")) {
            if editing {
                TextEditor(text: $draft)
                    .font(.system(size: AeroricType.body))
                    .scrollContentBackground(.hidden)
                    .focused($focused)
                    .onAppear {
                        draft = node.content ?? ""
                        focused = true
                    }
                    .onChange(of: focused) { _, isFocused in if !isFocused { onCommit(draft) } }
                    .onKeyPress(.escape) {
                        onCommit(draft)
                        return .handled
                    }
            } else {
                Text((node.content ?? "").isEmpty ? "双击编辑便签" : node.content ?? "")
                    .font(.system(size: AeroricType.body))
                    .foregroundStyle((node.content ?? "").isEmpty ? AeroricColor.textTertiary : AeroricColor.text)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        }
    }
}

struct RegionNodeView: View {
    let node: CanvasClient.Node
    let selected: Bool

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.l)
        let tint = CanvasPalette.color(node.color ?? "blue")
        VStack(alignment: .leading, spacing: 0) {
            Text(node.title?.isEmpty == false ? node.title! : "区域")
                .font(.system(size: AeroricType.secondary, weight: .semibold))
                .foregroundStyle(tint)
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.s)
            Spacer(minLength: 0)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(tint.opacity(0.06), in: shape)
        .overlay {
            shape.strokeBorder(selected ? AeroricColor.accent : tint.opacity(0.45),
                               style: StrokeStyle(lineWidth: selected ? 2 : 1, dash: selected ? [] : [6, 4]))
        }
    }
}

struct FileNodeView: View {
    let node: CanvasClient.Node
    let resolver: CanvasResolver
    let selected: Bool
    @State private var preview: Result<String, Error>?

    static let previewLines = 30

    var body: some View {
        CanvasCard(selected: selected) {
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                HStack(spacing: AeroricSpace.s) {
                    Image(systemName: FileSymbol.symbol(forFile: node.path ?? ""))
                        .foregroundStyle(AeroricColor.textSecondary)
                    Text(URL(fileURLWithPath: node.path ?? "").lastPathComponent)
                        .font(.system(size: AeroricType.body, weight: .medium))
                        .lineLimit(1)
                }
                Text(projectLabel)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                HairlineDivider()
                switch preview {
                case .none:
                    ProgressView().controlSize(.small)
                case let .success(text):
                    Text(text.split(separator: "\n", omittingEmptySubsequences: false)
                        .prefix(Self.previewLines).joined(separator: "\n"))
                        .font(AeroricType.mono(AeroricType.code - 1))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                        .clipped()
                case .failure:
                    Label("文件不可读或已不存在", systemImage: "exclamationmark.triangle")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
        }
        .task(id: "\(node.projectId ?? "")/\(node.path ?? "")") {
            guard let projectID = node.projectId, let path = node.path else { return }
            preview = await resolver.readFile(projectID, path)
        }
    }

    private var projectLabel: String {
        let project = resolver.projects().first { $0.id == node.projectId }?.name ?? "未知项目"
        return "\(project) · \(node.path ?? "")"
    }
}
