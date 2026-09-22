import Foundation

/// Minimal local-protocol v1 envelope used to prove the codec contract exists.
/// The full DTO set is frozen in P2-01; unknown members must survive a
/// decode/encode round trip so newer hosts stay readable by older clients.
public struct RPCEnvelope: Equatable, Sendable {
    public var version: Int
    public var id: String?
    public var method: String?
    public var ok: Bool?
    public var rawResult: String?
    /// JSON members this build does not model, preserved as their exact JSON text.
    public var unknownMembers: [String: String]

    public init(version: Int, id: String?, method: String?, ok: Bool? = nil,
                rawResult: String? = nil, unknownMembers: [String: String] = [:]) {
        self.version = version
        self.id = id
        self.method = method
        self.ok = ok
        self.rawResult = rawResult
        self.unknownMembers = unknownMembers
    }

    /// Text form of one JSON element. Wrapping in an array first is what makes
    /// scalars work: `data(withJSONObject:)` raises an Objective-C exception for
    /// a top-level fragment, which `try?` cannot catch.
    static func jsonElementText(_ value: Any) -> String? {
        guard let blob = try? JSONSerialization.data(withJSONObject: [value]),
              let text = String(data: blob, encoding: .utf8),
              text.hasPrefix("[") && text.hasSuffix("]") else { return nil }
        return String(text.dropFirst().dropLast())
    }

    static func jsonElement(_ text: String) -> Any? {
        guard let blob = ("[" + text + "]").data(using: .utf8),
              let array = try? JSONSerialization.jsonObject(with: blob) as? [Any],
              let first = array.first else { return nil }
        return first
    }

    public static func decode(_ text: String) -> RPCEnvelope? {
        guard let data = text.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let version = object["v"] as? Int else { return nil }
        var unknown: [String: String] = [:]
        let known: Set<String> = ["v", "id", "method", "ok", "result"]
        for (key, value) in object where !known.contains(key) {
            if let element = jsonElementText(value) { unknown[key] = element }
        }
        var result: String?
        if let r = object["result"] { result = jsonElementText(r) }
        return RPCEnvelope(version: version, id: object["id"] as? String,
                           method: object["method"] as? String, ok: object["ok"] as? Bool,
                           rawResult: result, unknownMembers: unknown)
    }

    public func encode() -> String? {
        var object: [String: Any] = ["v": version]
        if let id { object["id"] = id }
        if let method { object["method"] = method }
        if let ok { object["ok"] = ok }
        if let rawResult, let element = RPCEnvelope.jsonElement(rawResult) { object["result"] = element }
        for (key, text) in unknownMembers {
            if let element = RPCEnvelope.jsonElement(text) { object[key] = element }
        }
        guard let blob = try? JSONSerialization.data(withJSONObject: object),
              let text = String(data: blob, encoding: .utf8) else { return nil }
        return text
    }
}

public enum ProtocolMarker: Sendable {
    public static var version: Int { 1 }
}
