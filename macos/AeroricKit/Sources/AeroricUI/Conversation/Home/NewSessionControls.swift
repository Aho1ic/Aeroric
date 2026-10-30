import SwiftUI

/// D: the four harnesses above the composer — icons for all, the name only
/// on the selected one; a harness with nothing usable is greyed out.
struct HarnessSlider: View {
    let selection: String
    let availability: [String: Bool]
    let onSelect: (String) -> Void

    var body: some View {
        // 2026-10-27: the sliding pill (codeg agent-selector); only the
        // selected harness shows its name.
        SlidingCapsulePicker(
            selection: Binding(get: { selection }, set: onSelect),
            groupTitle: "Harness", titleMode: .selectedOnly, identifier: "new-session.harness",
            segments: NewSessionLayout.harnessOrder.map { id in
                let title = AgentConfigText.harnessTitle(id)
                let usable = availability[id] ?? false
                return .init(value: id, title: title,
                             asset: NativeHarnessPicker.harnesses.first { $0.id == id }?.asset,
                             isEnabled: usable, help: usable ? title : "\(title)：未安装或未配置")
            }
        )
        .fixedSize()
    }
}

/// D: the project's local branches; picking another one asks the App to
/// switch (it checks the working tree and confirms first).
struct BranchPicker: View {
    let current: String?
    let branches: [String]
    let onSelect: (String) -> Void

    var body: some View {
        ComposerPicker(
            title: current ?? "分支", systemImage: "arrow.triangle.branch",
            menu: PickerMenu(
                sections: [PickerSection(id: "branches", title: "本地分支", options: branches.map {
                    PickerOption(id: $0, title: $0, selected: $0 == current)
                })],
                note: branches.isEmpty ? "没有读到分支。" : nil
            ),
            identifier: "new-session.branch"
        ) { branch in
            if branch != current { onSelect(branch) }
        }
    }
}

/// Lays children out left to right and wraps onto the next line.
struct FlowLayout: Layout {
    var spacing: CGFloat = AeroricSpace.s
    /// Each row centred in the width instead of starting at the leading edge.
    var centered = false

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(width: proposal.width ?? .infinity, subviews: subviews)
        let height = rows.last.map { $0.y + $0.height } ?? 0
        let width = rows.map(\.width).max() ?? 0
        return CGSize(width: proposal.width ?? width, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for row in arrange(width: bounds.width, subviews: subviews) {
            var x = bounds.minX + (centered ? max(0, bounds.width - row.width) / 2 : 0)
            for index in row.indices {
                let size = subviews[index].sizeThatFits(.unspecified)
                subviews[index].place(at: CGPoint(x: x, y: bounds.minY + row.y), proposal: ProposedViewSize(size))
                x += size.width + spacing
            }
        }
    }

    private struct Row { var indices: [Int] = []; var y: CGFloat = 0; var width: CGFloat = 0; var height: CGFloat = 0 }

    private func arrange(width: CGFloat, subviews: Subviews) -> [Row] {
        var rows: [Row] = [Row()]
        for index in subviews.indices {
            let size = subviews[index].sizeThatFits(.unspecified)
            let needed = rows[rows.count - 1].indices.isEmpty ? size.width : rows[rows.count - 1].width + spacing + size.width
            if needed > width, !rows[rows.count - 1].indices.isEmpty {
                let y = rows[rows.count - 1].y + rows[rows.count - 1].height + spacing
                rows.append(Row(y: y))
            }
            var row = rows[rows.count - 1]
            row.width = row.indices.isEmpty ? size.width : row.width + spacing + size.width
            row.height = max(row.height, size.height)
            row.indices.append(index)
            rows[rows.count - 1] = row
        }
        return rows.filter { !$0.indices.isEmpty }
    }
}

/// D: a recipe as one capsule; its description is the tooltip.
struct RecipeChip: View {
    let recipe: ScenarioRecipe
    /// Backed by a built-in skill that is on (B2): marked with ✦.
    var skilled = false
    let action: () -> Void
    @State private var hovered = false
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        Button(action: action) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: recipe.symbol)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(AeroricColor.textSecondary)
                Text(recipe.title)
                    .font(.system(size: AeroricType.secondary, weight: .medium))
                    .foregroundStyle(AeroricColor.text)
                    .lineLimit(1)
                if skilled {
                    Image(systemName: "sparkle")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(AeroricColor.accent)
                        .accessibilityLabel("内置技能")
                }
            }
            .padding(.horizontal, AeroricSpace.l)
            .frame(height: 30)
            .background(hovered && isEnabled ? AeroricColor.hover : AeroricColor.layer1, in: Capsule(style: .continuous))
            .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
            .contentShape(Capsule(style: .continuous))
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help(skilled ? "\(recipe.detail) · 使用内置技能" : recipe.detail)
        .accessibilityLabel(recipe.title)
        .accessibilityHint(recipe.detail)
        .accessibilityIdentifier("new-session.recipe.\(recipe.id)")
    }
}
