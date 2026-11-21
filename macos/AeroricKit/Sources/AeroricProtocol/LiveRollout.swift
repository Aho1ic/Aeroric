import Foundation

/// H8 (2026-11-21): an automatic behaviour's tier, as `rollout.list`,
/// `rollout.set` and `rollout.revert` return it.
public struct LiveRolloutFeature: Equatable, Sendable, Identifiable {
    /// `recall`, or `skill:<name>` for a learned skill.
    public var id: String
    /// `off`, `candidate`, `described` or `default`.
    public var tier: String
    /// The tiers it was at before each move, oldest first.
    public var history: [String]
    /// Messages it was decided for.
    public var checked: Int
    /// Of those, how many it would have fired on.
    public var would: Int

    public init(id: String, tier: String, history: [String] = [], checked: Int = 0, would: Int = 0) {
        self.id = id
        self.tier = tier
        self.history = history
        self.checked = checked
        self.would = would
    }

    public static func decode(_ value: JSONValue) -> LiveRolloutFeature? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let tier = f["tier"]?.stringValue else { return nil }
        let count = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return LiveRolloutFeature(id: id, tier: tier,
                                  history: (f["history"]?.arrayValue ?? []).compactMap { $0["tier"]?.stringValue },
                                  checked: count("checked"), would: count("would"))
    }

    public static func decodeList(_ value: JSONValue) -> [LiveRolloutFeature] {
        (value["features"]?.arrayValue ?? []).compactMap(decode)
    }

    /// A learned skill (it is never off here; 停用 is on the skill).
    public var isSkill: Bool { id.hasPrefix("skill:") }

    public var skillName: String? { isSkill ? String(id.dropFirst("skill:".count)) : nil }
}
