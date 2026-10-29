import SwiftUI


/// 2026-10-28 (接着聊): what an imported session offers at the bottom of its
/// page. Claude, Codex, DeepSeek Harness and OMP sessions can be taken over
/// (resumed by their own harness); the others can only be forked into a new
/// session; a session outside every project first needs its folder added.
public enum ImportContinuation: Equatable, Sendable {
    case resume
    case addProject(path: String)
    case forkOnly
    /// No project and no folder to add: only forking is left.
    case forkWithoutProject

    public static let resumable: Set<String> = ["claude", "codex", "dsh", "omp"]

    public static func of(agent: String, projectID: String?, cwd: String?) -> ImportContinuation {
        guard resumable.contains(agent) else { return .forkOnly }
        if projectID != nil { return .resume }
        if let cwd, !cwd.isEmpty { return .addProject(path: cwd) }
        return .forkWithoutProject
    }
}

/// The opening prompt of a session forked from an import: its title and the
/// newest few turns, capped so the prompt stays readable.
public enum ImportForkPrompt {
    static let turns = 6
    static let perTurn = 600
    static let total = 4_000

    public static func make(agent: String, title: String, messages: [SessionMessage]) -> String {
        let spoken: [(String, String)] = messages.compactMap { message in
            let text = message.content.compactMap { content -> String? in
                if case let .text(text: text) = content { return text }
                return nil
            }
            .joined(separator: "\n")
            .trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty, message.role == "user" || message.role == "assistant" else { return nil }
            return (message.role == "user" ? "用户" : "助手", text)
        }
        var lines: [String] = []
        var used = 0
        for (speaker, text) in spoken.suffix(turns).reversed() {
            let clipped = text.count > perTurn ? String(text.prefix(perTurn)) + "…" : text
            let line = "\(speaker)：\(clipped)"
            if used + line.count > total { break }
            used += line.count
            lines.insert(line, at: 0)
        }
        let header = "以下是从 \(AgentGlyph.displayName(for: agent)) 导入的会话「\(title)」的最近几轮，请在此基础上继续。"
        guard !lines.isEmpty else { return header + "\n\n" }
        return header + "\n\n" + lines.joined(separator: "\n\n") + "\n\n---\n"
    }
}

/// The flat glass capsule under an imported session (设计稿 方案 A · 胶囊).
struct ImportedSessionBar: View {
    let agent: String
    let continuation: ImportContinuation
    let availability: [String: Bool]
    let preferredHarness: String
    let busy: Bool
    let onContinue: () -> Void
    let onFork: (String) -> Void

    @State private var forkHarness: String?

    private var harness: String {
        forkHarness ?? NewSessionLayout.defaultHarness(preferred: preferredHarness, availability: availability)
    }

    private var agentName: String { AgentGlyph.displayName(for: agent) }

    var body: some View {
        HStack(spacing: AeroricSpace.m) {
            if case .addProject = continuation {
                Image(systemName: "folder")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(AeroricColor.textSecondary)
            }
            message
                .font(.system(size: AeroricType.secondary))
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .leading)
            actions
        }
        .padding(.leading, AeroricSpace.xxl)
        .padding(.trailing, 7)
        .frame(height: 50)
        .glassSurface(.floatingCard, in: Capsule(style: .continuous))
        .shadow(color: AeroricColor.shadow, radius: 15, y: 8)
        .disabled(busy)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("imported.bar")
    }

    @ViewBuilder private var message: some View {
        switch continuation {
        case .resume:
            Text("**从 \(agentName) 导入** · 接着聊会恢复原会话并在 Aeroric 里接管，记录仍留在原处")
                .foregroundStyle(AeroricColor.textSecondary)
        case let .addProject(path):
            Text("**\(ImportPresentation.abbreviated(path))** 还不是项目，接着聊需要先添加")
                .foregroundStyle(AeroricColor.textSecondary)
        case .forkOnly, .forkWithoutProject:
            Text("**\(agentName) 的会话无法续接** · 分叉会带上标题和最近几轮，换一个 Harness 继续")
                .foregroundStyle(AeroricColor.textSecondary)
        }
    }

    @ViewBuilder private var actions: some View {
        switch continuation {
        case .resume:
            Button("分叉为新会话") { onFork(agent) }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                .accessibilityIdentifier("imported.fork")
            primary("接着聊", systemImage: "arrow.right", action: onContinue)
                .accessibilityIdentifier("imported.continue")
        case .addProject:
            primary("添加为项目并接着聊", systemImage: nil, action: onContinue)
                .accessibilityIdentifier("imported.add-project")
        case .forkOnly, .forkWithoutProject:
            HarnessSlider(selection: harness, availability: availability) { forkHarness = $0 }
                .fixedSize()
            primary("分叉为新会话", systemImage: "arrow.triangle.branch") { onFork(harness) }
                .accessibilityIdentifier("imported.fork")
        }
    }

    private func primary(_ title: String, systemImage: String?, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: AeroricSpace.s) {
                if busy { ProgressView().controlSize(.small) }
                else if let systemImage { Image(systemName: systemImage).font(.system(size: 11, weight: .bold)) }
                Text(title).fontWeight(.semibold)
            }
        }
        .buttonStyle(AeroricCapsuleButtonStyle(.primary))
    }
}
