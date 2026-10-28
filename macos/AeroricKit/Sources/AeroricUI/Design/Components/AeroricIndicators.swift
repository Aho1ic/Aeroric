import SwiftUI

// Overhaul M1 indicators and layout pieces: status dots, agent glyphs,
// section headers, empty states, hairlines and floating cards.

/// The five things a task can be doing, mapped from the host's wire
/// vocabulary (`pending`/`running`/`detached`/`input_required`/`done`/
/// `failed`/`cancelled`). "The glyph says who, the dot says what" (orca).
public enum ActivityState: Equatable, Sendable {
    case idle, running, waiting, done, failed

    public init(wireStatus: String?) {
        switch wireStatus {
        case "pending", "running", "detached": self = .running
        case "input_required": self = .waiting
        case "done": self = .done
        case "failed": self = .failed
        default: self = .idle
        }
    }

    public var color: Color {
        switch self {
        case .idle: AeroricColor.textTertiary.opacity(0.6)
        case .running: AeroricColor.running
        case .waiting: AeroricColor.warning
        case .done: AeroricColor.success
        case .failed: AeroricColor.danger
        }
    }
}

/// 6pt dot in a 12pt slot; running is a faint ring with a rotating arc.
public struct StatusDot: View {
    let state: ActivityState
    let label: String
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(_ state: ActivityState, label: String) {
        self.state = state
        self.label = label
    }

    public var body: some View {
        Group {
            if state == .running {
                runningIndicator
            } else {
                Circle().fill(state.color).frame(width: 6, height: 6)
            }
        }
        .frame(width: 12, height: 12)
        .help(label)
        .accessibilityElement()
        .accessibilityLabel(label)
    }

    @ViewBuilder private var runningIndicator: some View {
        let ring = Circle().stroke(state.color.opacity(0.25), lineWidth: 1.5).frame(width: 9, height: 9)
        if reduceMotion {
            ring.overlay(Circle().fill(state.color).frame(width: 4, height: 4))
        } else {
            TimelineView(.animation) { context in
                let angle = context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1) * 360
                ring.overlay(
                    Circle()
                        .trim(from: 0, to: 0.3)
                        .stroke(state.color, style: StrokeStyle(lineWidth: 1.5, lineCap: .round))
                        .frame(width: 9, height: 9)
                        .rotationEffect(.degrees(angle))
                )
            }
        }
    }
}

/// Monochrome agent glyph. Never the vendor's logo or wordmark (brand rules);
/// an SF Symbol per family, with the full name as tooltip.
public struct AgentGlyph: View {
    let agent: String?
    let size: CGFloat

    public init(agent: String?, size: CGFloat = 14) {
        self.agent = agent
        self.size = size
    }

    public static func symbol(for agent: String?) -> String {
        switch agent?.lowercased() {
        case "claude", "claude_gpt55": "asterisk"
        case "codex": "chevron.left.forwardslash.chevron.right"
        case "dsh": "water.waves"
        case "omp": "circle.hexagongrid"
        case "qoder": "q.circle"
        case "cursor": "cursorarrow"
        case "opencode": "terminal"
        case nil, "": "bubble.left"
        default: "cpu"
        }
    }

    public static func displayName(for agent: String?) -> String {
        switch agent?.lowercased() {
        case "claude": "Claude Code"
        case "claude_gpt55": "Claude Code (GPT-5.5)"
        case "codex": "Codex"
        case "dsh": "DSH"
        case "omp": "oh-my-pi"
        case "qoder": "Qoder"
        case "cursor": "Cursor"
        case "opencode": "OpenCode"
        case nil, "": "会话"
        default: agent ?? "Agent"
        }
    }

    public var body: some View {
        Image(systemName: Self.symbol(for: agent))
            .font(.system(size: size * 0.78, weight: .medium))
            .foregroundStyle(AeroricColor.textSecondary)
            .frame(width: size, height: size)
            .help(Self.displayName(for: agent))
            .accessibilityLabel(Self.displayName(for: agent))
    }
}

/// 11pt section label with an optional trailing accessory.
public struct SectionHeader<Trailing: View>: View {
    let title: String
    let trailing: () -> Trailing

    public init(_ title: String, @ViewBuilder trailing: @escaping () -> Trailing = { EmptyView() }) {
        self.title = title
        self.trailing = trailing
    }

    public var body: some View {
        HStack(spacing: AeroricSpace.xs) {
            Text(title)
                .font(AeroricType.sectionLabel())
                .foregroundStyle(AeroricColor.textTertiary)
            Spacer(minLength: 0)
            trailing()
        }
        .padding(.horizontal, AeroricSpace.m)
        .frame(height: 22)
    }
}

/// Centred empty / status state: glyph, title, one-line message, optional action.
public struct AeroricEmptyState<Action: View>: View {
    let systemImage: String
    let title: String
    let message: String?
    let tint: Color
    let titleIdentifier: String?
    let action: () -> Action

    public init(systemImage: String, title: String, message: String? = nil,
                tint: Color = AeroricColor.textTertiary, titleIdentifier: String? = nil,
                @ViewBuilder action: @escaping () -> Action = { EmptyView() }) {
        self.systemImage = systemImage
        self.title = title
        self.message = message
        self.tint = tint
        self.titleIdentifier = titleIdentifier
        self.action = action
    }

    public var body: some View {
        VStack(spacing: AeroricSpace.m) {
            Image(systemName: systemImage)
                .font(.system(size: 24, weight: .regular))
                .foregroundStyle(tint)
                .frame(width: 48, height: 48)
                .background(Circle().fill(AeroricColor.layer2))
                .padding(.bottom, AeroricSpace.xs)
            Text(title)
                .font(.system(size: AeroricType.headline, weight: .semibold))
                .foregroundStyle(AeroricColor.text)
                .accessibilityIdentifier(titleIdentifier ?? "")
            if let message {
                Text(message)
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 360)
            }
            action().padding(.top, AeroricSpace.xs)
        }
        .padding(AeroricSpace.xxxl)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// 0.5pt separator from the alpha ladder.
public struct HairlineDivider: View {
    public enum Axis: Sendable { case horizontal, vertical }
    let axis: Axis
    @Environment(\.colorSchemeContrast) private var contrast

    public init(_ axis: Axis = .horizontal) { self.axis = axis }

    public var body: some View {
        let color = contrast == .increased ? AeroricColor.separator4 : AeroricColor.separator2
        let thickness: CGFloat = contrast == .increased ? 1 : 0.5
        switch axis {
        case .horizontal: color.frame(height: thickness)
        case .vertical: color.frame(width: thickness)
        }
    }
}

/// Floating glass card (composer, hero input, popovers built by hand).
public struct FloatingCard<Content: View>: View {
    let radius: CGFloat
    let content: Content

    public init(radius: CGFloat = AeroricRadius.panel, @ViewBuilder content: () -> Content) {
        self.radius = radius
        self.content = content()
    }

    public var body: some View {
        content
            .glassSurface(.floatingCard, in: AeroricRadius.shape(radius))
            .aeroricElevation(.soft, in: AeroricRadius.shape(radius))
    }
}

/// Compact relative time for list rows ("刚刚", "5 分钟", "2 小时", "3 天").
public enum RelativeTime {
    public static func short(_ date: Date, now: Date = Date()) -> String {
        let seconds = max(0, now.timeIntervalSince(date))
        switch seconds {
        case ..<60: return "刚刚"
        case ..<3600: return "\(Int(seconds / 60)) 分钟"
        case ..<86_400: return "\(Int(seconds / 3600)) 小时"
        case ..<(86_400 * 30): return "\(Int(seconds / 86_400)) 天"
        default:
            return date.formatted(.dateTime.month(.defaultDigits).day())
        }
    }
}
