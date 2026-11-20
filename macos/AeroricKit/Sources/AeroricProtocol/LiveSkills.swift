import Foundation

/// C7 (2026-11-20): one skill in `skills.catalog` — from the App, learned
/// from memory, the user's own folder, or a Claude Code plugin.
public struct LiveSkillDefinition: Equatable, Sendable, Identifiable {
    public var skillId: String
    public var name: String
    /// `builtin` `learned` `user` `plugin`.
    public var source: String
    public var version: String
    public var contentHash: String
    public var path: String?
    public var embedded: Bool
    public var summary: String
    public var harnesses: [String]
    public var active: Bool
    public var note: String?
    /// A name may come from more than one source.
    public var id: String { "\(source)#\(skillId)#\(path ?? "")" }

    public init(skillId: String, name: String = "", source: String, version: String = "", contentHash: String = "",
                path: String? = nil, embedded: Bool = false, summary: String = "", harnesses: [String] = [],
                active: Bool = true, note: String? = nil) {
        self.skillId = skillId
        self.name = name.isEmpty ? skillId : name
        self.source = source
        self.version = version
        self.contentHash = contentHash
        self.path = path
        self.embedded = embedded
        self.summary = summary
        self.harnesses = harnesses
        self.active = active
        self.note = note
    }

    public static func decode(_ value: JSONValue) -> LiveSkillDefinition? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let source = f["source"]?.stringValue else { return nil }
        return LiveSkillDefinition(skillId: id, name: f["name"]?.stringValue ?? id, source: source,
                                   version: f["version"]?.stringValue ?? "",
                                   contentHash: f["contentHash"]?.stringValue ?? "",
                                   path: f["path"]?.stringValue, embedded: f["embedded"]?.boolValue ?? false,
                                   summary: f["summary"]?.stringValue ?? "",
                                   harnesses: (f["harnesses"]?.arrayValue ?? []).compactMap(\.stringValue),
                                   active: f["active"]?.boolValue ?? true, note: f["note"]?.stringValue)
    }
}

/// C7: one skill a launch of a session was given, and why.
public struct LiveMountedSkill: Equatable, Sendable {
    public var skillId: String
    public var source: String
    public var version: String
    public var contentHash: String
    /// `default` `accepted` `userFolder` `plugin`.
    public var reason: String

    public init(skillId: String, source: String, version: String = "", contentHash: String = "", reason: String) {
        self.skillId = skillId
        self.source = source
        self.version = version
        self.contentHash = contentHash
        self.reason = reason
    }

    public static func decode(_ value: JSONValue) -> LiveMountedSkill? {
        let f = value.fields
        guard let id = f["skillId"]?.stringValue, let source = f["source"]?.stringValue else { return nil }
        return LiveMountedSkill(skillId: id, source: source, version: f["version"]?.stringValue ?? "",
                                contentHash: f["contentHash"]?.stringValue ?? "",
                                reason: f["reason"]?.stringValue ?? "")
    }
}

/// C7: one launch (start or resume) of a session and the skills it had.
public struct LiveSkillLaunch: Equatable, Sendable {
    public var atMs: UInt64
    public var harness: String
    public var skills: [LiveMountedSkill]

    public init(atMs: UInt64, harness: String, skills: [LiveMountedSkill]) {
        self.atMs = atMs
        self.harness = harness
        self.skills = skills
    }

    public static func decode(_ value: JSONValue) -> LiveSkillLaunch? {
        let f = value.fields
        guard let at = f["atMs"]?.numberText.flatMap({ UInt64($0) }) else { return nil }
        return LiveSkillLaunch(atMs: at, harness: f["harness"]?.stringValue ?? "",
                               skills: (f["skills"]?.arrayValue ?? []).compactMap(LiveMountedSkill.decode))
    }
}
