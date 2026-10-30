import SwiftUI
import AeroricClient

/// An imported session (M4 / Q24): the regular session page with history from
/// `imports.history`. 2026-10-28: the composer's place holds the 接着聊
/// capsule — take the session over, add its folder as a project first, or
/// fork it into a new session.
public struct ImportedSessionPane: View {
    let book: ImportBook
    let id: String
    /// Harness availability and the default for a fork (Settings → Agents).
    let availability: [String: Bool]
    let preferredHarness: String
    /// Take the session over; `addingProject` is the folder to open first.
    let onContinue: (_ id: String, _ addingProject: String?) async throws -> Void
    /// Open a new-session draft in `projectID` with `harness` and `prompt`.
    let onFork: (_ projectID: String?, _ harness: String, _ prompt: String) -> Void

    @State private var busy = false
    @State private var errorText: String?

    public static let notice = "已导入的会话只可查看。"

    /// `从 Codex 导入 · 10/28 16:25` (2026-10-30), at the page's top left.
    public static func badge(agent: String, lastAtMs: UInt64, now: Date = Date()) -> String {
        "从 \(AgentGlyph.displayName(for: agent)) 导入 · \(TranscriptTurns.clock(lastAtMs, now: now))"
    }

    public init(book: ImportBook, id: String, availability: [String: Bool] = [:], preferredHarness: String = "codex",
                onContinue: @escaping (_ id: String, _ addingProject: String?) async throws -> Void = { _, _ in },
                onFork: @escaping (_ projectID: String?, _ harness: String, _ prompt: String) -> Void = { _, _, _ in }) {
        self.book = book
        self.id = id
        self.availability = availability
        self.preferredHarness = preferredHarness
        self.onContinue = onContinue
        self.onFork = onFork
    }

    public var body: some View {
        if let session = book.session(id: id) {
            ImportedTranscript(book: book, sessionID: session.id,
                               badge: Self.badge(agent: session.agent, lastAtMs: session.lastAtMs),
                               agentSessionID: session.sessionId) {
                footer(session)
            }
            .id(session.id)
        } else {
            AeroricEmptyState(systemImage: "tray", title: "导入的会话不可用",
                              message: "源文件可能已被删除，或导入授权已撤销。")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(AeroricColor.canvas)
        }
    }

    private func footer(_ session: ImportsClient.ImportedSession) -> some View {
        let continuation = ImportContinuation.of(agent: session.agent, projectID: session.projectId, cwd: session.cwd)
        return VStack(alignment: .leading, spacing: AeroricSpace.s) {
            if let errorText {
                Label(errorText, systemImage: "exclamationmark.triangle")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.danger)
                    .padding(.horizontal, AeroricSpace.l)
            }
            ImportedSessionBar(
                agent: session.agent, continuation: continuation, availability: availability,
                preferredHarness: preferredHarness, busy: busy,
                onContinue: { takeOver(session, continuation) },
                onFork: { harness in fork(session, harness: harness) }
            )
        }
    }

    private func takeOver(_ session: ImportsClient.ImportedSession, _ continuation: ImportContinuation) {
        guard !busy else { return }
        busy = true
        errorText = nil
        Task { @MainActor in
            defer { busy = false }
            do {
                if case let .addProject(path) = continuation {
                    try await onContinue(session.id, path)
                } else {
                    try await onContinue(session.id, nil)
                }
            } catch {
                errorText = "接着聊失败：\(error)"
            }
        }
    }

    private func fork(_ session: ImportsClient.ImportedSession, harness: String) {
        guard !busy else { return }
        busy = true
        errorText = nil
        Task { @MainActor in
            defer { busy = false }
            let messages = (try? await book.messages(id: session.id)) ?? []
            onFork(session.projectId, harness,
                   ImportForkPrompt.make(agent: session.agent, title: session.title, messages: messages))
        }
    }
}
