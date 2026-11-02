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

/// Uniform chrome for the tool sections (Files, Git, IDE, …). The icon,
/// title and project go to the window toolbar (`WindowTitle`, 2026-11-02);
/// a 44pt row keeps the section actions and the way back to the current
/// task, and is left out when there are neither.
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
            if hasRow {
                header
                HairlineDivider()
            }
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
        .windowTitle(WindowTitle(title, subtitle: subtitle, systemImage: systemImage))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("section.\(title)")
    }

    private var subtitle: String? {
        if case .comingSoon = status { return [projectName, "即将支持"].compactMap { $0 }.joined(separator: " · ") }
        return projectName
    }

    private var hasRow: Bool {
        (status.showsContent && Toolbar.self != EmptyView.self) || returnTitle != nil
    }

    private var header: some View {
        HStack(spacing: AeroricSpace.m) {
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
