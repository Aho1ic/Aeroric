import SwiftUI

/// One-line tool call: status · icon · monospaced name · summary · ±stats.
/// Click expands an opaque detail card with the arguments and the output.
struct ToolCallLine: View {
    let call: ToolCall
    let index: Int
    @State private var expanded = false
    @State private var hovered = false

    static let outputLineLimit = 200

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            Button { expanded.toggle() } label: {
                HStack(spacing: AeroricSpace.m) {
                    StatusDot(call.state.activity, label: stateLabel)
                    Image(systemName: call.symbol)
                        .font(.system(size: 12))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .frame(width: 16)
                    ShimmerText(text: call.name, active: call.state == .running)
                    if !call.summary.isEmpty {
                        Text(call.summary)
                            .font(.system(size: AeroricType.secondary))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    Spacer(minLength: AeroricSpace.s)
                    if let stats = call.stats {
                        HStack(spacing: AeroricSpace.xs) {
                            if stats.added > 0 { Text("+\(stats.added)").foregroundStyle(AeroricColor.success) }
                            if stats.removed > 0 { Text("−\(stats.removed)").foregroundStyle(AeroricColor.danger) }
                        }
                        .font(.system(size: AeroricType.caption, design: .monospaced))
                    }
                    if call.state == .unfinished {
                        Text("未完成").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                    }
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .rotationEffect(.degrees(expanded ? 90 : 0))
                }
                .padding(.horizontal, AeroricSpace.m)
                .frame(height: 28)
                .background(Capsule(style: .continuous).fill(hovered ? AeroricColor.hover : .clear))
                .contentShape(Capsule(style: .continuous))
            }
            .buttonStyle(.plain)
            .onHover { hovered = $0 }
            .accessibilityLabel("\(call.name)，\(stateLabel)")
            .accessibilityIdentifier("session.tool.\(index)")

            if expanded { detail }
        }
    }

    private var stateLabel: String {
        switch call.state {
        case .running: "运行中"
        case .done: "完成"
        case .unfinished: "未完成"
        }
    }

    private var detail: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            if !call.input.isEmpty {
                section("参数") { CodeText(call.input) }
            }
            if let output = call.output {
                let lines = output.split(separator: "\n", omittingEmptySubsequences: false)
                let shown = lines.count > Self.outputLineLimit
                    ? lines.prefix(Self.outputLineLimit).joined(separator: "\n") : output
                section("输出") {
                    if DiffText.looksLikeDiff(shown) { DiffText(shown) } else { CodeText(shown) }
                    if lines.count > Self.outputLineLimit {
                        Text("仅显示前 \(Self.outputLineLimit) 行，共 \(lines.count) 行")
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                    }
                }
            } else if call.state == .running {
                Text("等待输出…").font(.system(size: AeroricType.secondary)).foregroundStyle(AeroricColor.textTertiary)
            }
        }
        .padding(AeroricSpace.l)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.m))
        .overlay { AeroricRadius.shape(AeroricRadius.m).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
        .padding(.leading, AeroricSpace.xxxl)
        .accessibilityIdentifier("session.tool.\(index).detail")
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            Text(title).font(AeroricType.sectionLabel()).foregroundStyle(AeroricColor.textTertiary)
            content()
        }
    }
}

/// Three or more finished calls folded into "n 个工具调用".
struct ToolGroupLine: View {
    let calls: [ToolCall]
    let firstIndex: Int
    @State private var expanded = false
    @State private var hovered = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xxs) {
            Button { expanded.toggle() } label: {
                HStack(spacing: AeroricSpace.m) {
                    StatusDot(.done, label: "完成")
                    HStack(spacing: AeroricSpace.xs) {
                        ForEach(Array(Set(calls.map(\.symbol))).sorted().prefix(4), id: \.self) {
                            Image(systemName: $0).font(.system(size: 11))
                        }
                    }
                    .foregroundStyle(AeroricColor.textSecondary)
                    Text("\(calls.count) 个工具调用")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                    Spacer(minLength: AeroricSpace.s)
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .rotationEffect(.degrees(expanded ? 90 : 0))
                }
                .padding(.horizontal, AeroricSpace.m)
                .frame(height: 28)
                .background(Capsule(style: .continuous).fill(hovered ? AeroricColor.hover : .clear))
                .contentShape(Capsule(style: .continuous))
            }
            .buttonStyle(.plain)
            .onHover { hovered = $0 }
            .accessibilityIdentifier("session.tool-group.\(firstIndex)")
            if expanded {
                ForEach(Array(calls.enumerated()), id: \.element.id) { offset, call in
                    ToolCallLine(call: call, index: firstIndex + offset).padding(.leading, AeroricSpace.l)
                }
            }
        }
    }
}

/// Monospaced title with a moving highlight while the call runs.
private struct ShimmerText: View {
    let text: String
    let active: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        let label = Text(text)
            .font(.system(size: AeroricType.secondary, weight: .medium, design: .monospaced))
            .foregroundStyle(AeroricColor.text)
            .lineLimit(1)
            .fixedSize()
        if active && !reduceMotion {
            TimelineView(.animation) { context in
                let phase = context.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1.6) / 1.6
                label.overlay {
                    GeometryReader { proxy in
                        LinearGradient(colors: [.clear, AeroricColor.canvas.opacity(0.75), .clear],
                                       startPoint: .leading, endPoint: .trailing)
                            .frame(width: proxy.size.width * 0.5)
                            .offset(x: (phase * 1.5 - 0.5) * proxy.size.width)
                    }
                    .mask(label)
                    .allowsHitTesting(false)
                }
            }
        } else {
            label
        }
    }
}
