import Foundation
import Observation

/// Three-step text sizes for body text, code and the terminal (Q21, D5).
/// Views read the sizes through `AeroricType`; because this
/// is `@Observable`, a view that read a size re-renders when it changes.
/// Only body-level sizes scale; headings stay fixed.
@Observable
public final class AeroricTypeScale: @unchecked Sendable {
    public enum Step: String, CaseIterable, Identifiable, Sendable {
        case small, medium, large, xlarge
        public var id: String { rawValue }
        public var title: String {
            switch self {
            case .small: "小"
            case .medium: "中"
            case .large: "大"
            case .xlarge: "特大"
            }
        }
    }

    public enum Kind: String, CaseIterable, Sendable {
        /// `conversation` (2026-10-28): the session's own messages, set apart
        /// from the interface's body text.
        case text, code, terminal, conversation

        public var defaultsKey: String { "appearance.\(rawValue)Size" }

        public func size(_ step: Step) -> CGFloat {
            switch (self, step) {
            case (.text, .small): 12
            case (.text, .medium): 13
            case (.text, .large): 14
            case (.code, .small): 11
            case (.code, .medium): 12
            case (.code, .large): 13
            case (.terminal, .small): 12
            case (.terminal, .medium): 13
            case (.terminal, .large): 15
            case (.text, .xlarge): 15
            case (.code, .xlarge): 14
            case (.terminal, .xlarge): 17
            case (.conversation, .small): 13
            case (.conversation, .medium): 15
            case (.conversation, .large): 17
            case (.conversation, .xlarge): 19
            }
        }
    }

    public static let shared = AeroricTypeScale()

    public private(set) var text: Step
    public private(set) var code: Step
    public private(set) var terminal: Step
    public private(set) var conversation: Step
    @ObservationIgnored private let defaults: UserDefaults

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        func read(_ kind: Kind) -> Step {
            defaults.string(forKey: kind.defaultsKey).flatMap(Step.init(rawValue:)) ?? .medium
        }
        text = read(.text)
        code = read(.code)
        terminal = read(.terminal)
        conversation = read(.conversation)
    }

    public func step(for kind: Kind) -> Step {
        switch kind {
        case .text: text
        case .code: code
        case .terminal: terminal
        case .conversation: conversation
        }
    }

    public func set(_ step: Step, for kind: Kind) {
        switch kind {
        case .text: text = step
        case .code: code = step
        case .terminal: terminal = step
        case .conversation: conversation = step
        }
        defaults.set(step.rawValue, forKey: kind.defaultsKey)
    }

    public var textSize: CGFloat { Kind.text.size(text) }
    public var codeSize: CGFloat { Kind.code.size(code) }
    public var terminalSize: CGFloat { Kind.terminal.size(terminal) }
    public var conversationSize: CGFloat { Kind.conversation.size(conversation) }
}
