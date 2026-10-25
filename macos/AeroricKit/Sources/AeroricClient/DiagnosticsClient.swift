import AeroricProtocol
import Foundation

/// A13: the host's redacted diagnostics report (`diagnostics.export_report`).
public struct DiagnosticsClient: Sendable {
    public static let exportMethod = "diagnostics.export_report"

    public struct Report: Sendable, Equatable {
        public var fileName: String
        public var text: String

        public init(fileName: String, text: String) {
            self.fileName = fileName
            self.text = text
        }
    }

    public enum MalformedReply: Error { case malformed(String) }

    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func exportReport() async throws -> Report {
        // Collecting the report reads disk and the agent PATH; allow it time.
        let frame = try await connection.request(Self.exportMethod, params: .null, timeout: .seconds(60))
        return try Self.decode(frame.result)
    }

    public static func decode(_ value: JSONValue) throws -> Report {
        guard let name = value["fileName"]?.stringValue, !name.isEmpty,
              let text = value["text"]?.stringValue else {
            throw MalformedReply.malformed("diagnostics.export_report needs fileName and text")
        }
        return Report(fileName: name, text: text)
    }
}
