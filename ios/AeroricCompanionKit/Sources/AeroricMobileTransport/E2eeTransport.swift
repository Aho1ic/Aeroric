import CryptoKit
import Foundation

/// AP5: Native Swift CryptoKit implementation of Aeroric E2EE protocol v2.
/// Mirrors `core/crates/aeroric-core/src/remote/crypto.rs` and `mobile/src/transport/e2ee.ts` byte-for-byte:
/// - Handshake: X25519 ephemeral key agreement + static server key pinning, HKDF-SHA256 key derivation
///   (`salt = "aeroric-remote-e2ee-v2"`, `info = c_eph || s_eph || s_static`), and confirm tag
///   (`"aeroric-e2ee-confirm-v2"` with AAD `"confirm"` at seq 0 on the server-to-client cipher).
/// - Framing: `[kind: u8][seq: u64 BE][ChaCha20-Poly1305 ciphertext + 16B tag]`, AAD = `[kind]`,
///   nonce = 4 zero bytes + 8-byte BE `seq`. Strictly monotonic sequence numbers reject replay/out-of-order frames.
public enum E2eeError: Error, Equatable, Sendable {
    case invalidServerPublicKey
    case invalidHandshakeAck
    case serverRejected(String)
    case hostIdentityVerificationFailed
    case shortFrame
    case outOfOrderFrame(expected: UInt64, received: UInt64)
    case authenticationFailed
}

public enum E2eeFrameKind: UInt8, Sendable {
    case control = 1
    case terminal = 2
}

public enum Base64UrlCodec {
    public static func encode(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    public static func decode(_ text: String) -> Data? {
        var normalized = text
            .replacingOccurrences(of: "-", with: "+")
            .replacingOccurrences(of: "_", with: "/")
        let remainder = normalized.count % 4
        if remainder > 0 {
            normalized.append(String(repeating: "=", count: 4 - remainder))
        }
        return Data(base64Encoded: normalized)
    }
}

public final class DirectionCipher: @unchecked Sendable {
    private let lock = NSLock()
    private let key: SymmetricKey
    private var seq: UInt64 = 0

    public init(keyData: Data) {
        self.key = SymmetricKey(data: keyData)
    }

    private static func makeNonce(seq: UInt64) throws -> ChaChaPoly.Nonce {
        var bytes = [UInt8](repeating: 0, count: 12)
        var be = seq.bigEndian
        withUnsafeBytes(of: &be) { raw in
            for i in 0..<8 {
                bytes[4 + i] = raw[i]
            }
        }
        return try ChaChaPoly.Nonce(data: bytes)
    }

    public func seal(aad: Data, plaintext: Data) throws -> (seq: UInt64, ciphertextAndTag: Data) {
        lock.lock()
        let currentSeq = seq
        seq += 1
        lock.unlock()

        let nonce = try Self.makeNonce(seq: currentSeq)
        let sealed = try ChaChaPoly.seal(plaintext, using: key, nonce: nonce, authenticating: aad)
        var out = Data()
        out.append(sealed.ciphertext)
        out.append(sealed.tag)
        return (currentSeq, out)
    }

    public func open(expectedSeq: UInt64, aad: Data, ciphertextAndTag: Data) throws -> Data {
        lock.lock()
        let currentSeq = seq
        guard expectedSeq == currentSeq else {
            lock.unlock()
            throw E2eeError.outOfOrderFrame(expected: currentSeq, received: expectedSeq)
        }
        guard ciphertextAndTag.count >= 16 else {
            lock.unlock()
            throw E2eeError.shortFrame
        }
        let ctEnd = ciphertextAndTag.count - 16
        let ct = ciphertextAndTag.prefix(ctEnd)
        let tag = ciphertextAndTag.suffix(16)
        do {
            let nonce = try Self.makeNonce(seq: expectedSeq)
            let box = try ChaChaPoly.SealedBox(nonce: nonce, ciphertext: ct, tag: tag)
            let plain = try ChaChaPoly.open(box, using: key, authenticating: aad)
            seq = expectedSeq + 1
            lock.unlock()
            return plain
        } catch {
            lock.unlock()
            throw E2eeError.authenticationFailed
        }
    }
}

public final class E2eeSession: @unchecked Sendable {
    public static let protocolVersion = 2
    public static let frameHeaderBytes = 9
    fileprivate static let hkdfSalt = Data("aeroric-remote-e2ee-v2".utf8)
    fileprivate static let confirmPlaintext = Data("aeroric-e2ee-confirm-v2".utf8)
    fileprivate static let confirmAAD = Data("confirm".utf8)

    private let sendCipher: DirectionCipher
    private let recvCipher: DirectionCipher

    public init(sendCipher: DirectionCipher, recvCipher: DirectionCipher) {
        self.sendCipher = sendCipher
        self.recvCipher = recvCipher
    }

    public func encryptFrame(kind: E2eeFrameKind, plaintext: Data) throws -> Data {
        let aad = Data([kind.rawValue])
        let (seq, ct) = try sendCipher.seal(aad: aad, plaintext: plaintext)
        var out = Data(capacity: Self.frameHeaderBytes + ct.count)
        out.append(kind.rawValue)
        var be = seq.bigEndian
        withUnsafeBytes(of: &be) { out.append(contentsOf: $0) }
        out.append(ct)
        return out
    }

    public func decryptFrame(_ frame: Data) throws -> (kind: E2eeFrameKind, plaintext: Data) {
        guard frame.count >= Self.frameHeaderBytes + 16 else {
            throw E2eeError.shortFrame
        }
        guard let kind = E2eeFrameKind(rawValue: frame[frame.startIndex]) else {
            throw E2eeError.authenticationFailed
        }
        var seqBE: UInt64 = 0
        _ = withUnsafeMutableBytes(of: &seqBE) { dst in
            frame.subdata(in: (frame.startIndex + 1)..<(frame.startIndex + 9)).copyBytes(to: dst)
        }
        let seq = UInt64(bigEndian: seqBE)
        let ct = frame.subdata(in: (frame.startIndex + 9)..<frame.endIndex)
        let plain = try recvCipher.open(expectedSeq: seq, aad: Data([kind.rawValue]), ciphertextAndTag: ct)
        return (kind, plain)
    }

    fileprivate static func deriveKeys(
        dhStatic: SharedSecret,
        dhEphemeral: SharedSecret,
        clientEph: Data,
        serverEph: Data,
        serverStatic: Data
    ) -> (clientToServer: Data, serverToClient: Data) {
        var ikm = Data(capacity: 64)
        dhStatic.withUnsafeBytes { ikm.append(contentsOf: $0) }
        dhEphemeral.withUnsafeBytes { ikm.append(contentsOf: $0) }

        var info = Data(capacity: 96)
        info.append(clientEph)
        info.append(serverEph)
        info.append(serverStatic)

        let derived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: SymmetricKey(data: ikm),
            salt: hkdfSalt,
            info: info,
            outputByteCount: 64
        )
        var raw = Data(capacity: 64)
        derived.withUnsafeBytes { raw.append(contentsOf: $0) }
        return (raw.prefix(32), raw.suffix(32))
    }
}

public struct PendingClientHandshake: Sendable {
    public let helloJSON: String
    private let clientEphemeralPrivate: Curve25519.KeyAgreement.PrivateKey
    private let serverStaticPublic: Curve25519.KeyAgreement.PublicKey

    public static func start(serverStaticPubBase64Url: String) throws -> PendingClientHandshake {
        guard let rawStatic = Base64UrlCodec.decode(serverStaticPubBase64Url),
              rawStatic.count == 32,
              let serverStatic = try? Curve25519.KeyAgreement.PublicKey(rawRepresentation: rawStatic) else {
            throw E2eeError.invalidServerPublicKey
        }
        let ephPrivate = Curve25519.KeyAgreement.PrivateKey()
        let pubB64 = Base64UrlCodec.encode(ephPrivate.publicKey.rawRepresentation)
        let helloObj: [String: Any] = [
            "v": E2eeSession.protocolVersion,
            "type": "hello",
            "pub": pubB64,
        ]
        let helloData = try JSONSerialization.data(withJSONObject: helloObj)
        let helloJSON = String(decoding: helloData, as: UTF8.self)
        return PendingClientHandshake(
            helloJSON: helloJSON,
            clientEphemeralPrivate: ephPrivate,
            serverStaticPublic: serverStatic
        )
    }

    public func finish(ackJSON: String) throws -> E2eeSession {
        guard let data = ackJSON.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw E2eeError.invalidHandshakeAck
        }
        if (obj["type"] as? String) == "hello_error" {
            let reason = (obj["error"] as? String) ?? "host rejected connection"
            throw E2eeError.serverRejected(reason)
        }
        guard (obj["type"] as? String) == "hello_ack",
              let pubStr = obj["pub"] as? String,
              let confirmStr = obj["confirm"] as? String,
              let serverEphRaw = Base64UrlCodec.decode(pubStr),
              serverEphRaw.count == 32,
              let serverEphPub = try? Curve25519.KeyAgreement.PublicKey(rawRepresentation: serverEphRaw),
              let confirmCipher = Base64UrlCodec.decode(confirmStr) else {
            throw E2eeError.invalidHandshakeAck
        }
        let dhStatic = try clientEphemeralPrivate.sharedSecretFromKeyAgreement(with: serverStaticPublic)
        let dhEphemeral = try clientEphemeralPrivate.sharedSecretFromKeyAgreement(with: serverEphPub)
        let keys = E2eeSession.deriveKeys(
            dhStatic: dhStatic,
            dhEphemeral: dhEphemeral,
            clientEph: clientEphemeralPrivate.publicKey.rawRepresentation,
            serverEph: serverEphRaw,
            serverStatic: serverStaticPublic.rawRepresentation
        )
        let send = DirectionCipher(keyData: keys.clientToServer)
        let recv = DirectionCipher(keyData: keys.serverToClient)
        guard let confirmPlain = try? recv.open(
            expectedSeq: 0,
            aad: E2eeSession.confirmAAD,
            ciphertextAndTag: confirmCipher
        ), confirmPlain == E2eeSession.confirmPlaintext else {
            throw E2eeError.hostIdentityVerificationFailed
        }
        return E2eeSession(sendCipher: send, recvCipher: recv)
    }
}

/// Test/responder helper mirroring the Mac host's static key + handshake response.
public struct ServerHandshakeResponder: Sendable {
    public let staticPrivate: Curve25519.KeyAgreement.PrivateKey
    public var publicBase64Url: String {
        Base64UrlCodec.encode(staticPrivate.publicKey.rawRepresentation)
    }

    public init(staticPrivate: Curve25519.KeyAgreement.PrivateKey = Curve25519.KeyAgreement.PrivateKey()) {
        self.staticPrivate = staticPrivate
    }

    public func respond(helloJSON: String) throws -> (ackJSON: String, session: E2eeSession) {
        guard let data = helloJSON.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              (obj["v"] as? Int) == E2eeSession.protocolVersion,
              (obj["type"] as? String) == "hello",
              let pubStr = obj["pub"] as? String,
              let clientEphRaw = Base64UrlCodec.decode(pubStr),
              clientEphRaw.count == 32,
              let clientEphPub = try? Curve25519.KeyAgreement.PublicKey(rawRepresentation: clientEphRaw) else {
            throw E2eeError.invalidHandshakeAck
        }
        let serverEphPrivate = Curve25519.KeyAgreement.PrivateKey()
        let serverEphRaw = serverEphPrivate.publicKey.rawRepresentation
        let dhStatic = try staticPrivate.sharedSecretFromKeyAgreement(with: clientEphPub)
        let dhEphemeral = try serverEphPrivate.sharedSecretFromKeyAgreement(with: clientEphPub)
        let keys = E2eeSession.deriveKeys(
            dhStatic: dhStatic,
            dhEphemeral: dhEphemeral,
            clientEph: clientEphRaw,
            serverEph: serverEphRaw,
            serverStatic: staticPrivate.publicKey.rawRepresentation
        )
        let send = DirectionCipher(keyData: keys.serverToClient)
        let recv = DirectionCipher(keyData: keys.clientToServer)
        let (_, confirmCT) = try send.seal(aad: E2eeSession.confirmAAD, plaintext: E2eeSession.confirmPlaintext)
        let ackObj: [String: Any] = [
            "v": E2eeSession.protocolVersion,
            "type": "hello_ack",
            "pub": Base64UrlCodec.encode(serverEphRaw),
            "confirm": Base64UrlCodec.encode(confirmCT),
        ]
        let ackData = try JSONSerialization.data(withJSONObject: ackObj)
        return (String(decoding: ackData, as: UTF8.self), E2eeSession(sendCipher: send, recvCipher: recv))
    }
}
