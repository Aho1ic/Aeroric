#if DEBUG
import SwiftUI

/// DEBUG-only session sample (D2): fixed transcript with every tool state and
/// a pending approval, rendered by the real `SessionRouteView` without a host.
/// Launch with `AERORIC_REVIEW_SESSION_SAMPLE=1`.
public enum SessionSampleReviewEntry {
    public static let environmentKey = "AERORIC_REVIEW_SESSION_SAMPLE"

    public static func isActive(environment: [String: String]) -> Bool {
        environment[environmentKey] == "1"
    }
}

public struct SessionSampleReview: View {
    @State private var running = true
    @State private var store = SessionSampleReview.sampleStore()

    public init() {}

    public var body: some View {
        SessionRouteView(
            task: TaskStore.TaskEntry(id: running ? "sample-running" : "sample-done", projectID: "sample",
                                      title: "样例会话", status: running ? "running" : "done", agent: "dsh"),
            actions: SampleActions(), canSend: true, pendingStore: store
        )
        .id(running)
        .toolbar {
            Toggle("运行中", isOn: $running).toggleStyle(.switch)
        }
        .frame(minWidth: 900, minHeight: 640)
    }

    static func sampleStore() -> PendingRequestStore {
        let store = PendingRequestStore()
        store.upsert(.init(id: "approval-1", kind: .approval, sessionId: "sample", toolName: "Bash",
                           callId: "t7", reason: "运行 `cargo test --workspace`，会在项目目录写入 target/。"))
        return store
    }

    static let messages: [SessionMessage] = [
        SessionMessage(role: "user", content: [.text(text: "登录页在窄窗口下按钮被截断了，帮我修一下，并补一个测试。")], messageId: "u1"),
        SessionMessage(role: "assistant", content: [
            .thinking(thinking: "先看登录页的布局代码，确认按钮容器是否用了固定宽度。"),
            .text(text: "我先看一下登录页的布局。"),
            .toolUse(id: "t1", name: "Grep", input: #"{"pattern":"LoginView","path":"Sources"}"#),
            .toolUse(id: "t2", name: "Read", input: #"{"file_path":"Sources/Auth/LoginView.swift"}"#),
            .toolUse(id: "t3", name: "Read", input: #"{"file_path":"Sources/Auth/LoginButton.swift"}"#),
        ], messageId: "a1"),
        SessionMessage(role: "user", content: [
            .toolResult(id: "t1", output: "Sources/Auth/LoginView.swift:12: struct LoginView: View {"),
            .toolResult(id: "t2", output: "struct LoginView: View {\n    var body: some View {\n        HStack { … }.frame(width: 420)\n    }\n}"),
            .toolResult(id: "t3", output: "struct LoginButton: View { … }"),
        ], messageId: "r1"),
        SessionMessage(role: "assistant", content: [
            .text(text: "问题在 `LoginView` 把按钮行固定成了 **420pt**。改成随容器伸缩：\n\n- 去掉固定宽度\n- 按钮最小宽度 120pt，超出时换行"),
            .toolUse(id: "t4", name: "Edit", input: #"{"file_path":"Sources/Auth/LoginView.swift","old_string":"HStack { … }.frame(width: 420)\n","new_string":"ViewThatFits {\n    HStack { … }\n    VStack { … }\n}\n"}"#),
            .toolUse(id: "t5", name: "Write", input: #"{"file_path":"Tests/LoginViewTests.swift","content":"import XCTest\n\nfinal class LoginViewTests: XCTestCase {\n    func testNarrowLayout() {}\n}\n"}"#),
        ], messageId: "a2"),
        SessionMessage(role: "user", content: [
            .toolResult(id: "t4", output: "@@ -12,1 +12,4 @@\n-        HStack { … }.frame(width: 420)\n+        ViewThatFits {\n+            HStack { … }\n+            VStack { … }\n+        }"),
        ], messageId: "r2"),
        SessionMessage(role: "assistant", content: [
            .text(text: "改好了，现在跑测试确认。"),
            .toolUse(id: "t7", name: "Bash", input: #"{"command":"cargo test --workspace"}"#),
        ], messageId: "a3"),
    ]
}

@MainActor
private struct SampleActions: SessionRouteActions {
    func loadHistory(taskID: String) async throws -> [SessionMessage] { SessionSampleReview.messages }
    func send(taskID: String, text: String) async throws {}
    func stop(taskID: String) async {}
    func respond(eventID: String, outcome: PendingRequestStore.Outcome) async throws {}
    func export(taskID: String) async throws {}
}
#endif
