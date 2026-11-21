import AeroricProtocol
import SwiftUI

/// Agent 配置 → 自动行为 (H8, 2026-11-21): each automatic behaviour starts
/// as a candidate (only recorded), then only described, then on by default;
/// any tier goes back with one click.
public struct AutomaticBehaviorsPane: View {
    let book: RolloutBook

    public init(book: RolloutBook) { self.book = book }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AeroricSpace.l) {
                Text("Aeroric 自己会做的事分三档：候选只记录本来会不会触发，只挂描述是告诉 Agent 有这个东西，默认生效才直接起作用。看“本来会触发”的比例再决定升不升；任何一档都能一键退回。改动从下一条消息或下次启动起生效。")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .fixedSize(horizontal: false, vertical: true)
                if let error = book.errorText {
                    Text(error)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.danger)
                        .textSelection(.enabled)
                }
                ForEach(book.features) { feature in card(feature) }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(.horizontal, AeroricSpace.huge)
            .padding(.vertical, AeroricSpace.xxxl)
            .frame(maxWidth: .infinity)
        }
        .scrollIndicators(.hidden)
        .task { await book.refresh() }
        .accessibilityIdentifier("agent-config.rollout")
    }

    private func card(_ feature: LiveRolloutFeature) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.m) {
                Text(RolloutText.title(feature))
                    .font(.system(size: AeroricType.secondary, weight: .medium))
                Spacer(minLength: AeroricSpace.m)
                Picker("档位", selection: Binding(get: { feature.tier },
                                                set: { tier in Task { await book.set(feature.id, tier: tier) } })) {
                    ForEach(RolloutText.choices(feature), id: \.self) { tier in
                        Text(RolloutText.tier(tier)).tag(tier)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()
                .accessibilityIdentifier("agent-config.rollout.tier.\(feature.id)")
            }
            Text(RolloutText.meaning(feature))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: AeroricSpace.m) {
                Text(RolloutText.rate(feature))
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                Spacer(minLength: AeroricSpace.m)
                if let label = RolloutText.revertLabel(feature) {
                    Button(label) { Task { await book.revert(feature.id) } }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .accessibilityIdentifier("agent-config.rollout.revert.\(feature.id)")
                }
            }
        }
        .padding(AeroricSpace.l)
        .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.l))
    }
}
