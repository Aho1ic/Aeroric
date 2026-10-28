import SwiftUI

/// An imported session, read-only (M4 / Q24): the regular session page with
/// history from `imports.history` and a notice instead of the composer.
public struct ImportedSessionPane: View {
    let book: ImportBook
    let id: String

    public static let notice = "已导入的会话只可查看；继续对话将在后续版本支持。"

    public init(book: ImportBook, id: String) {
        self.book = book
        self.id = id
    }

    public var body: some View {
        if let session = book.session(id: id) {
            SessionRouteView(
                task: TaskStore.TaskEntry(id: session.id, projectID: session.projectId ?? "", title: session.title,
                                          status: "done", agent: session.agent, updatedAtMs: session.lastAtMs),
                actions: ImportedSessionActions(book: book), canSend: false,
                readOnlyNotice: Self.notice
            )
            .id(session.id)
        } else {
            AeroricEmptyState(systemImage: "tray", title: "导入的会话不可用",
                              message: "源文件可能已被删除，或导入授权已撤销。")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(AeroricColor.canvas)
        }
    }
}

@MainActor
struct ImportedSessionActions: SessionRouteActions {
    let book: ImportBook

    func loadHistory(taskID: String) async throws -> [SessionMessage] {
        try await book.messages(id: taskID)
    }
    func send(taskID: String, text: String) async throws {}
    func stop(taskID: String) async {}
    func respond(eventID: String, outcome: PendingRequestStore.Outcome) async throws {}
    func export(taskID: String) async throws {}
}
