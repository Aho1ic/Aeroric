import AeroricClient
import AeroricProtocol
import Foundation
import Observation

/// Agent 配置 → 自动行为 (H8, 2026-11-21): each automatic behaviour's tier,
/// how often it would have fired, a tier picker and 退回.
@MainActor @Observable
public final class RolloutBook {
    public struct Calls: Sendable {
        public var list: @Sendable () async throws -> [LiveRolloutFeature]
        public var set: @Sendable (String, String) async throws -> Void
        public var revert: @Sendable (String) async throws -> Void

        public init(list: @escaping @Sendable () async throws -> [LiveRolloutFeature],
                    set: @escaping @Sendable (String, String) async throws -> Void,
                    revert: @escaping @Sendable (String) async throws -> Void) {
            self.list = list; self.set = set; self.revert = revert
        }
    }

    public private(set) var features: [LiveRolloutFeature] = []
    public private(set) var loaded = false
    public var errorText: String?
    @ObservationIgnored private var calls: Calls?

    public init() {}

    public func attach(client: RolloutClient?) {
        guard let client else {
            attach(nil)
            return
        }
        attach(Calls(list: { try await client.list() },
                     set: { try await client.set(id: $0, tier: $1) },
                     revert: { try await client.revert(id: $0) }))
    }

    public func attach(_ calls: Calls?) {
        self.calls = calls
        features = []
        loaded = false
        errorText = nil
    }

    public func refresh() async {
        guard let calls else { return }
        do {
            features = try await calls.list()
            loaded = true
        } catch {
            errorText = "读取自动行为失败：\(error)"
        }
    }

    private func change(_ what: String, _ run: (Calls) async throws -> Void) async {
        guard let calls else { return }
        do {
            try await run(calls)
            errorText = nil
            await refresh()
        } catch {
            errorText = "\(what)失败：\(error)"
        }
    }

    public func set(_ id: String, tier: String) async {
        guard features.first(where: { $0.id == id })?.tier != tier else { return }
        await change("换档") { try await $0.set(id, tier) }
    }

    public func revert(_ id: String) async { await change("退回") { try await $0.revert(id) } }
}

/// The page's words.
public enum RolloutText {
    public static let tiers = ["off", "candidate", "described", "default"]

    public static func tier(_ tier: String) -> String {
        switch tier {
        case "off": "关"
        case "candidate": "候选"
        case "described": "只挂描述"
        case "default": "默认生效"
        default: tier
        }
    }

    /// The tiers this behaviour can be at: a learned skill is never off here.
    public static func choices(_ feature: LiveRolloutFeature) -> [String] {
        feature.isSkill ? Array(tiers.dropFirst()) : tiers
    }

    public static func title(_ feature: LiveRolloutFeature) -> String {
        if let name = feature.skillName { return "学到的技能 \(name)" }
        return feature.id == "recall" ? "按消息召回记忆和 Wiki" : feature.id
    }

    /// What the current tier does, for this behaviour.
    public static func meaning(_ feature: LiveRolloutFeature) -> String {
        switch (feature.isSkill, feature.tier) {
        case (false, "off"): "不找，也不记录。"
        case (false, "candidate"): "每条消息照常在本地找相关的记忆和 Wiki 页，只记下找到了什么，不附给 Agent。"
        case (false, "described"): "找到时只附出处（记忆的开头几个字、Wiki 页标题），Agent 用得上再用工具取。"
        case (false, "default"): "找到时把记忆原文和 Wiki 页指路附在消息前。"
        case (true, "candidate"): "不挂给会话，只按每条消息记下本来会不会用上。"
        case (true, "described"): "挂给会话，Agent 看到名字和描述，用不用由它。"
        case (true, "default"): "挂给会话，并在开场白里写明遇到相关问题先用。"
        default: ""
        }
    }

    /// "本来会触发 3/10 次（30%）".
    public static func rate(_ feature: LiveRolloutFeature) -> String {
        guard feature.checked > 0 else { return "还没有判断过" }
        let percent = Int((Double(feature.would) / Double(feature.checked) * 100).rounded())
        return "本来会触发 \(feature.would)/\(feature.checked) 次（\(percent)%）"
    }

    /// Where 退回 goes, or nil when there is nowhere lower.
    public static func revertTarget(_ feature: LiveRolloutFeature) -> String? {
        if let last = feature.history.last { return last }
        let choices = choices(feature)
        guard let at = choices.firstIndex(of: feature.tier), at > 0 else { return nil }
        return choices[at - 1]
    }

    public static func revertLabel(_ feature: LiveRolloutFeature) -> String? {
        revertTarget(feature).map { "退回到“\(tier($0))”" }
    }
}
