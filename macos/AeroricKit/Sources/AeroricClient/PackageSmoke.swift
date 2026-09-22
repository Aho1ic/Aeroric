import Foundation
import AeroricProtocol

/// Splits a UTF-8 JSONL byte stream into complete frames, buffering the tail.
/// A partial frame must never be surfaced as a frame, and one read may carry
/// several frames. PTY payloads ride the same path, so this cannot assume the
/// buffer boundary lands on a character edge.
public struct JSONLFrameSplitter: Sendable {
    private var buffer = Data()
    public init() {}

    public mutating func feed(_ bytes: [UInt8]) -> [String] {
        buffer.append(contentsOf: bytes)
        var frames: [String] = []
        while let newline = buffer.firstIndex(of: UInt8(ascii: "\n")) {
            let slice = buffer[buffer.startIndex..<newline]
            if let text = String(data: Data(slice), encoding: .utf8), !text.isEmpty {
                frames.append(text)
            }
            buffer = Data(buffer[buffer.index(after: newline)...])
        }
        return frames
    }

    public var pendingByteCount: Int { buffer.count }
}

/// Proves the client layer can consume a protocol-layer type without the
/// reverse dependency existing (AeroricProtocol does not import AeroricClient).
public func decodeFrame(_ text: String) -> RPCEnvelope? { RPCEnvelope.decode(text) }

public enum ClientMarker: Sendable {
    public static var protocolVersion: Int { ProtocolMarker.version }
}
