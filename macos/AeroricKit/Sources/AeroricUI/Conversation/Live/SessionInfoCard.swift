import SwiftUI
import AeroricProtocol

/// Where the session-info column goes and how wide it is (2026-10-26 E).
public enum SessionInfoLayout {
    public static let minWidth: CGFloat = 260
    public static let maxWidth: CGFloat = 340
    public static let defaultWidth: CGFloat = 280
    /// The drag strip between the conversation and the column.
    public static let handleWidth: CGFloat = 7
    public static let widthKey = "aeroric.session.infoWidth"

    /// F2: the column folds into a button unless the conversation keeps
    /// `minimumConversationWidth` beside it; the saved preference is untouched.
    public static func showsColumn(collapsed: Bool, centreWidth: CGFloat,
                                   columnWidth: CGFloat = defaultWidth) -> Bool {
        !collapsed && centreWidth - clampWidth(columnWidth) - handleWidth
            >= NativePanelGeometry.mainShell.minimumConversationWidth
    }

    public static func clampWidth(_ width: CGFloat) -> CGFloat {
        min(max(width, minWidth), maxWidth)
    }
}

/// The collapsible "会话信息" (Q27): a column beside the conversation, or a
/// floating card when shown from the folded button.
struct SessionInfoCard: View {
    let state: LiveSessionState?
    let model: LiveSessionModel?
    let facts: LiveSessionFacts
    @Binding var collapsed: Bool
    /// A full-height column next to the conversation instead of a floating card.
    var asColumn = false

    private static let dateFormat: Date.FormatStyle = .dateTime.month().day().hour().minute()

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.l)
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Button { collapsed.toggle() } label: {
                HStack(spacing: AeroricSpace.s) {
                    Image(systemName: "info.circle")
                    Text("会话信息").font(.system(size: AeroricType.secondary, weight: .semibold))
                    Spacer(minLength: 0)
                    Image(systemName: collapsed ? "chevron.down" : "chevron.up")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("session.info.toggle")
            if !collapsed || asColumn {
                ScrollView {
                    VStack(alignment: .leading, spacing: AeroricSpace.m) {
                        planSection
                        subagentSection
                        details
                    }
                }
                .frame(maxHeight: asColumn ? .infinity : 420)
            }
        }
        .padding(AeroricSpace.l)
        .frame(maxWidth: asColumn ? .infinity : 260, maxHeight: asColumn ? .infinity : nil, alignment: .topLeading)
        .modifier(FloatingCardChrome(enabled: !asColumn, shape: shape))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.info")
    }

    @ViewBuilder private var planSection: some View {
        let plan = model?.plan ?? state?.plan ?? []
        if !plan.isEmpty {
            section("计划") {
                ForEach(Array(plan.enumerated()), id: \.offset) { _, entry in
                    Label {
                        Text(entry.text).strikethrough(entry.status == "completed")
                    } icon: {
                        Image(systemName: entry.status == "completed" ? "checkmark.circle.fill"
                              : entry.status == "inProgress" ? "circle.dotted" : "circle")
                            .foregroundStyle(entry.status == "completed" ? AeroricColor.success : AeroricColor.textTertiary)
                    }
                    .font(.system(size: AeroricType.caption))
                }
            }
        }
    }

    @ViewBuilder private var subagentSection: some View {
        if let subagents = model?.subagents, !subagents.isEmpty {
            section("子 Agent") {
                ForEach(subagents, id: \.id) { agent in
                    HStack {
                        Text(agent.title).lineLimit(1)
                        Spacer()
                        Text(agent.status).foregroundStyle(AeroricColor.textTertiary)
                    }
                    .font(.system(size: AeroricType.caption))
                }
            }
        }
    }

    private var details: some View {
        let config = model?.config
        return section("会话") {
            row("Agent", [AgentGlyph.displayName(for: facts.agent), facts.configurationTitle].compactMap { $0 }.joined(separator: " · "))
            row("模型", config?.model ?? state?.model ?? "—")
            row("权限", (config?.permission ?? state?.permission).map(ComposerPickerOptions.permissionTitle) ?? "—")
            if let effort = config?.effort ?? state?.effort { row("推理强度", ComposerPickerOptions.effortTitle(effort)) }
            row("工作目录", facts.workingDirectory.map(ComposerPickerOptions.abbreviatedPath) ?? "—")
            if let branch = facts.branch { row("分支", branch) }
            row("会话 id", state?.sessionId ?? "—", copyable: state?.sessionId != nil)
            row("开始时间", facts.startedAt?.formatted(Self.dateFormat) ?? "—")
            row("改动", facts.changesText)
        }
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text(title)
                .font(.system(size: AeroricType.caption, weight: .semibold))
                .foregroundStyle(AeroricColor.textTertiary)
            content()
        }
    }

    private func row(_ label: String, _ value: String, copyable: Bool = false) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.s) {
            Text(label).foregroundStyle(AeroricColor.textTertiary).frame(width: 56, alignment: .leading)
            Text(value).lineLimit(2).truncationMode(.middle).textSelection(.enabled)
            if copyable {
                Button {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(value, forType: .string)
                } label: { Image(systemName: "doc.on.doc").imageScale(.small) }
                    .buttonStyle(.plain)
                    .help("复制")
            }
        }
        .font(.system(size: AeroricType.caption))
    }
}

/// Glass and shadow only for the floating card; the column sits flat.
private struct FloatingCardChrome<S: InsettableShape>: ViewModifier {
    let enabled: Bool
    let shape: S

    func body(content: Content) -> some View {
        if enabled {
            content.glassSurface(.floatingCard, in: shape).aeroricElevation(.floating, in: shape)
        } else {
            content
        }
    }
}
