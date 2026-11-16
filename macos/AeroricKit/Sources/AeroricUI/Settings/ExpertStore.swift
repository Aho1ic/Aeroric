import AeroricClient
import AeroricProtocol
import Foundation
import Observation

/// 设置 → Agents → 求助专家 (2026-11-13, Smart Friend): the model a session's
/// agent may consult with `ask_expert`. One for every session; unset, the
/// tool is not offered. Shown only once the host answered `delegation.expert`.
@MainActor @Observable
public final class ExpertStore {
    public struct Calls: Sendable {
        public var load: @Sendable () async throws -> (LiveExpert?, [LiveExpertChoice])
        public var set: @Sendable (_ configurationId: String, _ model: String?) async throws -> (LiveExpert?, [LiveExpertChoice])
        public var clear: @Sendable () async throws -> (LiveExpert?, [LiveExpertChoice])

        public init(load: @escaping @Sendable () async throws -> (LiveExpert?, [LiveExpertChoice]),
                    set: @escaping @Sendable (String, String?) async throws -> (LiveExpert?, [LiveExpertChoice]),
                    clear: @escaping @Sendable () async throws -> (LiveExpert?, [LiveExpertChoice])) {
            self.load = load
            self.set = set
            self.clear = clear
        }
    }

    public private(set) var isAvailable = false
    public private(set) var expert: LiveExpert?
    public private(set) var choices: [LiveExpertChoice] = []
    public private(set) var lastError: String?
    @ObservationIgnored private var calls: Calls?

    public init() {}

    public func attach(client: SessionsClient?) {
        guard let client else {
            attach(nil)
            return
        }
        attach(Calls(
            load: { try await client.expert() },
            set: { try await client.expert(set: $0, model: $1) },
            clear: { try await client.expert(clear: true) }))
    }

    public func attach(_ calls: Calls?) {
        self.calls = calls
        isAvailable = false
        expert = nil
        choices = []
        lastError = nil
    }

    public func refresh() async {
        guard let calls else {
            isAvailable = false
            return
        }
        do {
            (expert, choices) = try await calls.load()
            isAvailable = true
        } catch {
            isAvailable = false
        }
    }

    public func set(_ configurationId: String, model: String?) async {
        await run { try await $0.set(configurationId, model) }
    }

    public func clear() async {
        await run { try await $0.clear() }
    }

    private func run(_ action: (Calls) async throws -> (LiveExpert?, [LiveExpertChoice])) async {
        guard let calls else { return }
        do {
            (expert, choices) = try await action(calls)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }
}

/// The words of the expert card.
public enum ExpertText {
    public static let detail = "主会话卡住时可以用 ask_expert 请教它：只读，能看到交接清单，需要时自己读项目文件；主会话等它答完（最多 10 分钟），每回合最多问 2 次。不设置就不提供这个工具。"

    public static func current(_ expert: LiveExpert?) -> String {
        expert?.label ?? "未设置"
    }

    /// A configuration's models; one without a list offers its default (nil).
    public static func models(_ choice: LiveExpertChoice) -> [String?] {
        choice.models.isEmpty ? [nil] : choice.models
    }

    public static func isCurrent(_ expert: LiveExpert?, _ choice: LiveExpertChoice, model: String?) -> Bool {
        expert?.configurationId == choice.configurationId && expert?.model == model
    }
}
