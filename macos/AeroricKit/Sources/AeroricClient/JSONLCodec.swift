import Foundation
import AeroricProtocol

/// A transport-level fault: the bytes never became a well-formed frame.
/// Mirrors `core/crates/aeroric-host/src/codec.rs::CodecError` — a fault means
/// the connection must resync or close, never "skip a line and carry on":
/// frame boundaries after a lost byte are unknowable, so carrying on would
/// decode a whole stream of plausible-looking garbage.
public enum TransportFault: Error, Equatable, Sendable {
    /// The stream closed while a frame was still buffered.
    case prematureEOF(bufferedBytes: Int)
    /// The cap was hit before a newline arrived (host codec parity: rejected
    /// pre-delimiter so a peer cannot grow our memory past one cap).
    case overlong(bytes: Int, limit: Int)
    /// A complete line failed protocol decode (blank, not UTF-8, not JSON, …).
    case line(FrameError)
}

/// Line-level output of [`JSONLFrameSplitter`]: decoded-not lines, plus the
/// fault that (if present) terminates the stream — frames after a fault carry
/// no meaning, so the batch carries at most one fault and never both a fault
/// and further lines.
public struct JSONLSplitBatch: Equatable, Sendable {
    public var lines: [String]
    public var fault: TransportFault?

    public init(lines: [String], fault: TransportFault? = nil) {
        self.lines = lines
        self.fault = fault
    }
}

/// Splits a UTF-8 JSONL byte stream into complete lines, buffering the tail.
///
/// Cap parity with the host codec: a line that grows past
/// `ProtocolCodec.maxFrameBytes` without a delimiter faults immediately, so
/// memory stays bounded by one cap regardless of what the peer sends.
/// Feeding an empty array is legal and returns an empty batch.
public struct JSONLFrameSplitter: Sendable {
    private var buffer = Data()

    public init() {}

    public var pendingByteCount: Int { buffer.count }

    public mutating func feed(_ bytes: [UInt8]) -> JSONLSplitBatch {
        guard !bytes.isEmpty else { return JSONLSplitBatch(lines: []) }
        buffer.append(contentsOf: bytes)

        guard buffer.firstIndex(of: UInt8(ascii: "\n")) != nil else {
            if buffer.count > ProtocolCodec.maxFrameBytes {
                let fault = TransportFault.overlong(bytes: buffer.count, limit: ProtocolCodec.maxFrameBytes)
                buffer.removeAll()
                return JSONLSplitBatch(lines: [], fault: fault)
            }
            return JSONLSplitBatch(lines: [])
        }

        var lines: [String] = []
        var byteOffset = buffer.startIndex
        while let delimiter = buffer[byteOffset...].firstIndex(of: UInt8(ascii: "\n")) {
            let lineBytes = buffer[byteOffset..<delimiter]
            byteOffset = buffer.index(after: delimiter)
            // A tail that already exceeds the cap without another delimiter
            // cannot become a valid frame; stop before decoding garbage.
            if buffer[byteOffset...].firstIndex(of: UInt8(ascii: "\n")) == nil,
               buffer.distance(from: byteOffset, to: buffer.endIndex) > ProtocolCodec.maxFrameBytes {
                let fault = TransportFault.overlong(bytes: buffer.distance(from: byteOffset, to: buffer.endIndex),
                                                    limit: ProtocolCodec.maxFrameBytes)
                buffer.removeAll()
                return JSONLSplitBatch(lines: lines, fault: fault)
            }
            lines.append(String(decoding: lineBytes, as: UTF8.self))
        }
        buffer.removeSubrange(buffer.startIndex..<byteOffset)
        return JSONLSplitBatch(lines: lines)
    }

    /// Called once the peer half-closes: whatever is still buffered never
    /// became a frame, and the connection must fault rather than guess.
    public mutating func finish() -> JSONLSplitBatch {
        guard !buffer.isEmpty else { return JSONLSplitBatch(lines: []) }
        let fault = TransportFault.prematureEOF(bufferedBytes: buffer.count)
        buffer.removeAll()
        return JSONLSplitBatch(lines: [], fault: fault)
    }
}
