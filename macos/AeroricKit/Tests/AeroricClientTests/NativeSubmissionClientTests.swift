import XCTest
import AeroricProtocol
@testable import AeroricClient

/// Static contract fixture / pure decoders only. No connection, host, home or UI.
final class NativeSubmissionClientTests: XCTestCase {
    private var request: NativeTaskSubmission { submission() }

    private func submission(clarify: Bool = false) -> NativeTaskSubmission {
        NativeTaskSubmission(
            submissionId: "fixture-submission", projectId: "fixture-project",
            configurationId: "custom-fixture", configurationVersion: "fixture-v1",
            agent: "custom-fixture", prompt: "目标：保留\n原文 / \\\"",
            permissionMode: "ask", permissionAgent: "custom-fixture", selectedModel: "fixture/model",
            clarify: clarify
        )
    }

    private func accepted(_ request: NativeTaskSubmission? = nil) -> JSONValue {
        let request = request ?? self.request
        let fields = request.json.fields.filter {
            !["prompt", "attachments", "worktree", "recipeId", "team", "review", "evolution", "clarify"]
                .contains($0.key)
        }
        return .object(fields.merging([
            "state": .string("accepted"), "taskId": .string(request.submissionId),
            "reasons": .array([]), "options": .array([
                ("configuration", true), ("model", true), ("effort", false), ("permission", true),
                ("attachments", false), ("worktree", false), ("recipe", false),
                ("team", false), ("review", false), ("evolution", false),
                ("clarify", request.clarify),
            ].map { .object(["option": .string($0.0), "applied": .bool($0.1)]) }),
        ], uniquingKeysWith: { _, new in new }))
    }

    func testRustFixturePinsCanonicalFingerprintAndWire() throws {
        var root = URL(fileURLWithPath: #filePath)
        for _ in 0..<5 { root.deleteLastPathComponent() }
        let fixtureURL = root.appendingPathComponent("contracts/native/v1/fixtures/session-submission.json")
        let fixture = JSONValue.from(try JSONSerialization.jsonObject(with: Data(contentsOf: fixtureURL)))
        XCTAssertEqual(request.json, fixture["request"])
        XCTAssertEqual(request.fingerprint, "sha256:cb156eef8bed21a31bc0ad0a020b528c229423b5cd6f6eea12bcfa5efd256e26")
        XCTAssertEqual(request.fingerprintMaterial.count, 216)
        XCTAssertNil(request.json["model"])
        XCTAssertNil(request.json["effort"])
        XCTAssertEqual(request.json["selectedModel"], .string("fixture/model"))
    }

    func testAcceptedReceiptRequiresEveryOriginalOptionAndIdentity() throws {
        let receipt = try WorkspaceClient.decodeSubmissionReceipt(accepted(), request: request)
        XCTAssertEqual(receipt.taskId, request.submissionId)
        for field in ["taskId", "projectId", "configurationId", "configurationVersion", "fingerprint",
                      "agent", "selectedModel", "permissionMode", "permissionAgent"] {
            var wrong = accepted().fields
            wrong[field] = .string("different")
            XCTAssertThrowsError(try WorkspaceClient.decodeSubmissionReceipt(.object(wrong), request: request), field)
        }
        var missing = accepted().fields
        missing["options"] = .array([])
        XCTAssertThrowsError(try WorkspaceClient.decodeSubmissionReceipt(.object(missing), request: request))
        var duplicated = accepted().fields
        var options = try XCTUnwrap(duplicated["options"]?.arrayValue)
        options[0] = options[1]
        duplicated["options"] = .array(options)
        XCTAssertThrowsError(try WorkspaceClient.decodeSubmissionReceipt(.object(duplicated), request: request))
        var ignored = accepted().fields
        var ignoredOptions = try XCTUnwrap(ignored["options"]?.arrayValue)
        ignoredOptions[1] = .object(["option": .string("model"), "applied": .bool(false)])
        ignored["options"] = .array(ignoredOptions)
        XCTAssertThrowsError(try WorkspaceClient.decodeSubmissionReceipt(.object(ignored), request: request))
    }

    /// B4-CU-01: core always reports clarify, so both values must round-trip.
    func testAcceptedReceiptCarriesClarifyEitherWay() throws {
        for clarify in [false, true] {
            let request = submission(clarify: clarify)
            XCTAssertEqual(try WorkspaceClient.decodeSubmissionReceipt(accepted(request), request: request).state,
                           "accepted")
            let lookup: JSONValue = .object([
                "schemaVersion": .number("1"), "submissionId": .string(request.submissionId),
                "fingerprint": .string(request.fingerprint), "receipt": accepted(request),
            ])
            XCTAssertEqual(try WorkspaceClient.decodeSubmissionLookup(lookup, request: request)?.state, "accepted")
            var flipped = accepted(request).fields
            var options = try XCTUnwrap(flipped["options"]?.arrayValue)
            options[10] = .object(["option": .string("clarify"), "applied": .bool(!clarify)])
            flipped["options"] = .array(options)
            XCTAssertThrowsError(try WorkspaceClient.decodeSubmissionReceipt(.object(flipped), request: request))
        }
    }

    func testMissingReceiptRemainsUnknownAndWrongLookupNeverClearsInput() throws {
        var lookup: [String: JSONValue] = [
            "schemaVersion": .number("1"), "submissionId": .string(request.submissionId),
            "fingerprint": .string(request.fingerprint), "receipt": .null,
        ]
        XCTAssertNil(try WorkspaceClient.decodeSubmissionLookup(.object(lookup), request: request))
        lookup["fingerprint"] = .string("wrong")
        XCTAssertThrowsError(try WorkspaceClient.decodeSubmissionLookup(.object(lookup), request: request))
        lookup["fingerprint"] = .string(request.fingerprint)
        lookup["receipt"] = accepted()
        XCTAssertEqual(try WorkspaceClient.decodeSubmissionLookup(.object(lookup), request: request)?.state, "accepted")
    }

    func testDirectoryKeepsBlockedReasonsAndRejectsDifferentScopeAndVersion() throws {
        var catalog: [String: JSONValue] = [
            "schemaVersion": .number("1"), "projectId": .string("p"), "revision": .string("v1"),
            "asOf": .string("42"), "configurations": .array([.object([
                "id": .string("custom-fixture"), "version": .string("v1"), "displayName": .string("Fixture"),
                "agent": .string("custom-fixture"), "harness": .string("codex"),
                "models": .array([]), "efforts": .array([]), "permissionModes": .array([]),
                "blockedReasons": .array([.string("missing-model-directory")]),
            ])]),
        ]
        XCTAssertEqual(try WorkspaceClient.decodeConfigurations(.object(catalog), projectID: "p").configurations.first?.blockedReasons,
                       ["missing-model-directory"])
        XCTAssertThrowsError(try WorkspaceClient.decodeConfigurations(.object(catalog), projectID: "other"))
        catalog["revision"] = .string("v2")
        XCTAssertThrowsError(try WorkspaceClient.decodeConfigurations(.object(catalog), projectID: "p"))
    }
}
