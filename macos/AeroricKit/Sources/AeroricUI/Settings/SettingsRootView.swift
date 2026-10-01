import SwiftUI

/// Root of the single reusable settings window (P4-04 Action 3): a segmented
/// picker reusing the shared `NativeSegmentedPicker` primitive (ADR-001 §4
/// rule 4), then the selected section's content. Sections that arrive with
/// later tasks render an explicit not-yet-wired state — never a fake
/// completed panel. The section vocabulary itself lives in
/// `SettingsSection.swift`.
public struct SettingsRootView<State: SettingsStateProviding>: View {
    let state: State
    @State private var section: SettingsSection = .application

    public init(state: State) {
        self.state = state
    }

    public var body: some View {
        VStack(spacing: 0) {
            NativeSegmentedPicker(
                selection: $section,
                groupTitle: "设置分区",
                segments: SettingsSection.allCases.map {
                    .init(value: $0, title: $0.titleKey)
                }
            )
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityIdentifier("settings.section-picker")
            Divider().overlay(Color.dsBorder)
            switch section {
            case .application:
                NotWiredYetView(task: "P5-02（设置中的原生模式切换）")
            case .agents:
                NotWiredYetView(task: "P5-03/P5-04（Agent 与会话面板）")
            case .about:
                AboutSection(state: state)
            }
        }
        .frame(width: 520, height: 360)
        .background(NativeMaterial.underWindowBackground.ignoresSafeArea())
        // Kept as the lifecycle test's query target. Caveat (measured
        // 2026-10-01, AX dump): a container identifier propagates onto
        // descendant leaves and replaces their own, so identifiers below
        // this point (settings.environment …) are not AX-queryable while
        // this tag is here. When P5-05 wires real controls it must drop
        // this tag and query section leaves directly.
        .accessibilityIdentifier("settings.root")
    }
}

private struct NotWiredYetView: View {
    let task: String

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: "hourglass")
                .font(.system(size: 24))
                .foregroundStyle(Color.dsTextMuted)
            Text("该分区尚未接入")
                .font(.headline)
                .foregroundStyle(Color.dsText)
            Text("内容将随 \(task) 接入；当前构建不提供此设置。")
                .font(.caption)
                .foregroundStyle(Color.dsTextSecondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
    }
}

private struct AboutSection<State: SettingsStateProviding>: View {
    let state: State

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            LabeledContent("environment") {
                Text(state.environmentLabel)
                    .accessibilityIdentifier("settings.environment")
            }
            LabeledContent("protocol") {
                Text(state.protocolVersionText)
                    .accessibilityIdentifier("settings.protocol")
            }
            LabeledContent("build") {
                Text(state.buildIdentifier)
                    .accessibilityIdentifier("settings.build")
            }
            LabeledContent("application") {
                Text(state.applicationTag)
                    .accessibilityIdentifier("settings.application")
            }
            Spacer()
        }
        .font(.callout)
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .accessibilityIdentifier("settings.about")
    }
}
