import Foundation

/// AP5: Native Swift RPC v2/v3 envelope and 16-byte binary `TerminalFrame` codec.
/// Mirrors `packages/remote-contracts` and `mobile/src/terminal/terminal-frames.ts`.
public enum RemoteRpcVersion: Int, Codable, Equatable, Sendable {
    case v2 = 2
    case v3 = 3
}

public enum TerminalOpcode: UInt8, Equatable, Sendable, CaseIterable {
    case output = 1
    case snapshotStart = 2
    case snapshotChunk = 3
    case snapshotEnd = 4
    case resized = 5
    case error = 6
    case input = 7
    case resize = 8
    case subscribe = 9
    case unsubscribe = 10
}

public struct TerminalFrame: Equatable, Sendable {
    public static let kindByte: UInt8 = 0x74
    public static let versionByte: UInt8 = 1
    public static let headerBytes = 16
    public static let maxPayloadBytes = 64 * 1024

    public var opcode: TerminalOpcode
    public var streamId: UInt32
    public var seq: UInt64
    public var payload: Data

    public init(opcode: TerminalOpcode, streamId: UInt32, seq: UInt64, payload: Data) {
        self.opcode = opcode
        self.streamId = streamId
        self.seq = seq
        self.payload = payload
    }

    public func encode() -> Data {
        var out = Data(capacity: Self.headerBytes + payload.count)
        out.append(Self.kindByte)
        out.append(Self.versionByte)
        out.append(opcode.rawValue)
        out.append(0)
        var sidLE = streamId.littleEndian
        withUnsafeBytes(of: &sidLE) { out.append(contentsOf: $0) }
        var seqLE = seq.littleEndian
        withUnsafeBytes(of: &seqLE) { out.append(contentsOf: $0) }
        out.append(payload)
        return out
    }

    public static func decode(_ bytes: Data) -> TerminalFrame? {
        guard bytes.count >= headerBytes else { return nil }
        let start = bytes.startIndex
        guard bytes[start] == kindByte,
              bytes[start + 1] == versionByte,
              let opcode = TerminalOpcode(rawValue: bytes[start + 2]) else {
            return nil
        }
        var sidLE: UInt32 = 0
        _ = withUnsafeMutableBytes(of: &sidLE) { dst in
            bytes.subdata(in: (start + 4)..<(start + 8)).copyBytes(to: dst)
        }
        var seqLE: UInt64 = 0
        _ = withUnsafeMutableBytes(of: &seqLE) { dst in
            bytes.subdata(in: (start + 8)..<(start + 16)).copyBytes(to: dst)
        }
        let payload = bytes.subdata(in: (start + 16)..<bytes.endIndex)
        guard payload.count <= maxPayloadBytes else { return nil }
        return TerminalFrame(
            opcode: opcode,
            streamId: UInt32(littleEndian: sidLE),
            seq: UInt64(littleEndian: seqLE),
            payload: payload
        )
    }
}

public enum DecodedRemoteEnvelope: Equatable, Sendable {
    case responseSuccess(id: String, resultJSON: String)
    case responseFailure(id: String, code: String, message: String)
    case push(event: String, seq: UInt64?, payloadJSON: String)
}

public enum RemoteRpcCodec {
    public static func encodeRequest(
        version: RemoteRpcVersion,
        id: String,
        method: String,
        params: [String: Any] = [:]
    ) throws -> String {
        let obj: [String: Any]
        switch version {
        case .v3:
            obj = [
                "v": 3,
                "type": "request",
                "id": id,
                "method": method,
                "params": params,
            ]
        case .v2:
            obj = [
                "v": 2,
                "id": Int(id) ?? 0,
                "method": method,
                "params": params,
            ]
        }
        let data = try JSONSerialization.data(withJSONObject: obj)
        return String(decoding: data, as: UTF8.self)
    }

    public static func decodeEnvelope(_ jsonText: String) -> DecodedRemoteEnvelope? {
        guard let data = jsonText.data(using: .utf8),
              let frame = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }
        if let event = frame["event"] as? String {
            let seq = (frame["seq"] as? UInt64) ?? (frame["seq"] as? Int).map(UInt64.init)
            let payloadObj = frame["data"] ?? [:]
            let payloadData = (try? JSONSerialization.data(withJSONObject: payloadObj, options: [.fragmentsAllowed])) ?? Data("{}".utf8)
            return .push(
                event: event,
                seq: seq,
                payloadJSON: String(decoding: payloadData, as: UTF8.self)
            )
        }
        guard let ok = frame["ok"] as? Bool else { return nil }
        let idStr: String
        if let s = frame["id"] as? String {
            idStr = s
        } else if let n = frame["id"] as? Int {
            idStr = String(n)
        } else {
            return nil
        }
        if ok {
            let resObj = frame["result"] ?? [:]
            let resData = (try? JSONSerialization.data(withJSONObject: resObj, options: [.fragmentsAllowed])) ?? Data("{}".utf8)
            return .responseSuccess(id: idStr, resultJSON: String(decoding: resData, as: UTF8.self))
        } else {
            if let errDict = frame["error"] as? [String: Any] {
                let code = (errDict["code"] as? String) ?? "remote_error"
                let message = (errDict["message"] as? String) ?? code
                return .responseFailure(id: idStr, code: code, message: message)
            }
            let message = (frame["error"] as? String) ?? "request failed"
            return .responseFailure(id: idStr, code: "remote_error", message: message)
        }
    }
}
