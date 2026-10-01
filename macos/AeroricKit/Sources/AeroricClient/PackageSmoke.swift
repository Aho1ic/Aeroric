import Foundation
import AeroricProtocol

/// Proves the client layer can consume a protocol-layer type without the
/// reverse dependency existing (AeroricProtocol does not import AeroricClient).
public func decodeFrame(_ text: String) -> RPCEnvelope? { RPCEnvelope.decode(text) }

public enum ClientMarker: Sendable {
    public static var protocolVersion: Int { ProtocolMarker.version }
}
