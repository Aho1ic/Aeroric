import AeroricProtocol
import SwiftUI

/// R1-3b (2026-11-04): what the user agreed to or refused about committing,
/// pushing and releasing — the same lines the handoff brief carries. Read-only.
struct ConsentSection: View {
    let consents: [LiveConsent]

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.s) {
                CheckpointSectionLabel(title: "授权记录")
                Spacer(minLength: AeroricSpace.s)
                Text("仅供参考，不代表本次授权")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
            }
            ForEach(Array(ConsentText.ordered(consents).enumerated()), id: \.offset) { _, consent in
                // Two lines so the quote gets the row's full width (2026-11-04 目检).
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: AeroricSpace.m) {
                        CheckpointBadge(text: ConsentText.badge(consent),
                                        tint: consent.stance == "yes" ? AeroricColor.success : AeroricColor.warning)
                        Spacer(minLength: AeroricSpace.s)
                        Text(ConsentText.when(consent))
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .lineLimit(1)
                            .fixedSize()
                    }
                    Text("「\(consent.quote)」")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.text)
                        .lineLimit(1)
                        .truncationMode(.tail)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .help(consent.quote)
                }
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, 7)
                .background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(AeroricColor.layer2))
            }
        }
        .accessibilityIdentifier("progress.consents")
    }
}

public enum ConsentText {
    private static let kinds = ["commit", "push", "release"]

    /// Grouped by kind in the brief's order, each kind oldest first.
    public static func ordered(_ consents: [LiveConsent]) -> [LiveConsent] {
        kinds.flatMap { kind in consents.filter { $0.kind == kind } }
    }

    /// "提交 · 同意".
    public static func badge(_ consent: LiveConsent) -> String {
        let kind = switch consent.kind {
        case "commit": "提交"
        case "push": "推送"
        case "release": "发布"
        default: consent.kind
        }
        return "\(kind) · \(consent.stance == "yes" ? "同意" : "否定")"
    }

    /// "第 12 轮 · 11-04 00:32".
    public static func when(_ consent: LiveConsent) -> String {
        guard consent.atMs > 0 else { return "第 \(consent.turn) 轮" }
        let formatter = DateFormatter()
        formatter.dateFormat = "MM-dd HH:mm"
        let date = Date(timeIntervalSince1970: TimeInterval(consent.atMs) / 1000)
        return "第 \(consent.turn) 轮 · \(formatter.string(from: date))"
    }
}
