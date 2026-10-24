import Foundation
import CryptoKit

/// Sanitized host-owned directory. Never a CLI config-file decoder.
public struct NativeConfigurationCatalog: Decodable, Equatable, Sendable {
    public let schemaVersion: Int
    public let projectId: String
    public let revision: String
    public let asOf: String
    public let configurations: [NativeAgentConfiguration]
}

public struct NativeAgentConfiguration: Decodable, Equatable, Sendable {
    public let id: String
    public let version: String
    public let displayName: String
    public let agent: String
    public let harness: String
    public let models: [String]
    public let efforts: [String]
    public let permissionModes: [String]
    public let blockedReasons: [String]
}

/// Mirrors aeroric_protocol::submission::SubmissionRequest. Fingerprinting is
/// defined by length-prefixed UTF-8 bytes, independent of JSON encoder behavior.
public struct NativeTaskSubmission: Equatable, Sendable {
    public let submissionId: String
    public let projectId: String
    public let configurationId: String
    public let configurationVersion: String
    public let agent: String
    public let prompt: String
    public let permissionMode: String
    public let permissionAgent: String
    public let selectedModel: String?
    public let reasoningEffort: String?
    public let attachments: [String]
    public let worktree: String?
    public let recipeId: String?
    public let team: Bool
    public let review: Bool
    public let evolution: Bool

    public init(submissionId: String, projectId: String, configurationId: String,
                configurationVersion: String, agent: String, prompt: String,
                permissionMode: String, permissionAgent: String,
                selectedModel: String?, reasoningEffort: String? = nil,
                attachments: [String] = [], worktree: String? = nil, recipeId: String? = nil,
                team: Bool = false, review: Bool = false, evolution: Bool = false) {
        self.submissionId = submissionId
        self.projectId = projectId
        self.configurationId = configurationId
        self.configurationVersion = configurationVersion
        self.agent = agent
        self.prompt = prompt
        self.permissionMode = permissionMode
        self.permissionAgent = permissionAgent
        self.selectedModel = selectedModel
        self.reasoningEffort = reasoningEffort
        self.attachments = attachments
        self.worktree = worktree
        self.recipeId = recipeId
        self.team = team
        self.review = review
        self.evolution = evolution
    }

    public var fingerprintMaterial: Data {
        var bytes = Data("aeroric.task-submission.v1\0".utf8)
        func count(_ value: UInt64) {
            var bigEndian = value.bigEndian
            withUnsafeBytes(of: &bigEndian) { bytes.append(contentsOf: $0) }
        }
        func text(_ value: String) {
            let utf8 = Data(value.utf8)
            count(UInt64(utf8.count))
            bytes.append(utf8)
        }
        func optional(_ value: String?) {
            bytes.append(value == nil ? 0 : 1)
            if let value { text(value) }
        }
        for value in [projectId, configurationId, configurationVersion, agent,
                      prompt, permissionMode, permissionAgent] { text(value) }
        optional(selectedModel)
        optional(reasoningEffort)
        count(UInt64(attachments.count))
        attachments.forEach(text)
        optional(worktree)
        optional(recipeId)
        bytes.append(contentsOf: [team, review, evolution].map { $0 ? 1 : 0 })
        return bytes
    }

    public var fingerprint: String {
        "sha256:" + SHA256.hash(data: fingerprintMaterial).map { String(format: "%02x", $0) }.joined()
    }

    public var json: JSONValue {
        .object([
            "schemaVersion": .number("1"), "submissionId": .string(submissionId),
            "projectId": .string(projectId), "configurationId": .string(configurationId),
            "configurationVersion": .string(configurationVersion), "fingerprint": .string(fingerprint),
            "agent": .string(agent), "prompt": .string(prompt),
            "permissionMode": .string(permissionMode), "permissionAgent": .string(permissionAgent),
            "selectedModel": selectedModel.map(JSONValue.string) ?? .null,
            "reasoningEffort": reasoningEffort.map(JSONValue.string) ?? .null,
            "attachments": .array(attachments.map(JSONValue.string)),
            "worktree": worktree.map(JSONValue.string) ?? .null,
            "recipeId": recipeId.map(JSONValue.string) ?? .null,
            "team": .bool(team), "review": .bool(review), "evolution": .bool(evolution),
        ])
    }
}

public struct NativeSubmissionReceipt: Decodable, Equatable, Sendable {
    public struct Consumption: Decodable, Equatable, Sendable {
        public let option: String
        public let applied: Bool
    }
    public let schemaVersion: Int
    public let submissionId: String
    public let projectId: String
    public let configurationId: String
    public let configurationVersion: String
    public let fingerprint: String
    public let state: String
    public let taskId: String?
    public let reasons: [String]
    public let agent: String
    public let selectedModel: String?
    public let reasoningEffort: String?
    public let permissionMode: String
    public let permissionAgent: String
    public let options: [Consumption]

    /// An accepted ID alone is insufficient: every adopted option must agree.
    public func matches(_ request: NativeTaskSubmission) -> Bool {
        guard schemaVersion == 1, submissionId == request.submissionId,
              projectId == request.projectId, fingerprint == request.fingerprint,
              configurationId == request.configurationId,
              configurationVersion == request.configurationVersion,
              agent == request.agent, selectedModel == request.selectedModel,
              reasoningEffort == request.reasoningEffort,
              permissionMode == request.permissionMode, permissionAgent == request.permissionAgent,
              ["accepted", "pending", "unknown", "rejected_before_side_effect"].contains(state)
        else { return false }
        if state == "accepted" {
            let expected = [
                "configuration": true, "model": request.selectedModel != nil,
                "effort": request.reasoningEffort != nil, "permission": true,
                "attachments": false, "worktree": false, "recipe": false,
                "team": false, "review": false, "evolution": false,
            ]
            guard taskId == request.submissionId, reasons.isEmpty,
                  request.attachments.isEmpty, request.worktree == nil, request.recipeId == nil,
                  !request.team, !request.review, !request.evolution,
                  options.count == expected.count, Set(options.map(\.option)).count == expected.count
            else { return false }
            return options.allSatisfy { expected[$0.option] == $0.applied }
        }
        return taskId == nil && options.isEmpty
            && (state != "rejected_before_side_effect" || !reasons.isEmpty)
    }
}

public struct NativeSubmissionLookup: Decodable, Equatable, Sendable {
    public let schemaVersion: Int
    public let submissionId: String
    public let fingerprint: String
    /// Absence is unknown; it never authorizes retry.
    public let receipt: NativeSubmissionReceipt?
}
