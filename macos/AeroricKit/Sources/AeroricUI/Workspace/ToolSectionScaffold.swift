import SwiftUI

/// How much of a tool section is actually backed by the host (M4b D3).
public enum ToolSectionStatus: Equatable, Sendable {
    case wired
    /// Shows its content with a one-line caveat under the header.
    case partial(String)
    /// Replaces the content with an explanation; no empty controls, no fake data.
    case comingSoon(String)

    public var showsContent: Bool {
        if case .comingSoon = self { return false }
        return true
    }
}

/// Uniform chrome for the tool sections (Files, Git, IDE, …): a 44pt header
/// with icon, title, project breadcrumb, an optional way back to the current
/// task and section actions, then the content on the canvas.
public struct ToolSectionScaffold<Toolbar: View, Content: View>: View {
    let title: String
    let systemImage: String
    let projectName: String?
    let status: ToolSectionStatus
    let returnTitle: String?
    let onReturn: () -> Void
    let toolbar: Toolbar
    let content: Content

    public init(title: String, systemImage: String, projectName: String?, status: ToolSectionStatus,
                returnTitle: String? = nil, onReturn: @escaping () -> Void = {},
                @ViewBuilder toolbar: () -> Toolbar = { EmptyView() },
                @ViewBuilder content: () -> Content) {
        self.title = title
        self.systemImage = systemImage
        self.projectName = projectName
        self.status = status
        self.returnTitle = returnTitle
        self.onReturn = onReturn
        self.toolbar = toolbar()
        self.content = content()
    }

    public var body: some View {
        VStack(spacing: 0) {
            header
            HairlineDivider()
            if case let .partial(note) = status {
                Label(note, systemImage: "info.circle")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, AeroricSpace.xl)
                    .padding(.vertical, AeroricSpace.s)
                    .background(AeroricColor.layer1)
                    .accessibilityIdentifier("section.partial-note")
                HairlineDivider()
            }
            if case let .comingSoon(detail) = status {
                AeroricEmptyState(systemImage: systemImage, title: "\(title) · 即将支持", message: detail)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("section.coming-soon")
            } else {
                content.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(AeroricColor.canvas)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("section.\(title)")
    }

    private var header: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: systemImage)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(AeroricColor.textSecondary)
            Text(title).font(.system(size: AeroricType.headline, weight: .semibold))
            if let projectName {
                Text("· \(projectName)")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
            }
            if case .comingSoon = status { ComingSoonBadge() }
            Spacer(minLength: AeroricSpace.m)
            if status.showsContent { toolbar }
            if let returnTitle {
                Button(action: onReturn) {
                    Label(returnTitle, systemImage: "arrow.uturn.left")
                        .lineLimit(1)
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .help(returnTitle)
                .accessibilityIdentifier("workspace.return-to-task")
            }
        }
        .padding(.horizontal, AeroricSpace.xl)
        .frame(height: 44)
    }
}
