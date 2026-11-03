import AeroricProtocol
import SwiftUI

/// R1-2: what the delivery bar says, if anything (Q-S3: nothing while
/// everything arrives). One line for the most pressing group.
public struct DeliveryNotice: Equatable, Sendable {
    public enum Tone: Equatable, Sendable { case quiet, warning, danger }
    public enum Action: Equatable, Sendable { case resume, discard, resend }

    public var tone: Tone
    public var symbol: String
    public var text: String
    /// The buttons, in order, with their titles.
    public var actions: [(action: Action, title: String)]
    /// The items the actions apply to.
    public var ids: [String]

    public static func == (lhs: DeliveryNotice, rhs: DeliveryNotice) -> Bool {
        lhs.tone == rhs.tone && lhs.symbol == rhs.symbol && lhs.text == rhs.text && lhs.ids == rhs.ids
            && lhs.actions.map(\.title) == rhs.actions.map(\.title)
    }
}

public enum DeliveryText {
    /// Failed first (it needs the user), then unknown, then paused, then
    /// what waits for this turn to end.
    public static func notice(_ deliveries: [LiveDelivery]) -> DeliveryNotice? {
        let open = deliveries.filter(\.isOpen).sorted { $0.createdMs < $1.createdMs }
        let failed = open.filter { $0.state == "failed" }
        if let first = failed.first {
            return DeliveryNotice(tone: .danger, symbol: "exclamationmark.circle",
                                  text: failed.count == 1 ? "没送达：\(first.reason ?? "原因不明")"
                                      : "\(failed.count) 条没送达：\(first.reason ?? "原因不明")",
                                  actions: [(.resend, "重试"), (.discard, "丢弃")], ids: failed.map(\.id))
        }
        let unknown = open.filter { $0.state == "unknown" }
        if let first = unknown.first {
            return DeliveryNotice(tone: .warning, symbol: "questionmark.circle",
                                  text: "\(first.label.isEmpty ? "一条消息" : first.label) 不确定是否送达 · "
                                      + (first.reason ?? "正在查会话记录"),
                                  actions: [(.resend, "重发"), (.discard, "不用了")], ids: unknown.map(\.id))
        }
        let paused = open.filter { $0.state == "paused" }
        if !paused.isEmpty {
            return DeliveryNotice(tone: .warning, symbol: "pause.circle",
                                  text: "\(paused.count) 条未发出（你按了停止）",
                                  actions: [(.resume, "继续发送"), (.discard, "丢弃")], ids: paused.map(\.id))
        }
        let waiting = open.filter { $0.state == "waiting" && $0.kind == "user" }
        if !waiting.isEmpty {
            return DeliveryNotice(tone: .quiet, symbol: "clock",
                                  text: "\(waiting.count) 条消息等这轮结束后发送",
                                  actions: [(.discard, "撤回")], ids: waiting.map(\.id))
        }
        return nil
    }

    public static func state(_ delivery: LiveDelivery) -> String {
        switch delivery.state {
        case "waiting": "等待发送"
        case "sending": "发送中"
        case "accepted": "已发出"
        case "delivered": "已送达"
        case "failed": delivery.blocked ? "没送达 · 等你处理" : "没送达"
        case "unknown": "不确定"
        case "paused": "已暂停"
        case "withdrawn": "已撤回"
        default: delivery.state
        }
    }

    public static func evidence(_ delivery: LiveDelivery) -> String? {
        switch delivery.evidence {
        case "protocol": "Agent 回应"
        case "echo": "Claude 回显"
        case "turnStarted": "回合开始"
        case "transcript": "会话记录核对"
        default: nil
        }
    }

    public static func method(_ action: DeliveryNotice.Action) -> String {
        switch action {
        case .resume: "delivery.resume"
        case .discard: "delivery.discard"
        case .resend: "delivery.resend"
        }
    }
}

/// The one-line capsule above the composer.
struct DeliveryBar: View {
    let notice: DeliveryNotice
    let act: (DeliveryNotice.Action) -> Void

    private var tint: Color {
        switch notice.tone {
        case .quiet: AeroricColor.textSecondary
        case .warning: AeroricColor.warning
        case .danger: AeroricColor.danger
        }
    }

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            Image(systemName: notice.symbol)
            Text(notice.text)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: AeroricSpace.s)
            ForEach(notice.actions, id: \.title) { item in
                Button(item.title) { act(item.action) }
                    .buttonStyle(.plain)
                    .foregroundStyle(AeroricColor.accent)
                    .fixedSize()
                    .accessibilityIdentifier("session.delivery.\(DeliveryText.method(item.action))")
            }
        }
        .font(.system(size: AeroricType.caption))
        .foregroundStyle(tint)
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 30)
        .background(Capsule(style: .continuous).fill(notice.tone == .quiet ? AeroricColor.layer2 : tint.opacity(0.12)))
        .accessibilityIdentifier("session.live.delivery")
    }
}

/// 详情 › 投递: this session's recent deliveries and what proved them.
public struct DeliveryTimeline: View {
    let taskID: String
    let load: (String) async -> [LiveDelivery]
    @State private var items: [LiveDelivery] = []

    public init(taskID: String, load: @escaping (String) async -> [LiveDelivery]) {
        self.taskID = taskID
        self.load = load
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("投递")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            if items.isEmpty {
                Text("还没有替这个会话投递过消息")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
            }
            ForEach(items.suffix(8).reversed()) { item in
                HStack(spacing: AeroricSpace.s) {
                    Circle().fill(dot(item)).frame(width: 6, height: 6)
                    Text(item.label.isEmpty ? item.kind : item.label)
                        .lineLimit(1)
                        .truncationMode(.tail)
                    Spacer(minLength: AeroricSpace.s)
                    Text([DeliveryText.state(item), DeliveryText.evidence(item)].compactMap { $0 }.joined(separator: " · "))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .lineLimit(1)
                        .fixedSize()
                }
                .font(.system(size: AeroricType.caption))
                .help(item.reason ?? item.text)
            }
        }
        .task(id: taskID) {
            while !Task.isCancelled {
                items = await load(taskID)
                try? await Task.sleep(for: .seconds(5))
            }
        }
        .accessibilityIdentifier("auxiliary.deliveries")
    }

    private func dot(_ item: LiveDelivery) -> Color {
        switch item.state {
        case "delivered": AeroricColor.success
        case "failed": AeroricColor.danger
        case "unknown", "paused", "waiting": AeroricColor.warning
        default: AeroricColor.textTertiary
        }
    }
}
