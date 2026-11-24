import XCTest
import AeroricProtocol
@testable import AeroricUI

@MainActor
final class ConversationModelTests: XCTestCase {
    // MARK: - Protocol blocks → UI content

    private func opaque(_ kind: String, sourceId: String?, payload: JSONValue) -> AeroricProtocol.ContentBlock {
        var fields: [String: JSONValue] = ["kind": .string(kind), "payload": payload]
        if let sourceId { fields["sourceId"] = .string(sourceId) }
        return .decode(.object(fields))
    }

    func testOpaqueToolBlocksAreClaimedByKind() {
        let use = opaque("tool_use", sourceId: "tu1",
                         payload: .object(["name": .string("Read"), "input": .object(["file_path": .string("a.rs")])]))
        XCTAssertEqual(SessionBlockMapping.content(use),
                       .toolUse(id: "tu1", name: "Read", input: "{\n  \"file_path\" : \"a.rs\"\n}"))
        let result = opaque("tool_result", sourceId: "tu1", payload: .object(["output": .string("ok")]))
        XCTAssertEqual(SessionBlockMapping.content(result), .toolResult(id: "tu1", output: "ok"))
        XCTAssertEqual(SessionBlockMapping.content(.text(kind: "thinking", text: "嗯")), .thinking(thinking: "嗯"))
        XCTAssertEqual(SessionBlockMapping.content(.text(kind: "text", text: "答")), .text(text: "答"))
        let mystery = opaque("mystery", sourceId: nil, payload: .null)
        guard case let .text(text) = SessionBlockMapping.content(mystery) else { return XCTFail("kept visible") }
        XCTAssertTrue(text.contains("mystery"), "an unclaimed block is shown by kind, never dropped")
    }

    /// Round-5 review (2026-11-18): a refused ExitPlanMode read "完成" once
    /// history loaded. A failed result stays failed and is not folded into
    /// a "已调用 n 个工具" group.
    func testAFailedResultMakesAFailedToolRow() {
        let failed = opaque("tool_result", sourceId: "x",
                            payload: .object(["output": .string("这是只读会话"), "failed": .bool(true)]))
        XCTAssertEqual(SessionBlockMapping.content(failed), .toolResult(id: "x", output: "这是只读会话", failed: true))
        let messages = [
            AeroricUI.SessionMessage(role: "assistant", content: [
                .toolUse(id: "x", name: "ExitPlanMode", input: "{}"),
                .toolUse(id: "y", name: "Read", input: "{}"),
                .toolUse(id: "z", name: "Read", input: "{}"),
            ], messageId: "a1"),
            AeroricUI.SessionMessage(role: "user", content: [
                .toolResult(id: "x", output: "这是只读会话", failed: true),
                .toolResult(id: "y", output: "1"),
                .toolResult(id: "z", output: "2"),
            ], messageId: "r1"),
        ]
        let items = TranscriptBuilder.items(from: messages, taskRunning: false)
        let standalone: [String: ToolCallState] = Dictionary(uniqueKeysWithValues: items.compactMap {
            if case let .tool(call) = $0 { return (call.id, call.state) }
            return nil
        })
        let grouped: [String: ToolCallState] = Dictionary(uniqueKeysWithValues: items.flatMap { item -> [(String, ToolCallState)] in
            if case let .toolGroup(_, calls) = item { return calls.map { ($0.id, $0.state) } }
            return []
        })
        XCTAssertEqual(standalone["x"], .failed, "shown on its own, not folded")
        XCTAssertEqual(standalone["y"] ?? grouped["y"], .done)
        XCTAssertEqual(ToolCallState.failed.activity, .failed)
    }

    // MARK: - Tool call pairing and states

    private func messages() -> [AeroricUI.SessionMessage] {
        [
            AeroricUI.SessionMessage(role: "user", content: [.text(text: "改一下")], messageId: "u1"),
            AeroricUI.SessionMessage(role: "assistant", content: [
                .text(text: "好的"),
                .toolUse(id: "a", name: "Read", input: #"{"file_path":"a.rs"}"#),
                .toolUse(id: "b", name: "Edit", input: #"{"file_path":"a.rs","old_string":"x\ny","new_string":"z"}"#),
            ], messageId: "a1"),
            AeroricUI.SessionMessage(role: "user", content: [.toolResult(id: "a", output: "1")], messageId: "r1"),
        ]
    }

    func testResultsPairWithCallsAndResultOnlyMessagesDisappear() {
        let items = TranscriptBuilder.items(from: messages(), taskRunning: true)
        XCTAssertEqual(items.count, 4, "user text, assistant text, two tools; the result-only turn is absorbed")
        guard case let .tool(read) = items[2], case let .tool(edit) = items[3] else { return XCTFail("tools") }
        XCTAssertEqual(read.state, .done)
        XCTAssertEqual(read.output, "1")
        XCTAssertEqual(edit.state, .running, "no result while the task runs")
        XCTAssertEqual(edit.stats, .init(added: 1, removed: 2))
        XCTAssertEqual(read.summary, "a.rs")
    }

    /// Prints what one derivation of a long session's turns costs (80 turns,
    /// 6 tool calls each). Before 2026-11-12 the live page paid it N+3 times
    /// per body for N turns, ~20 bodies a second while a turn ran.
    func testLongHistoryTurnDerivationTime() {
        var history: [AeroricUI.SessionMessage] = []
        for turn in 0..<80 {
            history.append(AeroricUI.SessionMessage(role: "user", content: [.text(text: "第 \(turn) 轮：改一下 a.rs")],
                                                    messageId: "u\(turn)"))
            var calls: [AeroricUI.SessionContent] = [.text(text: String(repeating: "说明。", count: 40))]
            for call in 0..<6 {
                calls.append(.toolUse(id: "t\(turn)-\(call)", name: call % 2 == 0 ? "Read" : "Edit",
                                      input: #"{"file_path":"a.rs","old_string":"x\ny","new_string":"z"}"#))
            }
            history.append(AeroricUI.SessionMessage(role: "assistant", content: calls, messageId: "a\(turn)"))
            history.append(AeroricUI.SessionMessage(
                role: "user", content: (0..<6).map { .toolResult(id: "t\(turn)-\($0)", output: String(repeating: "ok\n", count: 50)) },
                messageId: "r\(turn)"))
            history.append(AeroricUI.SessionMessage(role: "assistant", content: [.text(text: "完成。")], messageId: "f\(turn)"))
        }
        measure {
            let turns = TranscriptTurns.attach([], to: TranscriptTurns.group(TranscriptBuilder.items(from: history, taskRunning: false)))
            XCTAssertEqual(turns.count, 80)
        }
    }

    func testMissingResultAfterTheTaskEndsIsUnfinished() {
        let items = TranscriptBuilder.items(from: messages(), taskRunning: false)
        guard case let .tool(edit) = items[3] else { return XCTFail("tool") }
        XCTAssertEqual(edit.state, .unfinished)
        XCTAssertEqual(edit.state.activity, .idle)
    }

    func testFinishedCallsInARowFoldIntoOneGroup() {
        let calls = (0..<3).flatMap { index -> [SessionContent] in
            [.toolUse(id: "t\(index)", name: "Grep", input: #"{"pattern":"x"}"#), .toolResult(id: "t\(index)", output: "")]
        }
        let items = TranscriptBuilder.items(from: [AeroricUI.SessionMessage(role: "assistant", content: calls)], taskRunning: false)
        XCTAssertEqual(items.count, 1)
        guard case let .toolGroup(_, grouped) = items[0] else { return XCTFail("grouped") }
        XCTAssertEqual(grouped.count, 3)
        let two = Array(calls.prefix(4))
        XCTAssertEqual(TranscriptBuilder.items(from: [AeroricUI.SessionMessage(role: "assistant", content: two)],
                                               taskRunning: false).count, 1, "2026-10-27: two in a row fold too")
        let one = Array(calls.prefix(2))
        XCTAssertEqual(TranscriptBuilder.items(from: [AeroricUI.SessionMessage(role: "assistant", content: one)],
                                               taskRunning: false).count, 1)
        guard case .tool = TranscriptBuilder.items(from: [AeroricUI.SessionMessage(role: "assistant", content: one)],
                                                   taskRunning: false)[0] else { return XCTFail("one stays a line") }
    }

    func testOrphanResultStaysVisible() {
        let items = TranscriptBuilder.items(
            from: [AeroricUI.SessionMessage(role: "user", content: [.toolResult(id: "old", output: "done")])], taskRunning: false)
        guard case let .tool(call) = items.first else { return XCTFail("orphan kept") }
        XCTAssertEqual(call.output, "done")
    }

    func testStatsOnlyWhereTheArgumentsSayItExactly() {
        XCTAssertEqual(ToolCallPresentation.stats(name: "Write", input: #"{"content":"a\nb\n"}"#), .init(added: 2, removed: 0))
        XCTAssertEqual(ToolCallPresentation.stats(
            name: "MultiEdit", input: #"{"edits":[{"old_string":"a","new_string":"b\nc"},{"old_string":"d","new_string":""}]}"#),
            .init(added: 2, removed: 2))
        XCTAssertNil(ToolCallPresentation.stats(name: "Bash", input: #"{"command":"ls"}"#))
        XCTAssertNil(ToolCallPresentation.stats(name: "Edit", input: "not json"))
        XCTAssertEqual(ToolCallPresentation.symbol(for: "SomethingNew"), "wrench.and.screwdriver")
        XCTAssertEqual(ToolCallPresentation.summary(input: #"{"command":"cargo test\n--all"}"#), "cargo test")
        XCTAssertEqual(ToolCallPresentation.summary(input: "plain text\nmore"), "plain text")
    }

    // MARK: - Geometry

    func testColumnWidthClamps() {
        XCTAssertEqual(TranscriptGeometry.columnWidth(for: 1000), 680)
        XCTAssertEqual(TranscriptGeometry.columnWidth(for: 1250), 800)
        XCTAssertEqual(TranscriptGeometry.columnWidth(for: 2000), 880)
        XCTAssertEqual(TranscriptGeometry.columnWidth(for: 600), 568, "narrow centre keeps a 16pt gutter")
    }

    // MARK: - Picker options

    private func configuration(_ id: String, harness: String = "claude", blocked: [String] = [],
                               models: [String] = ["opus"], efforts: [String] = []) -> SessionConfiguration {
        SessionConfiguration(id: id, version: "1", title: id, agent: harness, model: nil, effort: nil,
                             options: .init(agents: [harness], models: models, efforts: efforts,
                                            permissionModes: ["ask", "full_access"]),
                             harness: harness, blockedReasons: blocked,
                             requiresModel: true, requiresPermissionIdentity: true)
    }

    func testConfigurationsGroupByHarnessAndKeepBlockedReasons() {
        let menu = ComposerPickerOptions.configurations(
            [configuration("dsh-main", harness: "dsh"), configuration("c1"), configuration("c2", blocked: ["缺少密钥"])],
            draft: NewSessionDraft())
        XCTAssertEqual(menu.sections.map(\.id), ["claude", "dsh"], "harness picker order, empty harnesses omitted")
        let blocked = menu.sections[0].options[1]
        XCTAssertFalse(blocked.isEnabled)
        XCTAssertEqual(blocked.disabledReason, "缺少密钥")
        XCTAssertNil(menu.note)
        XCTAssertEqual(ComposerPickerOptions.configurations([], draft: NewSessionDraft()).note, "宿主尚未提供 Agent 配置目录。")
    }

    func testDependentPickersExplainWhyTheyAreEmpty() {
        let draft = NewSessionDraft()
        XCTAssertEqual(ComposerPickerOptions.permissions(nil, draft: draft).note, "先选择 Agent 配置。")
        XCTAssertEqual(ComposerPickerOptions.models(nil, draft: draft).note, "先选择 Agent 配置。")
        XCTAssertNil(ComposerPickerOptions.efforts(configuration("c"), draft: draft), "no efforts → no capsule")
        let noModels = ComposerPickerOptions.models(configuration("c", models: []), draft: draft)
        XCTAssertTrue(noModels.isEmpty)
        XCTAssertNotNil(noModels.note)
    }

    func testSelectionMarksOnlyTheCurrentItem() {
        var draft = NewSessionDraft()
        let config = configuration("c1", efforts: ["low", "high"])
        draft.selectHarness("claude")
        draft.apply(config)
        draft.task.model = "opus"
        draft.task.effort = "high"
        draft.selectPermission("ask")
        XCTAssertEqual(ComposerPickerOptions.selectedConfiguration([config], draft: draft), config)
        XCTAssertEqual(ComposerPickerOptions.configurations([config], draft: draft).sections[0].options[0].selected, true)
        XCTAssertEqual(ComposerPickerOptions.models(config, draft: draft).sections[0].options.map(\.selected), [true])
        XCTAssertEqual(ComposerPickerOptions.efforts(config, draft: draft)?.sections[0].options.map(\.selected), [false, true])
        XCTAssertEqual(ComposerPickerOptions.permissions(config, draft: draft).sections[0].options.map(\.selected), [true, false])
        XCTAssertEqual(ComposerPickerOptions.permissionTitle("full_access"), "完全访问")
    }

    // MARK: - Scenarios

    func testEveryScenarioHasSixRecipesWithUniqueIDs() {
        // 2026-10-30 (B2): 科研 carries one card per research skill.
        for scenario in Scenario.allCases where scenario != .science {
            XCTAssertEqual(scenario.recipes.count, 6, scenario.title)
        }
        XCTAssertEqual(Scenario.science.recipes.count, 13)
        XCTAssertEqual(Set(ScenarioCatalog.recipes.map(\.id)).count, ScenarioCatalog.recipes.count)
        XCTAssertTrue(ScenarioCatalog.recipes.allSatisfy { !$0.template.isEmpty && $0.placeholderRange != nil })
    }

    func testTemplatesReplaceOrAppendButNeverSend() {
        let recipe = ScenarioCatalog.recipes[0]
        XCTAssertEqual(ScenarioCatalog.merged("旧", with: recipe, as: .replace), recipe.template)
        XCTAssertEqual(ScenarioCatalog.merged(" 旧 \n", with: recipe, as: .append), "旧\n\n" + recipe.template)
        XCTAssertEqual(ScenarioCatalog.merged("  ", with: recipe, as: .append), recipe.template)
    }

    // MARK: - Live command groups (2026-10-27)

    private func liveTool(_ id: String, _ status: LiveToolStatus, kind: LiveToolKind = .command) -> LiveItem {
        .tool(LiveTool(id: id, kind: kind, title: "ls \(id)", input: .null, output: "", status: status, result: nil))
    }

    func testConsecutiveLiveToolsFoldIntoOneCollapsedGroup() {
        let rows = LiveRowGrouping.rows([
            .userEcho(id: "u", text: "hi", steer: .none),
            liveTool("a", .completed), liveTool("b", .completed), liveTool("c", .running),
            .message(id: "m", text: "done"),
            liveTool("d", .failed),
        ])
        XCTAssertEqual(rows.count, 4)
        guard case let .tools(_, group) = rows[1] else { return XCTFail("grouped") }
        XCTAssertEqual(group.map(\.id), ["a", "b", "c"])
        XCTAssertEqual(LiveRowGrouping.title(group), "正在执行第 3 / 3 个指令")
        guard case .item(.tool) = rows[3] else { return XCTFail("a single call stays a line") }
        let done = [LiveTool](repeating: LiveTool(id: "x", kind: .command, title: "", input: .null, output: "",
                                                  status: .completed, result: nil), count: 2)
        XCTAssertEqual(LiveRowGrouping.title(done), "已执行 2 个指令")
        var mixed = done
        mixed[1].kind = .mcp
        XCTAssertEqual(LiveRowGrouping.title(mixed), "已调用 2 个工具")
    }

    /// 1123: thinking and tool calls with no reply between them are one row
    /// that shows only the latest step while it moves.
    func testThinkingAndToolsFoldIntoOneWorkRow() {
        let thought: (String, String) -> LiveItem = { .thought(id: $0, text: $1, summary: false) }
        let rows = LiveRowGrouping.rows([
            thought("t0", "look first"),
            .message(id: "m", text: "我先看一下现场"),
            liveTool("a", .completed), liveTool("b", .failed),
            thought("t1", "hm"),
            liveTool("c", .completed),
            thought("t2", "line one\nnext step: read the log\n"),
        ])
        XCTAssertEqual(rows.count, 3, "a thought alone stays a row; reply text ends a run")
        guard case .item(.thought) = rows[0] else { return XCTFail("lone thought") }
        guard case let .work(id, items) = rows[2] else { return XCTFail("work row") }
        XCTAssertEqual(id, "tool:a")
        XCTAssertEqual(items.count, 5)
        XCTAssertEqual(LiveRowGrouping.workTitle(items), "思考 2 次 · 执行 3 个指令")
        XCTAssertEqual(LiveRowGrouping.latestStep(items), "思考：next step: read the log")
        let running = items + [liveTool("d", .running)]
        XCTAssertEqual(LiveRowGrouping.latestStep(running), "正在执行：ls d")
        var mixed = items
        mixed[3] = .tool(LiveTool(id: "c", kind: .mcp, title: "", input: .null, output: "", status: .completed, result: nil))
        XCTAssertEqual(LiveRowGrouping.workTitle(mixed), "思考 2 次 · 调用 3 个工具")
        XCTAssertEqual(LiveRowGrouping.latestStep([thought("t", "")]), "正在思考…")
    }

    func testCodexFallbackMetadataWarningIsRecognised() {
        let warning = "Model metadata for `cline-pass/deepseek-v4.1-flash` not found. Defaulting to fallback metadata; this can degrade performance and cause issues."
        XCTAssertEqual(LiveNoticeRow.fallbackModel(in: warning), "cline-pass/deepseek-v4.1-flash")
        XCTAssertNil(LiveNoticeRow.fallbackModel(in: "Something else went wrong"))
    }
}
