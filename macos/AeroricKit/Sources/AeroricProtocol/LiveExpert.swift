import Foundation

/// 2026-11-13 Smart Friend: the model a session's agent consults through
/// `ask_expert`, as `delegation.expert` returns it.
public struct LiveExpert: Equatable, Sendable {
    public var configurationId: String
    public var agent: String
    public var model: String?
    /// "Claude 官方 · opus".
    public var label: String

    public init(configurationId: String, agent: String, model: String? = nil, label: String) {
        self.configurationId = configurationId
        self.agent = agent
        self.model = model
        self.label = label
    }

    public static func decode(_ value: JSONValue?) -> LiveExpert? {
        guard let f = value?.objectValue, let id = f["configurationId"]?.stringValue else { return nil }
        return LiveExpert(configurationId: id, agent: f["agent"]?.stringValue ?? "",
                          model: f["model"]?.stringValue, label: f["label"]?.stringValue ?? id)
    }
}

/// A configuration the expert can run on (Claude Code or Codex, startable).
public struct LiveExpertChoice: Equatable, Sendable, Identifiable {
    public var id: String { configurationId }
    public var configurationId: String
    public var title: String
    public var agent: String
    public var models: [String]

    public init(configurationId: String, title: String, agent: String, models: [String] = []) {
        self.configurationId = configurationId
        self.title = title
        self.agent = agent
        self.models = models
    }

    public static func decode(_ value: JSONValue) -> LiveExpertChoice? {
        guard let f = value.objectValue, let id = f["configurationId"]?.stringValue else { return nil }
        return LiveExpertChoice(configurationId: id, title: f["title"]?.stringValue ?? id,
                                agent: f["agent"]?.stringValue ?? "",
                                models: (f["models"]?.arrayValue ?? []).compactMap(\.stringValue))
    }
}
