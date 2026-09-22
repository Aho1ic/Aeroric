import Foundation

/// Local protocol v1 in Swift (plan §2.3, Task P2-01).
///
/// The Rust crate `aeroric-protocol` is the source of truth: `pnpm
/// native:contracts --write` publishes `contracts/native/v1/`, and
/// `Tests/AeroricProtocolTests/ProtocolGoldenTests.swift` decodes exactly those
/// documents with this file. A rule that lives only here — or only there — is a bug
/// waiting for the first cross-language call, so the fixtures, not these types, are
/// the authority.
///
/// Two conventions carry through the whole file:
/// * Counters are decimal strings (`Quantity`). A `Double` cursor past 2^53-1 is
///   silent data loss, and a lost `seq` looks like a missed state change.
/// * Unrecognised content is preserved, never dropped or defaulted. `additional`
///   holds keys this build does not model, and an unknown enum text keeps what the
///   host actually said.

// MARK: - JSON tree

/// A JSON value that keeps numbers as text where precision matters.
public enum JSONValue: Equatable, Sendable {
    case null
    case bool(Bool)
    /// Kept as text: `JSONSerialization` turns every JSON number into an `NSNumber`,
    /// and the digits of a large integer are already gone by the time it does.
    case number(String)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public var stringValue: String? { if case let .string(text) = self { text } else { nil } }
    public var boolValue: Bool? { if case let .bool(value) = self { value } else { nil } }
    public var numberText: String? { if case let .number(text) = self { text } else { nil } }
    public var objectValue: [String: JSONValue]? { if case let .object(value) = self { value } else { nil } }
    public var arrayValue: [JSONValue]? { if case let .array(value) = self { value } else { nil } }
    public var isNull: Bool { if case .null = self { true } else { false } }

    public subscript(key: String) -> JSONValue? {
        if case let .object(fields) = self { fields[key] } else { nil }
    }

    /// The `params` / `result` / `data` payloads are open objects; a missing one
    /// reads as empty rather than failing the frame.
    public var fields: [String: JSONValue] { objectValue ?? [:] }

    static func from(_ object: Any) -> JSONValue {
        if object is NSNull { return .null }
        if let text = object as? String { return .string(text) }
        if let number = object as? NSNumber {
            if CFGetTypeID(number) == CFBooleanGetTypeID() { return .bool(number.boolValue) }
            return .number(number.stringValue)
        }
        if let array = object as? [Any] { return .array(array.map(JSONValue.from)) }
        if let dictionary = object as? [String: Any] {
            return .object(Dictionary(uniqueKeysWithValues: dictionary.map { ($0.key, JSONValue.from($0.value)) }))
        }
        // Nothing else can come out of `JSONSerialization`; `.null` would be a lie,
        // so this is deliberately loud in tests.
        preconditionFailure("unsupported JSON element: \(type(of: object))")
    }

    public var foundationObject: Any {
        switch self {
        case .null: NSNull()
        case let .bool(value): value
        case let .number(text):
            // An integer that fits stays an integer; anything else keeps its text
            // rather than being rounded through a Double.
            if let whole = Int64(text) { NSNumber(value: whole) }
            else if let real = Double(text), text.lowercased().contains("e") { NSNumber(value: real) }
            else { NSDecimalNumber(string: text) }
        case let .string(value): value
        case let .array(values): values.map(\.foundationObject)
        case let .object(fields): fields.mapValues(\.foundationObject)
        }
    }
}

// MARK: - Quantities

/// A decimal-string counter: `seq`, `revision`, `generation`, byte offsets.
public struct Quantity: Equatable, Hashable, Sendable, CustomStringConvertible {
    public let text: String
    public init?(_ text: String) {
        guard !text.isEmpty, text.allSatisfy({ $0 >= "0" && $0 <= "9" }) else { return nil }
        self.text = text
    }
    public init(_ value: UInt64) { text = String(value) }
    /// Mirrors `Quantity::as_u64`: unrepresentable text reads as the maximum rather
    /// than trapping, because a cursor is compared, never arithmetic'd, at the edge.
    public var uint64Value: UInt64 { UInt64(text) ?? .max }
    public var description: String { text }
}

// MARK: - Errors

/// A transport fault: the bytes never formed a frame. Distinct from `RpcError`,
/// which is a well-formed refusal from the host.
public enum FrameError: Error, Equatable, CustomStringConvertible {
    case empty
    case tooLarge(bytes: Int, limit: Int)
    case notUtf8
    case malformed(String)

    public var description: String {
        switch self {
        case .empty: "frame is empty"
        case let .tooLarge(bytes, limit): "frame of \(bytes) B exceeds the \(limit) B limit"
        case .notUtf8: "frame is not valid UTF-8"
        case let .malformed(reason): "frame is not a valid protocol frame: \(reason)"
        }
    }
}

/// The wire error code. `other` is what a decoder produces for text it has never
/// seen — the text is kept so the user can be told what the host said.
public enum WireErrorCode: Equatable, Sendable {
    case invalidArgument
    case unauthenticated
    case unsupportedCapability
    case revisionConflict
    case notFound
    case busy
    case streamGap
    case snapshotRequired
    case hostRestarted
    case interactiveUiRequired
    case ioError
    case other(String)

    /// Codes a host may emit. `.other` is never one: it is a client-side fallback.
    public static let emittable: [WireErrorCode] = [
        .invalidArgument, .unauthenticated, .unsupportedCapability, .revisionConflict,
        .notFound, .busy, .streamGap, .snapshotRequired, .hostRestarted,
        .interactiveUiRequired, .ioError,
    ]

    public var text: String {
        switch self {
        case .invalidArgument: "INVALID_ARGUMENT"
        case .unauthenticated: "UNAUTHENTICATED"
        case .unsupportedCapability: "UNSUPPORTED_CAPABILITY"
        case .revisionConflict: "REVISION_CONFLICT"
        case .notFound: "NOT_FOUND"
        case .busy: "BUSY"
        case .streamGap: "STREAM_GAP"
        case .snapshotRequired: "SNAPSHOT_REQUIRED"
        case .hostRestarted: "HOST_RESTARTED"
        case .interactiveUiRequired: "INTERACTIVE_UI_REQUIRED"
        case .ioError: "IO_ERROR"
        case let .other(unknown): unknown
        }
    }

    public init(text: String) {
        self = WireErrorCode.emittable.first { $0.text == text } ?? .other(text)
    }

    public var isRetryableByDefault: Bool {
        switch self {
        case .busy, .streamGap, .snapshotRequired: true
        default: false
        }
    }
}

public struct RpcError: Equatable, Sendable {
    public var code: WireErrorCode
    public var message: String
    public var retryable: Bool
    /// Machine-readable detail. Never carries tokens, passwords or raw session text.
    public var detail: JSONValue?
    public var additional: [String: JSONValue]

    public init(code: WireErrorCode, message: String, retryable: Bool, detail: JSONValue? = nil,
                additional: [String: JSONValue] = [:]) {
        self.code = code
        self.message = message
        self.retryable = retryable
        self.detail = detail
        self.additional = additional
    }

    static func decode(_ value: JSONValue) -> RpcError? {
        guard let fields = value.objectValue,
              let code = fields["code"]?.stringValue,
              let message = fields["message"]?.stringValue else { return nil }
        let known: Set<String> = ["code", "message", "retryable", "detail"]
        return RpcError(
            code: WireErrorCode(text: code),
            message: message,
            retryable: fields["retryable"]?.boolValue ?? false,
            detail: fields["detail"],
            additional: fields.filter { !known.contains($0.key) }
        )
    }

    public var json: JSONValue {
        var fields: [String: JSONValue] = [
            "code": .string(code.text),
            "message": .string(message),
            "retryable": .bool(retryable),
        ]
        if let detail { fields["detail"] = detail }
        for (key, value) in additional { fields[key] = value }
        return .object(fields)
    }
}

// MARK: - Envelope

public struct RequestFrame: Equatable, Sendable {
    public var id: String
    public var method: String
    public var idempotencyKey: String?
    public var params: JSONValue
    public var additional: [String: JSONValue]
}

public struct SuccessFrame: Equatable, Sendable {
    public var id: String
    public var result: JSONValue
    public var additional: [String: JSONValue]
}

public struct FailureFrame: Equatable, Sendable {
    public var id: String
    public var error: RpcError
    public var additional: [String: JSONValue]
}

public struct EventFrame: Equatable, Sendable {
    public var event: String
    public var epoch: String
    public var seq: Quantity
    public var data: JSONValue
    public var additional: [String: JSONValue]
}

public enum Frame: Equatable, Sendable {
    case event(EventFrame)
    case failure(FailureFrame)
    case success(SuccessFrame)
    case request(RequestFrame)
}

// MARK: - Codec

public enum ProtocolCodec {
    public static let protocolVersion = 1
    public static let maxFrameBytes = 1_048_576
    public static let maxTerminalChunkRawBytes = 65_536
    public static let replayMaxEvents = 4_096
    public static let replayMaxBytes = 8_388_608

    /// Order mirrors the Rust decoder: blank, byte limit, UTF-8, JSON, frame. A
    /// transport fault must not be laundered into a generic parse error.
    public static func decodeLine(_ bytes: Data) -> Result<Frame, FrameError> {
        // Size first, matching Rust and TypeScript: an oversized line is a transport
        // fault even when every byte of it is whitespace.
        if bytes.count > maxFrameBytes {
            return .failure(.tooLarge(bytes: bytes.count, limit: maxFrameBytes))
        }
        if bytes.allSatisfy({ $0 == 0x20 || $0 == 0x09 || $0 == 0x0a || $0 == 0x0d }) {
            return .failure(.empty)
        }
        guard let text = String(data: bytes, encoding: .utf8) else { return .failure(.notUtf8) }
        return parse(text)
    }

    public static func parse(_ text: String) -> Result<Frame, FrameError> {
        guard let data = text.data(using: .utf8) else { return .failure(.notUtf8) }
        let object: Any
        do {
            object = try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        } catch {
            return .failure(.malformed(error.localizedDescription))
        }
        return frame(JSONValue.from(object))
    }

    static func frame(_ value: JSONValue) -> Result<Frame, FrameError> {
        guard let fields = value.objectValue else {
            return .failure(.malformed("a frame must be a JSON object"))
        }
        guard fields["v"]?.numberText == String(protocolVersion) else {
            return .failure(.malformed("frame is missing v=\(protocolVersion)"))
        }
        // `additional` is every key this build does not model: a newer host's
        // addition must be re-emitted, not silently dropped.
        var kept = fields
        for key in ["v", "id", "event", "seq", "epoch", "data", "method", "params",
                    "idempotencyKey", "ok", "result", "error"] {
            kept[key] = nil
        }
        let id = fields["id"]?.stringValue

        if let event = fields["event"]?.stringValue {
            guard let seqText = fields["seq"]?.stringValue, let seq = Quantity(seqText) else {
                return .failure(.malformed("event seq must be a decimal string"))
            }
            guard let epoch = fields["epoch"]?.stringValue else {
                return .failure(.malformed("event epoch must be a string"))
            }
            return .success(.event(EventFrame(
                event: event, epoch: epoch, seq: seq,
                data: fields["data"] ?? .object([:]), additional: kept
            )))
        }
        if let method = fields["method"]?.stringValue {
            if case .some(let key) = fields["idempotencyKey"], key.stringValue == nil {
                return .failure(.malformed("idempotencyKey must be a string when present"))
            }
            guard let id else { return .failure(.malformed("request id must be a string")) }
            return .success(.request(RequestFrame(
                id: id, method: method,
                idempotencyKey: fields["idempotencyKey"]?.stringValue,
                params: fields["params"] ?? .object([:]), additional: kept
            )))
        }
        guard let id else { return .failure(.malformed("response id must be a string")) }
        guard let ok = fields["ok"]?.boolValue else {
            return .failure(.malformed("a response frame needs a boolean ok"))
        }
        if ok {
            return .success(.success(SuccessFrame(
                id: id, result: fields["result"] ?? .object([:]), additional: kept
            )))
        }
        guard let error = RpcError.decode(fields["error"] ?? .null) else {
            return .failure(.malformed("failure frame needs an error object"))
        }
        return .success(.failure(FailureFrame(id: id, error: error, additional: kept)))
    }

    /// One JSONL line, terminated by `\\n`, refusing to emit past the frame limit.
    /// Key order is not stable in `JSONSerialization`, so the contract test compares
    /// decoded structures across a round trip rather than bytes.
    public static func encodeLine(_ frame: Frame) throws -> String {
        var fields = payload(of: frame)
        fields["v"] = .number(String(protocolVersion))
        let data = try JSONSerialization.data(withJSONObject: fields.mapValues(\.foundationObject))
        guard let body = String(data: data, encoding: .utf8) else {
            throw FrameError.malformed("frame could not be encoded")
        }
        let line = body + "\n"
        guard line.utf8.count <= maxFrameBytes else {
            throw FrameError.tooLarge(bytes: line.utf8.count, limit: maxFrameBytes)
        }
        return line
    }

    static func payload(of frame: Frame) -> [String: JSONValue] {
        switch frame {
        case let .event(value):
            var fields: [String: JSONValue] = [
                "event": .string(value.event), "epoch": .string(value.epoch),
                "seq": .string(value.seq.text), "data": value.data,
            ]
            for (key, item) in value.additional { fields[key] = item }
            return fields
        case let .request(value):
            var fields: [String: JSONValue] = [
                "id": .string(value.id), "method": .string(value.method),
                "params": value.params,
            ]
            if let key = value.idempotencyKey { fields["idempotencyKey"] = .string(key) }
            for (name, item) in value.additional { fields[name] = item }
            return fields
        case let .success(value):
            var fields: [String: JSONValue] = ["id": .string(value.id), "ok": .bool(true), "result": value.result]
            for (key, item) in value.additional { fields[key] = item }
            return fields
        case let .failure(value):
            var fields: [String: JSONValue] = ["id": .string(value.id), "ok": .bool(false), "error": value.error.json]
            for (key, item) in value.additional { fields[key] = item }
            return fields
        }
    }

}
