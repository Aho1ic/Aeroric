import AeroricClient
import AppKit
import Observation
import SwiftUI

/// Linked folders (2026-10-29, L1–L2): folders outside a project that its new
/// sessions get as extra working directories and that the file tree lists.
/// Nothing is written into the project.
@MainActor @Observable
public final class LinkedFolderBook {
    public private(set) var folders: [String: [LinkedFolder]] = [:]
    @ObservationIgnored private var client: WorkspaceClient?

    public init() {}

    public func attach(client: WorkspaceClient?) {
        self.client = client
        Task { await reload() }
    }

    public func reload() async {
        guard let client else { return }
        if let all = try? await client.linkedFolders() { folders = all }
    }

    public func folders(of project: String) -> [LinkedFolder] { folders[project] ?? [] }

    /// Nil on success, else the reason to show.
    public func link(_ url: URL, to project: String) async -> String? {
        guard let client else { return "没有连接宿主" }
        do {
            folders[project] = try await client.linkFolder(projectId: project, path: url.path)
            return nil
        } catch let ConnectionError.refused(failure) {
            return failure.error.message
        } catch {
            return "\(error)"
        }
    }

    public func unlink(_ folder: LinkedFolder, from project: String) async {
        guard let client else { return }
        if let rest = try? await client.unlinkFolder(projectId: project, path: folder.path) {
            folders[project] = rest.isEmpty ? nil : rest
        }
    }
}

/// How a linked folder sits in the project's file tree: its nodes carry
/// `@linked:<folder>|<relative path>`; the host is asked for them as project
/// `<project>#linked:<folder>` (and serves only folders the project lists).
public enum LinkedFolderPath {
    static let prefix = "@linked:"

    public static func node(folder: String, relative: String = "") -> String {
        "\(prefix)\(folder)|\(relative)"
    }

    /// `(host project id, relative path)` for a tree path.
    public static func request(projectID: String, path: String) -> (projectID: String, path: String) {
        guard path.hasPrefix(prefix), let bar = path.firstIndex(of: "|") else { return (projectID, path) }
        let folder = String(path[path.index(path.startIndex, offsetBy: prefix.count) ..< bar])
        return ("\(projectID)#linked:\(folder)", String(path[path.index(after: bar)...]))
    }

    /// A child path the host returned for `parent`, re-rooted into the tree.
    public static func child(of parent: String, hostPath: String) -> String {
        guard parent.hasPrefix(prefix), let bar = parent.firstIndex(of: "|") else { return hostPath }
        return String(parent[...bar]) + hostPath
    }
}

/// 关联文件夹 · <项目> (2026-10-29 design): rows with name, path and a
/// missing state; 添加文件夹… opens a folder panel; refusals show inline.
struct LinkedFoldersSheet: View {
    let project: SidebarItem
    let book: LinkedFolderBook
    let onClose: () -> Void
    @State private var error: String?
    @State private var hovered: String?

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            HStack {
                Text("关联文件夹 · \(project.title)")
                    .font(.system(size: AeroricType.title, weight: .semibold))
                    .lineLimit(1)
                Spacer()
                Button { choose() } label: { Label("添加文件夹…", systemImage: "folder.badge.plus") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                    .accessibilityIdentifier("linked.add")
            }
            Text("新会话启动时交给 Agent 作为额外工作目录，并显示在文件树里。不会往项目里写任何东西。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            let folders = book.folders(of: project.id)
            if folders.isEmpty {
                Text("还没有关联文件夹。")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .padding(.vertical, AeroricSpace.l)
            } else {
                VStack(spacing: 2) {
                    ForEach(folders) { folder in row(folder) }
                }
            }
            if let error {
                Label(error, systemImage: "exclamationmark.circle")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.danger)
            }
            HStack {
                Spacer()
                Button("完成", action: onClose)
                    .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                    .keyboardShortcut(.defaultAction)
                    .fixedSize()
            }
            .padding(.top, AeroricSpace.s)
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 480)
    }

    private func row(_ folder: LinkedFolder) -> some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: folder.exists ? "folder.badge.gearshape" : "folder.badge.questionmark")
                .foregroundStyle(folder.exists ? AeroricColor.accent : AeroricColor.warning)
                .frame(width: 18)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: AeroricSpace.s) {
                    Text(folder.name).lineLimit(1)
                    if !folder.exists {
                        Text("找不到这个文件夹")
                            .font(.system(size: AeroricType.caption - 1))
                            .foregroundStyle(AeroricColor.warning)
                            .fixedSize()
                    }
                }
                Text(Self.tilde(folder.path))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: AeroricSpace.s)
            RoundIconButton(systemImage: "xmark", tint: AeroricColor.textSecondary, help: "移除关联") {
                Task { await book.unlink(folder, from: project.id) }
            }
        }
        .padding(.horizontal, AeroricSpace.l)
        .frame(minHeight: 44)
        .background(hovered == folder.id ? AeroricColor.hover : .clear, in: Capsule(style: .continuous))
        .onHover { inside in hovered = inside ? folder.id : (hovered == folder.id ? nil : hovered) }
        .help(folder.path)
    }

    private func choose() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = true
        panel.prompt = "关联"
        panel.message = "选择要关联到「\(project.title)」的文件夹"
        guard panel.runModal() == .OK else { return }
        Task {
            error = nil
            for url in panel.urls {
                if let message = await book.link(url, to: project.id) {
                    error = "\(url.lastPathComponent)：\(message)"
                }
            }
        }
    }

    static func tilde(_ path: String) -> String {
        let home = NSHomeDirectory()
        return path.hasPrefix(home) ? "~" + path.dropFirst(home.count) : path
    }
}

/// Which harness gets the linked folders (checked 2026-10-29): Claude and OMP
/// take `--add-dir`, Codex writes them as writable roots and reads anywhere;
/// DeepSeek Harness has no such option.
public enum LinkedFolderReach {
    public static func reaches(agent: String?) -> Bool { agent != "dsh" }

    public static func text(count: Int, agent: String?) -> String {
        reaches(agent: agent) ? "\(count) 个，下一次启动时带上" : "\(count) 个，这个 Harness 看不到"
    }

    /// The session page's hint for a harness that cannot see them.
    public static func hint(count: Int, agent: String?) -> String? {
        guard count > 0, !reaches(agent: agent) else { return nil }
        return "\(AgentGlyph.displayName(for: agent)) 看不到这个项目的 \(count) 个关联文件夹"
    }
}
