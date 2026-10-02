import Foundation
import AeroricProtocol

/// 终端重放面/租约面的类型化客户端（P5-01，ADR-038）。三个方法都是
/// dispatcher 注册的宿主方法：`terminal.plan_replay` 承接回放与活流
/// （字节环即缓冲，轮询节奏由调用方自持），acquire/release 管租约。
/// 应答解析是纯函数（`TerminalReplayTail.parse`），连接错误一律上抛。
public struct TerminalStreamClient: Sendable {
    public static let capabilityName = "terminal.replay"

    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    // MARK: - plan_replay

    public struct Cursor: Sendable, Equatable {
        public enum Kind: String, Sendable { case bytes, unitsFromStart }
        public var kind: Kind
        public var value: UInt64

        public init(bytes value: UInt64) {
            self.kind = .bytes
            self.value = value
        }

        public init(unitsFromStart value: UInt64) {
            self.kind = .unitsFromStart
            self.value = value
        }

        var json: JSONValue {
            .object([
                "kind": .string(kind.rawValue),
                "value": .number(String(value)),
            ])
        }
    }

    /// 一次 plan_replay 应答的解码结果。`reason` 只在 reset 时非 nil。
    public struct ReplayTail: Sendable, Equatable {
        public enum Kind: String, Sendable { case seamless, reset }
        public var kind: Kind
        public var from: UInt64
        public var bytes: [UInt8]
        public var resumeAfter: UInt64
        /// reset 时的具名理由（ADR-038）：notStarted/evicted/compacted/
        /// cursorBeyondStream/unitsNotAnchorable/unknownStream。
        public var reasonKind: String?
        /// compacted 携带最早可用 offset；cursorBeyondStream 携带流末端。
        public var reasonDetail: UInt64?

        /// 与 ADR-038 的核心承诺对齐：无论 seamless 还是 reset，下一轮
        /// 记的都是「已交付字节之后」的字节游标。
        public var nextCursor: Cursor {
            Cursor(bytes: resumeAfter)
        }
    }

    public enum ReplayError: Error, Equatable {
        case malformedReply(String)
    }

    /// 应答 JSON → ReplayTail（纯函数，单测覆盖）。
    public static func parseReplayTail(_ result: JSONValue) throws -> ReplayTail {
        let kind = result["kind"]?.stringValue
        let from = result["from"]?.numberText.flatMap(UInt64.init)
        let resumeAfter = result["resumeAfter"]?.numberText.flatMap(UInt64.init)
        let b64 = result["bytes"]?.stringValue
        guard let kind, let from, let resumeAfter, let b64 else {
            throw ReplayError.malformedReply("missing kind/from/bytes/resumeAfter")
        }
        guard let decoded = Data(base64Encoded: b64) else {
            throw ReplayError.malformedReply("bytes is not valid base64")
        }
        let reason = result["reason"]
        let reasonKind = reason?["kind"]?.stringValue
        // compacted 携带 earliest、cursorBeyondStream 携带 nextOffset——都在
        // reason 对象里（宿主 terminal_plan_replay 的应答形状）。
        var reasonDetail: UInt64?
        if let earliest = reason?["earliest"]?.numberText.flatMap(UInt64.init) {
            reasonDetail = earliest
        }
        if let nextOffset = reason?["nextOffset"]?.numberText.flatMap(UInt64.init) {
            reasonDetail = nextOffset
        }
        return ReplayTail(
            kind: .init(rawValue: kind) ?? .reset,
            from: from,
            bytes: [UInt8](decoded),
            resumeAfter: resumeAfter,
            reasonKind: reasonKind,
            reasonDetail: reasonDetail
        )
    }

    @discardableResult
    public func planReplay(
        taskId: String,
        generation: UInt64,
        cursor: Cursor,
        maxBytes: UInt64 = UInt64(aeroric_protocol_terminal_max_chunk)
    ) async throws -> ReplayTail {
        let frame = try await connection.request(
            "terminal.plan_replay",
            params: .object([
                "taskId": .string(taskId),
                "generation": .number(String(generation)),
                "cursor": cursor.json,
                "maxBytes": .number(String(maxBytes)),
            ])
        )
        return try Self.parseReplayTail(frame.result)
    }

    // MARK: - 租约

    public struct LeaseTicket: Sendable, Equatable {
        public var value: UInt64
    }

    @discardableResult
    public func acquireControl(taskId: String, generation: UInt64) async throws -> LeaseTicket {
        let frame = try await connection.request(
            "terminal.acquire_control",
            params: .object([
                "taskId": .string(taskId),
                "generation": .number(String(generation)),
            ])
        )
        guard let ticket = frame.result["ticket"]?.numberText.flatMap(UInt64.init) else {
            throw ReplayError.malformedReply("missing ticket")
        }
        return LeaseTicket(value: ticket)
    }

    /// 过期票据按宿主语义返回 `released == false`（静默 no-op，不报错）。
    public func releaseControl(taskId: String, generation: UInt64, ticket: LeaseTicket) async throws -> Bool {
        let frame = try await connection.request(
            "terminal.release_control",
            params: .object([
                "taskId": .string(taskId),
                "generation": .number(String(generation)),
                "ticket": .number(String(ticket.value)),
            ])
        )
        return frame.result["released"]?.boolValue ?? false
    }
}

/// 与 AeroricProtocol 的 MAX_TERMINAL_CHUNK_BYTES 对齐（协议 crate 的常量
/// 不经 Swift 导出，这里以数字钉住并由 contracts 逐字节门禁守护）。
public let aeroric_protocol_terminal_max_chunk: UInt64 = 64 * 1024
