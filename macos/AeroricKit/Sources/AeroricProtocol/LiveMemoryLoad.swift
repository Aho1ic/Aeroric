/// E8 (2026-11-22): what a session started now would be handed from memory
/// (`memory.profile`): the characters it uses of the budget, how many items,
/// and what it leaves out — over the budget, or moved out of the opening
/// (found by recall only).
public struct LiveMemoryLoad: Equatable, Sendable {
    public var usedChars: Int
    public var budgetChars: Int
    public var carried: Int
    public var leftOut: [LeftOut]

    public struct LeftOut: Equatable, Sendable, Identifiable {
        public var id: String
        public var kind: String
        public var text: String
        /// `budget` or `demoted`.
        public var why: String

        public init(id: String, kind: String, text: String, why: String) {
            self.id = id
            self.kind = kind
            self.text = text
            self.why = why
        }
    }

    public init(usedChars: Int = 0, budgetChars: Int = 0, carried: Int = 0, leftOut: [LeftOut] = []) {
        self.usedChars = usedChars
        self.budgetChars = budgetChars
        self.carried = carried
        self.leftOut = leftOut
    }

    /// Nil from a host older than E8 (no `budgetChars`).
    public static func decode(_ value: JSONValue) -> LiveMemoryLoad? {
        let f = value.fields
        guard let budget = f["budgetChars"]?.numberText.flatMap({ Int($0) }) else { return nil }
        let number = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        let left: [LeftOut] = (f["leftOut"]?.arrayValue ?? []).compactMap { value in
            let r = value.fields
            guard let id = r["id"]?.stringValue else { return nil }
            return LeftOut(id: id, kind: r["kind"]?.stringValue ?? "", text: r["text"]?.stringValue ?? "",
                           why: r["why"]?.stringValue ?? "budget")
        }
        return LiveMemoryLoad(usedChars: number("usedChars"), budgetChars: budget,
                              carried: number("carried"), leftOut: left)
    }
}
