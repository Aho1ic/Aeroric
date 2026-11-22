import Foundation

/// C4 (2026-11-19): the gateway's requests, try by try (`gateway.requests`).
/// Mirrors `aeroric_core::local_router::{RouterUsageSummary, RouterRequestRecord, RouterAttempt}`.
public struct LiveGatewayUsage: Equatable, Sendable {
    public var summary: Summary
    public var requests: [Request]
    /// E7 (2026-11-22): the last 30 days by purpose, the most requested first.
    public var purposes: [Purpose] = []

    public struct Purpose: Equatable, Sendable, Identifiable {
        public var id: String { purpose }
        /// `answer` `review` `delegation` `discussion` `rerun` `schedule`
        /// `unattributed`, or `""` for rows from before E7.
        public var purpose: String
        public var requests: UInt64 = 0
        public var failedRequests: UInt64 = 0
        public var inputTokens: UInt64 = 0
        public var outputTokens: UInt64 = 0
        public var unknownUsageRequests: UInt64 = 0
        public var failedAttempts: UInt64 = 0
        public var failedAttemptInputTokens: UInt64 = 0
        public var failedAttemptOutputTokens: UInt64 = 0
        /// E7 step 2: Aeroric called the analysis model directly (memory,
        /// Wiki), not through the gateway.
        public var direct = false

        public init(purpose: String) { self.purpose = purpose }
    }

    public init(summary: Summary = Summary(), requests: [Request] = []) {
        self.summary = summary
        self.requests = requests
    }

    public struct Summary: Equatable, Sendable {
        public var totalRequests: UInt64 = 0
        public var failedRequests: UInt64 = 0
        /// Left out of the sums below: nobody reported their usage.
        public var unknownUsageRequests: UInt64 = 0
        public var inputTokens: UInt64 = 0
        public var outputTokens: UInt64 = 0
        public var cacheReadTokens: UInt64 = 0
        public var cacheCreationTokens: UInt64 = 0
        public var cacheWrite5mTokens: UInt64 = 0
        public var cacheWrite1hTokens: UInt64 = 0

        public init() {}
    }

    public struct Request: Equatable, Sendable, Identifiable {
        public var id: String { requestId }
        public var requestId: String
        public var model: String
        public var targetName: String?
        public var statusCode: Int
        public var success: Bool
        public var latencyMs: UInt64
        public var completedAt: Int64
        public var inputTokens: UInt64
        public var outputTokens: UInt64
        public var cacheReadTokens: UInt64
        /// `reported` / `estimated` / `unknown`.
        public var usageSource: String
        public var errorSummary: String?
        public var attempts: [Attempt]

        public init(requestId: String, model: String = "", targetName: String? = nil, statusCode: Int = 200,
                    success: Bool = true, latencyMs: UInt64 = 0, completedAt: Int64 = 0, inputTokens: UInt64 = 0,
                    outputTokens: UInt64 = 0, cacheReadTokens: UInt64 = 0, usageSource: String = "reported",
                    errorSummary: String? = nil, attempts: [Attempt] = []) {
            self.requestId = requestId
            self.model = model
            self.targetName = targetName
            self.statusCode = statusCode
            self.success = success
            self.latencyMs = latencyMs
            self.completedAt = completedAt
            self.inputTokens = inputTokens
            self.outputTokens = outputTokens
            self.cacheReadTokens = cacheReadTokens
            self.usageSource = usageSource
            self.errorSummary = errorSummary
            self.attempts = attempts
        }
    }

    public struct Attempt: Equatable, Sendable, Identifiable {
        public var id: Int { seq }
        public var seq: Int
        public var targetName: String?
        public var outboundModel: String?
        public var statusCode: Int?
        public var latencyMs: UInt64
        public var inputTokens: UInt64
        public var outputTokens: UInt64
        public var usageSource: String
        public var errorSummary: String?
        /// The circuit breaker kept the request away from this target.
        public var skipped: Bool

        public init(seq: Int, targetName: String? = nil, outboundModel: String? = nil, statusCode: Int? = nil,
                    latencyMs: UInt64 = 0, inputTokens: UInt64 = 0, outputTokens: UInt64 = 0,
                    usageSource: String = "unknown", errorSummary: String? = nil, skipped: Bool = false) {
            self.seq = seq
            self.targetName = targetName
            self.outboundModel = outboundModel
            self.statusCode = statusCode
            self.latencyMs = latencyMs
            self.inputTokens = inputTokens
            self.outputTokens = outputTokens
            self.usageSource = usageSource
            self.errorSummary = errorSummary
            self.skipped = skipped
        }
    }

    public static func decode(_ value: JSONValue) -> LiveGatewayUsage {
        let f = value.fields
        let s = f["summary"]?.fields ?? [:]
        func n(_ fields: [String: JSONValue], _ key: String) -> UInt64 {
            fields[key]?.numberText.flatMap { UInt64($0) } ?? 0
        }
        var summary = Summary()
        summary.totalRequests = n(s, "totalRequests")
        summary.failedRequests = n(s, "failedRequests")
        summary.unknownUsageRequests = n(s, "unknownUsageRequests")
        summary.inputTokens = n(s, "inputTokens")
        summary.outputTokens = n(s, "outputTokens")
        summary.cacheReadTokens = n(s, "cacheReadTokens")
        summary.cacheCreationTokens = n(s, "cacheCreationTokens")
        summary.cacheWrite5mTokens = n(s, "cacheWrite5mTokens")
        summary.cacheWrite1hTokens = n(s, "cacheWrite1hTokens")
        let requests: [Request] = (f["requests"]?.arrayValue ?? []).compactMap { value in
            let r = value.fields
            guard let id = r["requestId"]?.stringValue else { return nil }
            let attempts: [Attempt] = (r["attempts"]?.arrayValue ?? []).compactMap { value in
                let a = value.fields
                guard let seq = a["seq"]?.numberText.flatMap({ Int($0) }) else { return nil }
                return Attempt(
                    seq: seq, targetName: a["targetName"]?.stringValue, outboundModel: a["outboundModel"]?.stringValue,
                    statusCode: a["statusCode"]?.numberText.flatMap { Int($0) }, latencyMs: n(a, "latencyMs"),
                    inputTokens: n(a, "inputTokens"), outputTokens: n(a, "outputTokens"),
                    usageSource: a["usageSource"]?.stringValue ?? "unknown",
                    errorSummary: a["errorSummary"]?.stringValue, skipped: a["skipped"]?.boolValue ?? false)
            }
            return Request(
                requestId: id, model: r["model"]?.stringValue ?? "", targetName: r["targetName"]?.stringValue,
                statusCode: r["statusCode"]?.numberText.flatMap { Int($0) } ?? 0,
                success: r["success"]?.boolValue ?? false, latencyMs: n(r, "latencyMs"),
                completedAt: r["completedAt"]?.numberText.flatMap { Int64($0) } ?? 0,
                inputTokens: n(r, "inputTokens"), outputTokens: n(r, "outputTokens"),
                cacheReadTokens: n(r, "cacheReadTokens"), usageSource: r["usageSource"]?.stringValue ?? "reported",
                errorSummary: r["errorSummary"]?.stringValue, attempts: attempts)
        }
        var usage = LiveGatewayUsage(summary: summary, requests: requests)
        usage.purposes = (f["purposes"]?.arrayValue ?? []).map { value in
            let p = value.fields
            var row = Purpose(purpose: p["purpose"]?.stringValue ?? "")
            row.requests = n(p, "requests")
            row.failedRequests = n(p, "failedRequests")
            row.inputTokens = n(p, "inputTokens")
            row.outputTokens = n(p, "outputTokens")
            row.unknownUsageRequests = n(p, "unknownUsageRequests")
            row.failedAttempts = n(p, "failedAttempts")
            row.failedAttemptInputTokens = n(p, "failedAttemptInputTokens")
            row.failedAttemptOutputTokens = n(p, "failedAttemptOutputTokens")
            row.direct = p["direct"]?.boolValue ?? false
            return row
        }
        return usage
    }
}
