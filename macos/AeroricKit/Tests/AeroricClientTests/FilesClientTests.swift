import AeroricClient
import XCTest

@testable import AeroricProtocol

/// Native replacement wire tests use the existing fake UDS host and real codec.
final class FilesClientTests: XCTestCase {
    private static let path = "notes/title.txt"
    private static let matchFields: [String: JSONValue] = [
        "start": .number("3"),
        "end": .number("8"),
        "matchText": .string("title"),
    ]

    private static func previewFile(matches: [JSONValue]) -> JSONValue {
        .object([
            "path": .string(path),
            "name": .string("title.txt"),
            "matches": .array(matches),
        ])
    }

    private static func previewResult(files: [JSONValue]) -> JSONValue {
        .object([
            "query": .string("title"),
            "replacement": .string("name"),
            "files": .array(files),
        ])
    }

    private func makeClient(preview: JSONValue) async throws -> (FilesClient, ScriptedServer) {
        let server = ScriptedServer()
        addTeardownBlock { server.stop() }
        server.start { line, sender in
            do {
                guard case let .request(request) = try ProtocolCodec.parse(line).get() else {
                    XCTFail("fake host received a non-request frame")
                    sender.close()
                    return
                }
                let result: JSONValue
                switch request.method {
                case "handshake":
                    result = .object(["clientId": .string("files-test-client")])
                case "files.replace_preview":
                    result = preview
                case "files.apply_replacements":
                    result = .object([
                        "filesChanged": .number("1"),
                        "replacementsApplied": .number("1"),
                        "replacementsSkipped": .number("0"),
                    ])
                default:
                    XCTFail("unexpected files client method: \(request.method)")
                    sender.close()
                    return
                }
                let reply = try ProtocolCodec.encodeLine(.success(SuccessFrame(
                    id: request.id, result: result, additional: [:]
                )))
                sender.full(reply.trimmingCharacters(in: .newlines))
            } catch {
                XCTFail("fake host codec failure: \(error)")
                sender.close()
            }
        }
        let connection = HostConnection(socketPath: server.path, tokenProvider: { "files-test-token" })
        addTeardownBlock { await connection.close() }
        try await connection.connect()
        return (FilesClient(connection: connection), server)
    }

    private func request(_ method: String, from server: ScriptedServer) throws -> RequestFrame {
        for line in server.recordedLines() {
            if case let .request(request) = try ProtocolCodec.parse(line).get(),
               request.method == method {
                return request
            }
        }
        return try XCTUnwrap(nil, "missing request for \(method)")
    }

    private func assertMalformed(
        _ result: JSONValue, file: StaticString = #filePath, line: UInt = #line
    ) async throws {
        let (client, _) = try await makeClient(preview: result)
        do {
            _ = try await client.replacePreview(
                projectId: "p1", path: Self.path, query: "title", replacement: "name"
            )
            XCTFail("malformed preview must not be silently accepted", file: file, line: line)
        } catch {
            XCTAssertTrue(
                error is FilesClient.MalformedReply,
                "expected MalformedReply, got \(error)", file: file, line: line
            )
        }
    }

    private func assertMalformedMatch(
        _ match: JSONValue, file: StaticString = #filePath, line: UInt = #line
    ) async throws {
        // A valid neighbour must not turn a dropped malformed row into success.
        try await assertMalformed(
            Self.previewResult(files: [
                Self.previewFile(matches: [.object(Self.matchFields), match]),
            ]),
            file: file, line: line
        )
    }

    func testPreviewAndApplyRoundTripKeepsUTF8OffsetsAndMatchText() async throws {
        var richMatch = Self.matchFields
        richMatch["replacementText"] = .string("name")
        richMatch["line"] = .number("1")
        richMatch["column"] = .number("2")
        richMatch["futureField"] = .object(["ignored": .bool(true)])
        let (client, server) = try await makeClient(preview: Self.previewResult(files: [
            Self.previewFile(matches: [.object(richMatch)]),
        ]))

        let preview = try await client.replacePreview(
            projectId: "p1", path: Self.path, query: "title", replacement: "name"
        )
        XCTAssertEqual(preview.query, "title")
        XCTAssertEqual(preview.replacement, "name")
        XCTAssertEqual(preview.files.count, 1)
        let file = try XCTUnwrap(preview.files.first)
        XCTAssertEqual(file.path, Self.path)
        XCTAssertEqual(file.matches.count, 1)
        let match = try XCTUnwrap(file.matches.first)
        XCTAssertEqual(match.start, 3)
        XCTAssertEqual(match.end, 8)
        XCTAssertEqual(match, FilesClient.ReplacementMatch(start: 3, end: 8, matchText: "title"))
        let original = Array("中title".utf8)
        XCTAssertEqual(String(decoding: original[3..<8], as: UTF8.self), "title")

        let previewRequest = try request("files.replace_preview", from: server)
        XCTAssertEqual(previewRequest.params, .object([
            "projectId": .string("p1"), "path": .string(Self.path),
            "query": .string("title"), "replacement": .string("name"),
        ]))
        let summary = try await client.applyReplacements(
            projectId: "p1", path: Self.path, replacement: preview.replacement, files: preview.files
        )
        XCTAssertEqual(summary.filesChanged, 1)
        XCTAssertEqual(summary.replacementsApplied, 1)
        XCTAssertEqual(summary.replacementsSkipped, 0)
        let applyRequest = try request("files.apply_replacements", from: server)
        XCTAssertEqual(applyRequest.params, .object([
            "projectId": .string("p1"), "path": .string(Self.path),
            "replacement": .string("name"),
            "matches": .array([.object(Self.matchFields)]),
        ]))
    }

    func testApplyEncodesAllTypedMatchesVerbatim() async throws {
        let (client, server) = try await makeClient(preview: Self.previewResult(files: []))
        let matches: [FilesClient.ReplacementMatch] = [
            .init(start: 3, end: 8, matchText: "title"),
            .init(start: 12, end: 20, matchText: "中😀\""),
        ]
        _ = try await client.applyReplacements(
            projectId: "p1", path: Self.path, replacement: "name",
            files: [
                .init(path: "other.txt", matches: [.init(start: 0, end: 5, matchText: "other")]),
                .init(path: Self.path, matches: matches),
            ]
        )
        let applyRequest = try request("files.apply_replacements", from: server)
        XCTAssertEqual(applyRequest.params, .object([
            "projectId": .string("p1"), "path": .string(Self.path),
            "replacement": .string("name"),
            "matches": .array([
                .object(Self.matchFields),
                .object([
                    "start": .number("12"), "end": .number("20"), "matchText": .string("中😀\""),
                ]),
            ]),
        ]))
    }

    func testEmptyPreviewIsValid() async throws {
        let (client, _) = try await makeClient(preview: .object([
            "query": .string(""), "replacement": .string(""), "files": .array([]),
            "futureField": .bool(true),
        ]))
        let preview = try await client.replacePreview(
            projectId: "p1", path: Self.path, query: "", replacement: ""
        )
        XCTAssertEqual(preview.query, "")
        XCTAssertEqual(preview.replacement, "")
        XCTAssertTrue(preview.files.isEmpty)
    }

    func testEmptyMatchListIsValid() async throws {
        let (client, _) = try await makeClient(preview: Self.previewResult(files: [
            .object(["path": .string(Self.path), "matches": .array([])]),
        ]))
        let preview = try await client.replacePreview(
            projectId: "p1", path: Self.path, query: "title", replacement: "name"
        )
        XCTAssertEqual(preview.files.count, 1)
        XCTAssertTrue(try XCTUnwrap(preview.files.first).matches.isEmpty)
    }

    func testZeroLengthMatchIsValid() async throws {
        let (client, _) = try await makeClient(preview: Self.previewResult(files: [
            Self.previewFile(matches: [.object([
                "start": .number("3"), "end": .number("3"), "matchText": .string(""),
            ])]),
        ]))
        let preview = try await client.replacePreview(
            projectId: "p1", path: Self.path, query: "title", replacement: "name"
        )
        let match = try XCTUnwrap(preview.files.first?.matches.first)
        XCTAssertEqual(match.start, 3)
        XCTAssertEqual(match.end, 3)
    }

    func testPreviewRejectsLegacyTupleMatches() async throws {
        try await assertMalformedMatch(.array([.number("3"), .number("8")]))
    }

    func testPreviewRejectsMissingMatchText() async throws {
        try await assertMalformedMatch(.object(["start": .number("3"), "end": .number("8")]))
    }

    func testPreviewRejectsNegativeOffsets() async throws {
        for field in ["start", "end"] {
            var fields = Self.matchFields
            fields[field] = .number("-1")
            try await assertMalformedMatch(.object(fields))
        }
    }

    func testPreviewRejectsInvertedRanges() async throws {
        var fields = Self.matchFields
        fields["start"] = .number("8")
        fields["end"] = .number("3")
        try await assertMalformedMatch(.object(fields))
    }

    func testPreviewRejectsMalformedMatchFields() async throws {
        try await assertMalformedMatch(.null)
        for field in ["start", "end"] {
            var fields = Self.matchFields
            fields.removeValue(forKey: field)
            try await assertMalformedMatch(.object(fields))
            for value: JSONValue in [
                .string("3"), .bool(true), .null, .number("3.5"),
                .number("18446744073709551616"),
            ] {
                fields[field] = value
                try await assertMalformedMatch(.object(fields))
            }
        }
        for value: JSONValue in [.null, .number("1")] {
            var fields = Self.matchFields
            fields["matchText"] = value
            try await assertMalformedMatch(.object(fields))
        }
    }

    func testPreviewRejectsMalformedFileEntries() async throws {
        for item: JSONValue in [
            .null,
            .array([]),
            .object(["matches": .array([])]),
            .object(["path": .number("3"), "matches": .array([])]),
            .object(["path": .string(Self.path)]),
            .object(["path": .string(Self.path), "matches": .null]),
            .object(["path": .string(Self.path), "matches": .object([:])]),
        ] {
            try await assertMalformed(Self.previewResult(files: [
                Self.previewFile(matches: [.object(Self.matchFields)]), item,
            ]))
        }
    }

    func testPreviewRequiresFilesArray() async throws {
        var fields = Self.previewResult(files: []).fields
        fields.removeValue(forKey: "files")
        try await assertMalformed(.object(fields))
        for value: JSONValue in [.null, .object([:]), .string("not an array")] {
            fields["files"] = value
            try await assertMalformed(.object(fields))
        }
    }

    func testPreviewRequiresStringQueryAndReplacement() async throws {
        for field in ["query", "replacement"] {
            var fields = Self.previewResult(files: []).fields
            fields.removeValue(forKey: field)
            try await assertMalformed(.object(fields))
            for value: JSONValue in [.null, .number("3"), .array([])] {
                fields[field] = value
                try await assertMalformed(.object(fields))
            }
        }
    }
}
