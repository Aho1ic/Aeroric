import XCTest

@testable import AeroricProtocol

/// The Swift half of the P2-01 three-language contract test. Every case reads a
/// committed `contracts/native/v1/` document — the same bytes the Rust DTOs and
/// `src/lib/nativeHostProtocol.ts` decode — so a rule that only one language
/// enforces shows up here instead of in a user's session.
///
/// A missing artifact fails the test rather than skipping it: `Data(contentsOf:)`
/// throws, and a golden test that silently no-ops is how a contract goes untested.
final class ProtocolGoldenTests: XCTestCase {
    private static let repoRoot: URL = {
        var url = URL(fileURLWithPath: #filePath)
        // file -> AeroricProtocolTests -> Tests -> AeroricKit -> macos -> repo root
        for _ in 0..<5 { url.deleteLastPathComponent() }
        return url
    }()

    private func document(_ relative: String) throws -> JSONValue {
        let url = Self.repoRoot.appendingPathComponent("contracts/native/v1/\(relative)")
        let data = try Data(contentsOf: url)
        return JSONValue.from(try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]))
    }

    private func fixture(_ name: String) throws -> JSONValue { try document("fixtures/\(name)") }

    /// A JSON number read as an `Int` without the type checker having to unwind a
    /// chain of optionals, `??` and arithmetic in one expression.
    private func number(_ value: JSONValue?) -> Int { value?.numberText.flatMap(Int.init) ?? -1 }

    // MARK: - constants

    func testConstantsMatchThePublishedArtifact() throws {
        let constants = try document("envelope.schema.json")["constants"]?.fields ?? [:]
        let texts: (String) -> [String] = { key in
            (constants[key]?.arrayValue ?? []).compactMap(\.stringValue)
        }
        XCTAssertEqual(constants["protocolVersion"]?.numberText, "1")
        XCTAssertEqual(ProtocolCodec.protocolVersion, 1)
        XCTAssertEqual(texts("taskStatuses"), TaskStatus.allCases.map(\.rawValue))
        XCTAssertEqual(texts("protocolFamilies"), ProtocolFamily.allCases.map(\.rawValue))
        XCTAssertEqual(texts("permissionModes"), PermissionMode.allCases.map(\.rawValue))
        XCTAssertEqual(texts("builtInAgents"), BuiltInAgent.allCases.map(\.rawValue))
        XCTAssertEqual(texts("errorCodes"), WireErrorCode.emittable.map(\.text))
        XCTAssertEqual(constants["limits"]?["maxFrameBytes"]?.numberText,
                       String(ProtocolCodec.maxFrameBytes))
        XCTAssertEqual(constants["limits"]?["maxTerminalChunkRawBytes"]?.numberText,
                       String(BytesBase64.maxRawBytes))
        XCTAssertEqual(constants["limits"]?["replay"]?["maxEvents"]?.numberText,
                       String(ProtocolCodec.replayMaxEvents))
    }

    func testPublishedNamesDecodeIntoRealCases() throws {
        // Drift in either direction breaks a client: a published name the types
        // reject, or a case the types have that nothing advertises.
        let constants = try document("envelope.schema.json")["constants"]?.fields ?? [:]
        for text in (constants["taskStatuses"]?.arrayValue ?? []).compactMap(\.stringValue) {
            XCTAssertEqual(TaskStatus(rawValue: text)?.rawValue, text)
        }
        for text in (constants["builtInAgents"]?.arrayValue ?? []).compactMap(\.stringValue) {
            XCTAssertNotNil(BuiltInAgent(rawValue: text), text)
        }
        for text in (constants["protocolFamilies"]?.arrayValue ?? []).compactMap(\.stringValue) {
            XCTAssertNotNil(ProtocolFamily(rawValue: text), text)
        }
        XCTAssertEqual(TaskStatus.allCases.filter(\.isLive).map(\.rawValue),
                       ["pending", "running", "input_required", "detached"])
        for code in WireErrorCode.emittable {
            XCTAssertEqual(WireErrorCode(text: code.text), code, "\(code.text) must not fall back")
        }
    }

    // MARK: - envelope

    func testSuccessAndFailureFrames() throws {
        let success = try fixture("success.json")
        guard case .success(.request(let request)) = ProtocolCodec.frame(success["request"] ?? .null)
        else { return XCTFail("the success fixture's request must decode as a request") }
        XCTAssertEqual(request.method, "terminal.run_task")
        XCTAssertEqual(request.idempotencyKey, "op-1")
        XCTAssertEqual(request.params["expectedRevision"]?.stringValue, "7",
                       "a revision must arrive as a decimal string")

        guard case .success(let answer) = ProtocolCodec.frame(success["response"] ?? .null),
              case .success(let body) = answer else {
            return XCTFail("the response must decode as a success frame")
        }
        XCTAssertEqual(body.result["revision"]?.stringValue, "8")
        XCTAssertEqual(body.result["generation"]?.stringValue, "2")

        let error = try fixture("error.json")
        guard case .success(let refusal) = ProtocolCodec.frame(error["response"] ?? .null),
              case .failure(let failure) = refusal else {
            return XCTFail("the response must decode as a failure frame")
        }
        XCTAssertEqual(failure.error.code, .revisionConflict)
        XCTAssertFalse(failure.error.retryable, "a conflict must not be retried blindly")
        XCTAssertEqual(failure.error.detail?["actualRevision"]?.stringValue, "9")
    }

    func testUnknownFieldsCodesAndTopicsSurvive() throws {
        let case1 = try fixture("unknown.json")
        guard case .success(let extra) = ProtocolCodec.frame(case1["frameWithUnknownFields"] ?? .null),
              case .success(let body) = extra else { return XCTFail("expected a success frame") }
        XCTAssertEqual(body.additional["hostFeature"], .object(["enabled": .bool(true)]),
                       "a newer host's addition must be carried, not dropped")

        guard let decoded = RpcError.decode(case1["unknownCode"] ?? .null) else {
            return XCTFail("the unknown code object must still decode")
        }
        XCTAssertEqual(decoded.code, .other("QUOTA_EXHAUSTED"))
        XCTAssertEqual(decoded.code.text, "QUOTA_EXHAUSTED", "the text is what the user gets told")
        XCTAssertFalse(decoded.code.isRetryableByDefault)
        XCTAssertEqual(decoded.json["code"]?.stringValue, "QUOTA_EXHAUSTED")

        guard case .success(let later) = ProtocolCodec.frame(case1["unknownEvent"] ?? .null),
              case .event(let event) = later else { return XCTFail("expected an event frame") }
        XCTAssertEqual(event.event, "some.newer.topic")
        XCTAssertEqual(event.seq, Quantity("42"))
        XCTAssertEqual(case1["refusedUnknownMutating"]?["method"]?.stringValue,
                       "terminal.frobnicate_task")
    }

    func testRejectedFrameShapes() throws {
        let rejected = try fixture("terminal.json")["frameVectors"]?["rejected"]?.fields ?? [:]
        // The blank-line rule lives in the byte path, where size and UTF-8 are known.
        let blank = Data((rejected["empty"]?.stringValue ?? "   ").utf8)
        XCTAssertEqual(ProtocolCodec.decodeLine(blank), .failure(.empty))
        for key in ["noMethod", "noVersion", "numericSeq"] {
            guard case .failure(let failure) = ProtocolCodec.frame(rejected[key] ?? .null) else {
                return XCTFail("\(key) must be refused")
            }
            guard case .malformed = failure else {
                return XCTFail("\(key) failed as \(failure), expected malformed")
            }
        }
        // A status nobody knows stays verbatim in `result` rather than defaulting.
        guard case .success(let parsed) = ProtocolCodec.frame(rejected["unknownStatus"] ?? .null),
              case .success(let body) = parsed else { return XCTFail("expected success") }
        XCTAssertEqual(body.result["status"]?.stringValue, "vibrating")
        XCTAssertNil(TaskSnapshot.decode(.object([
            "id": .string("t"), "projectId": .string("p"),
            "status": .string("vibrating"), "agent": .object(["builtIn": .string("codex")]),
        ])), "an unrecognised status must be refused, never defaulted")
    }

    func testTransportLimits() throws {
        let oversized = Data(repeating: 0x20, count: ProtocolCodec.maxFrameBytes + 1)
        XCTAssertEqual(ProtocolCodec.decodeLine(oversized),
                       .failure(.tooLarge(bytes: ProtocolCodec.maxFrameBytes + 1,
                                          limit: ProtocolCodec.maxFrameBytes)))
        XCTAssertEqual(ProtocolCodec.decodeLine(Data([0xFF, 0xFE, 0x00, 0x01])), .failure(.notUtf8))
        XCTAssertEqual(ProtocolCodec.decodeLine(Data()), .failure(.empty))
    }

    func testMax64BitCursorRoundTripsExactly() throws {
        let accepted = try fixture("terminal.json")["frameVectors"]?["accepted"]?["maxU64Seq"] ?? .null
        guard case .success(let decoded) = ProtocolCodec.frame(accepted),
              case .event(let event) = decoded else { return XCTFail("expected an event frame") }
        XCTAssertEqual(event.seq.text, "18446744073709551615")
        XCTAssertEqual(event.seq.uint64Value, .max)
        let line = try ProtocolCodec.encodeLine(.event(event))
        guard case .success(let again) = ProtocolCodec.parse(line), case .event(let second) = again else {
            return XCTFail("re-decode must work")
        }
        XCTAssertEqual(second, event, "a cursor that loses precision is silent data loss")
    }

    // MARK: - terminal

    func testBase64VectorsFromTheArtifact() throws {
        let vectors = try fixture("terminal.json")["base64"]?.fields ?? [:]
        for vector in vectors["valid"]?.arrayValue ?? [] {
            let text = vector["text"]?.stringValue ?? ""
            let expected = (vector["bytes"]?.arrayValue ?? []).compactMap(\.numberText).map { UInt8($0)! }
            XCTAssertEqual(BytesBase64(text)?.bytes, expected, "\(text) decoded wrongly")
        }
        for case let .string(text) in vectors["invalid"]?.arrayValue ?? [] {
            XCTAssertNil(BytesBase64(text), "\(text) must not decode")
        }
        // The runtime's own reader is the counter-example: it accepts what the
        // published table refuses, which is why the table is shared rather than each
        // language paraphrasing its own idea of base64.
        XCTAssertNil(BytesBase64("Y WJj"))
        XCTAssertEqual(Data(base64Encoded: "Y WJj", options: [.ignoreUnknownCharacters])?.count, 3)
    }

    func testTerminalPlane() throws {
        let case1 = try fixture("terminal.json")
        guard case .success(let outputFrame) = ProtocolCodec.frame(case1["output"] ?? .null),
              case .event(let event) = outputFrame,
              let output = TerminalOutput.decode(event.data) else {
            return XCTFail("terminal.output must decode")
        }
        XCTAssertEqual(output.identity.taskId, "t")
        XCTAssertEqual(output.identity.generation.uint64Value, 2)
        XCTAssertEqual(output.bytes?.bytes, Array("abc".utf8))
        XCTAssertNil(output.reset, "an absent flag stays absent")

        guard case .success(let gapFrame) = ProtocolCodec.frame(case1["gap"] ?? .null),
              case .event(let event) = gapFrame,
              let gap = TerminalGap.decode(event.data) else {
            return XCTFail("terminal.gap must decode")
        }
        XCTAssertEqual(gap.reason, .snapshotRequired)
        XCTAssertTrue(gap.reset, "a compaction must tell the renderer to reset")
        XCTAssertEqual(gap.truncated, true)

        guard let lease = TerminalLease.decode(case1["lease"] ?? .null) else {
            return XCTFail("the lease must decode")
        }
        XCTAssertEqual(lease.holder, "swiftui-window-3")
        XCTAssertEqual(lease.acquiredAt, "1760000000000", "kept as text so no bit is lost")
        XCTAssertFalse(TerminalResize(cols: 0, rows: 24).isMeaningful)
        XCTAssertTrue(TerminalResize(cols: 80, rows: 24).isMeaningful)
    }

    // MARK: - tasks and sessions

    func testResumeTableFromTheArtifact() throws {
        let rule = try fixture("legacy-task.json")["resumeRule"]?.fields ?? [:]
        let record = { (status: String, transcript: Bool, discarded: Bool) in
            var fields: [String: JSONValue] = [
                "id": .string("t"), "projectId": .string("p"), "status": .string(status),
                "agent": .object(["builtIn": .string("codex")]),
            ]
            if transcript {
                fields["sessionOwner"] = .object([
                    "family": .string("codex"), "sessionId": .string("s1"),
                ])
            }
            if discarded { fields["worktreeDiscarded"] = .bool(true) }
            return TaskSnapshot.decode(.object(fields))
        }
        var checked = 0
        for (key, transcript) in [("withTranscript", true), ("withoutTranscript", false)] {
            for (status, expected) in rule[key]?.fields ?? [:] {
                XCTAssertEqual(record(status, transcript, false)?.canResume, expected.boolValue,
                               "\(status) transcript=\(transcript)")
                checked += 1
            }
        }
        XCTAssertGreaterThanOrEqual(checked, 13)
        XCTAssertEqual(record("done", true, true)?.canResume,
                       rule["discardedWorktreeWithTranscript"]?.boolValue)
    }

    func testLegacyTaskReadsWithoutAMigration() throws {
        let case1 = try fixture("legacy-task.json")
        guard let task = TaskSnapshot.decode(case1["stored"] ?? .null) else {
            return XCTFail("a legacy record must read")
        }
        XCTAssertEqual(task.status, .interrupted)
        XCTAssertEqual(task.agent.declaredFamily, .codex)
        XCTAssertFalse(task.hasSession)
        XCTAssertFalse(task.canResume, "no transcript means nothing to resume")
        XCTAssertNil(task.sessionOwner)
        XCTAssertEqual(task.additional["sessionCodexLike"], .bool(true),
                       "legacy fields survive in `additional`")
        XCTAssertEqual(case1["expectations"]?["canResume"]?.boolValue, task.canResume)
        XCTAssertEqual(task.revision, Quantity("0"), "an absent counter reads as 0, not a guess")

        let pathOnly = TaskSnapshot.decode(.object([
            "id": .string("t"), "projectId": .string("p"), "status": .string("done"),
            "agent": .object(["builtIn": .string("omp")]),
            "sessionOwner": .object([
                "family": .string("omp"), "sessionId": .string(""), "path": .string("/s.jsonl"),
            ]),
        ]))
        XCTAssertEqual(pathOnly?.hasSession, true, "an id or a path counts, as on the shipping client")
        XCTAssertEqual(pathOnly?.canResume, true)
    }

    func testSessionOwnershipAndOpaqueContent() throws {
        let case1 = try fixture("session.json")
        let owner = SessionOwner.decode(case1["owner"] ?? .null)
        XCTAssertEqual(owner?.family, .omp)
        XCTAssertEqual(owner?.source, .native)
        XCTAssertEqual(owner?.recordedBy, "my-codex-like-profile")
        let handoff = SessionOwner.decode(case1["handoff"] ?? .null)
        XCTAssertEqual(handoff?.source, .handoff)
        XCTAssertEqual(handoff?.originSessionId, "s1")
        XCTAssertNil(handoff?.path, "an absent path stays absent, not null")

        guard let page = SessionPage.decode(case1["page"] ?? .null) else {
            return XCTFail("the page must decode")
        }
        XCTAssertEqual(page.nextCursor, "opaque-cursor-2")
        XCTAssertTrue(page.hasMore)
        let blocks = page.messages.first?.blocks ?? []
        XCTAssertEqual(blocks.count, 2)
        XCTAssertEqual(blocks.first, .text(kind: "text", text: "kept verbatim"))
        guard case .some(.unknown(let opaque)) = blocks.last else {
            return XCTFail("an unrecognised block must stay opaque")
        }
        XCTAssertEqual(opaque.kind, "mystery_tool")
        XCTAssertEqual(opaque.sourceId, "b7")
        XCTAssertEqual(opaque.originFamily, .claude)
    }

    // MARK: - surfaces

    func testMethodSurfaceIsWhatADR002Froze() throws {
        let methods = try document("methods.schema.json")
        let rows = methods["methods"]?.arrayValue ?? []
        // ADR-006: 641 names minus the 109 prefix twins that became aliases.
        XCTAssertEqual(rows.count, 532)
        // `count` must equal the array it advertises, and every method must be
        // classified exactly once.
        XCTAssertEqual(number(methods["count"]), rows.count)
        let classified = number(methods["readCount"]) + number(methods["mutatingCount"])
        XCTAssertEqual(classified, rows.count)
        var names: [String] = []
        for row in rows {
            let fields = row.fields
            guard let name = fields["name"]?.stringValue else { return XCTFail("row without a name") }
            names.append(name)
            XCTAssertFalse(name.contains("/"), name)
            XCTAssertNotNil(name.range(of: "^[a-z0-9_]+\\.[A-Za-z0-9_]+$", options: .regularExpression),
                            name)
            XCTAssertEqual(fields["legacyCommand"]?.stringValue,
                           name.split(separator: ".").last.map(String.init),
                           "ADR-002 keeps the leaf equal to the legacy command")
        }
        XCTAssertEqual(Set(names).count, names.count, "two methods share a wire name")
        XCTAssertTrue(names.contains("terminal.run_task"))
        XCTAssertFalse(names.contains("task.start"),
                       "plan §2.3's illustrative name predates the derived surface")
    }

    /// ADR-006 in Swift: the twins are aliases, an alias must point at a published
    /// method, and only a mapping with no outstanding diff may be `proven`.
    func testPrefixTwinsAreAliasesNotMethods() throws {
        let methods = try document("methods.schema.json")
        let published = Set((methods["methods"]?.arrayValue ?? []).compactMap { $0.fields["name"]?.stringValue })
        let aliases = methods["twinAliases"]?.arrayValue ?? []
        XCTAssertEqual(aliases.count, 109)
        XCTAssertEqual(number(methods["aliasSummary"]?["proven"]), 77)
        XCTAssertEqual(number(methods["aliasSummary"]?["pending"]), 32)
        XCTAssertEqual(methods["aliasSummary"]?["pendingBlocksHost"]?.boolValue, true)
        var proven = 0
        for alias in aliases {
            let fields = alias.fields
            let legacy = fields["legacyCommand"]?.stringValue ?? "?"
            XCTAssertNotNil(fields["method"]?.stringValue.flatMap { published.contains($0) ? Optional<Bool>.some(true) : nil }, "\(legacy) aliases an unpublished method")
            let diffEmpty = (fields["paramDiff"]?.arrayValue ?? []).isEmpty
            let isProven = fields["paramMapping"]?.stringValue == "proven"
            XCTAssertEqual(isProven, diffEmpty, "\(legacy): mapping status and diff disagree")
            if isProven { proven += 1 }
        }
        XCTAssertEqual(proven, 77)
    }

    func testPlatformMethodsAreCapabilityGated() throws {
        let methods = try document("methods.schema.json")
        let rows = methods["methods"]?.arrayValue ?? []
        let gated = rows.filter { $0.fields["requiresCapability"]?.stringValue != nil }
        XCTAssertEqual(gated.count, 21)
        XCTAssertEqual(number(methods["capabilityGatedCount"]), gated.count)
        for row in gated {
            let fields = row.fields
            let name = fields["name"]?.stringValue ?? "?"
            XCTAssertEqual(fields["requiresCapability"]?.stringValue, "wsl",
                           "\(name) is gated on the wrong capability")
            XCTAssertEqual(fields["category"]?.stringValue, "platform_specific",
                           "\(name) is gated but not platform-specific")
        }
        let ungated = rows.filter {
            $0.fields["category"]?.stringValue == "platform_specific"
                && $0.fields["requiresCapability"]?.stringValue == nil
        }
        var ungatedNames: Set<String> = []
        for row in ungated {
            if let name = row.fields["name"]?.stringValue { ungatedNames.insert(name) }
        }
        let crossPlatform: Set<String> = [
            "platform.get_platform_runtime_info", "platform.build_runnable_file_command",
        ]
        // The two cross-platform commands must stay callable on every host (ADR-003).
        XCTAssertEqual(ungatedNames, crossPlatform)
    }

    func testHostCapabilityReportDrivesTheUI() throws {
        let report = HostCapabilities.decode(.object([
            "protocolVersion": .number("1"), "buildVersion": .string("1.4.9"),
            "capabilities": .object([
                "wsl": .object(["name": .string("wsl"), "enabled": .bool(false),
                                "note": .string("本机没有 WSL")]),
                "ssh": .object(["name": .string("ssh"), "enabled": .bool(true)]),
            ]),
        ]))
        XCTAssertEqual(report?.protocolVersion, 1)
        XCTAssertEqual(report?.enabled("wsl"), false)
        XCTAssertEqual(report?.reason(for: "wsl"), "本机没有 WSL")
        XCTAssertEqual(report?.reason(for: "absent"), "absent is not reported by this host")
        XCTAssertEqual(report?.enabled("ssh"), true)
    }

    /// ADR-007 in Swift: the bridge table must cover each of the 38 legacy channels
    /// exactly once, and an undecided row must not look decided.
    func testEventTopicTableBridgesEveryLegacyChannel() throws {
        let topics = try document("event-topics.schema.json")
        let rows = topics["rows"]?.arrayValue ?? []
        XCTAssertEqual(rows.count, 39)
        XCTAssertEqual(number(topics["mapped"]), 3)
        XCTAssertEqual(number(topics["pending"]), 36)
        var claimed: [String: Int] = [:]
        for row in rows {
            let fields = row.fields
            let topic = fields["topic"]?.stringValue ?? "?"
            XCTAssertNotNil(topic.range(of: "^[a-z0-9_]+\\.[a-z0-9_]+$", options: .regularExpression), topic)
            for case let .string(channel) in fields["legacyChannels"]?.arrayValue ?? [] {
                claimed[channel, default: 0] += 1
            }
            if fields["status"]?.stringValue == "mapped" {
                XCTAssertTrue(fields["source"]?.stringValue?.contains("§") ?? false, "\(topic) is mapped without a source")
                let payload = fields["payload"]?.stringValue ?? ""
                XCTAssertTrue(payload.hasPrefix("reshaped:") || payload.hasPrefix("verbatim"), "\(topic) has no payload statement")
            } else {
                XCTAssertTrue((fields["plannedDirection"]?.stringValue ?? "").hasPrefix("pending"),
                              "\(topic) is undecided yet states a direction")
            }
        }
        XCTAssertEqual(claimed.count, 38)
        for case let .object(fields) in try document("events.schema.json")["events"]?.arrayValue ?? [] {
            guard let name = fields["name"]?.stringValue else { continue }
            XCTAssertEqual(claimed[name], 1, "\(name) must be claimed by exactly one topic")
        }
    }

    func testEventSurfaceIsCountable() throws {
        let events = try document("events.schema.json")
        let rows = events["events"]?.arrayValue ?? []
        XCTAssertEqual(number(events["count"]), rows.count)
        XCTAssertEqual(rows.count, 38)
        let names = rows.compactMap { $0.fields["name"]?.stringValue }
        XCTAssertEqual(Set(names).count, names.count)
        XCTAssertTrue(names.contains("task-status"))
    }
}
