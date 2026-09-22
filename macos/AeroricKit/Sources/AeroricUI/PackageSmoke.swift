import Foundation
import AeroricClient
import AeroricProtocol

/// Pure view-model helper: turns a decoded frame into a displayable line.
/// Deliberately free of any host I/O so the UI layer stays testable off-main.
public struct ConnectionLine: Equatable, Sendable {
    public var text: String
    public init(text: String) { self.text = text }
}

public func present(_ frame: String) -> ConnectionLine? {
    guard let envelope = decodeFrame(frame) else { return nil }
    if let method = envelope.method { return ConnectionLine(text: "\(method) #\(envelope.id ?? "-")") }
    if let ok = envelope.ok { return ConnectionLine(text: "response ok=\(ok)") }
    return ConnectionLine(text: "v\(envelope.version)")
}

public enum UIMarker: Sendable {
    public static var seenProtocolVersion: Int { ClientMarker.protocolVersion }
}
