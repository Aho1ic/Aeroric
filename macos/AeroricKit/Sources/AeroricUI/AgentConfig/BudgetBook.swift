import AeroricClient
import AeroricProtocol
import Foundation
import Observation
import SwiftUI

/// E6 (2026-11-21): the unattended-work budget — the caps on the 用量与成本
/// tab, 暂停所有 Agent in the sidebar.
@MainActor @Observable
public final class BudgetBook {
    public struct Calls: Sendable {
        public var status: @Sendable () async throws -> LiveBudget
        public var set: @Sendable (LiveBudget) async throws -> LiveBudget

        public init(status: @escaping @Sendable () async throws -> LiveBudget,
                    set: @escaping @Sendable (LiveBudget) async throws -> LiveBudget) {
            self.status = status; self.set = set
        }
    }

    public private(set) var budget = LiveBudget()
    public private(set) var loaded = false
    public var errorText: String?
    @ObservationIgnored private var calls: Calls?

    public init() {}

    public func attach(client: BudgetClient?) {
        guard let client else {
            attach(nil)
            return
        }
        attach(Calls(status: { try await client.status() }, set: { try await client.set($0) }))
    }

    public func attach(_ calls: Calls?) {
        self.calls = calls
        budget = LiveBudget()
        loaded = false
        errorText = nil
    }

    public func refresh() async {
        guard let calls else { return }
        do {
            budget = try await calls.status()
            loaded = true
        } catch {
            errorText = "读取用量上限失败：\(error)"
        }
    }

    @discardableResult
    private func save(_ next: LiveBudget) async -> Bool {
        guard let calls else { return false }
        do {
            budget = try await calls.set(next)
            errorText = nil
            return true
        } catch {
            errorText = "保存失败：\(error)"
            return false
        }
    }

    public func setPaused(_ paused: Bool) async {
        var next = budget
        next.paused = paused
        await save(next)
    }

    @discardableResult
    public func saveCaps(_ caps: [LiveUsageCap]) async -> Bool {
        var next = budget
        next.caps = caps
        return await save(next)
    }
}

/// The budget's words.
public enum BudgetText {
    public static func period(_ p: String) -> String { p == "month" ? "每月" : "每天" }

    /// "每天 200K tok · $5.00".
    public static func cap(_ cap: LiveUsageCap) -> String {
        var parts: [String] = []
        if let tokens = cap.tokens { parts.append("\(SessionStatsText.compact(UInt64(tokens))) tok") }
        if let cost = cap.costUsd { parts.append(TurnBillText.money(cost)) }
        return "\(period(cap.period)) " + (parts.isEmpty ? "未设" : parts.joined(separator: " · "))
    }

    /// "今天 120K tok · ≈$1.20 · 本月 2.1M tok · ≈$18.00".
    public static func spent(_ b: LiveBudget) -> String {
        "今天 \(SessionStatsText.compact(UInt64(b.todayTokens))) tok · ≈\(TurnBillText.money(b.todayCost))"
            + " · 本月 \(SessionStatsText.compact(UInt64(b.monthTokens))) tok · ≈\(TurnBillText.money(b.monthCost))"
    }

    /// What the gate means now.
    public static func gate(_ b: LiveBudget) -> String {
        switch b.gate {
        case "paused": "已暂停所有 Agent：正在跑的回合已停下，新消息不收"
        case "stop": b.reason ?? "到上限了：无人值守的工作先不开始"
        case "near": (b.reason ?? "快到上限了") + "，快到上限了"
        default: b.caps.isEmpty ? "没有设上限" : "在上限以内"
        }
    }

    public static let note = "只算无人值守的工作：定时任务、不是你发起的委派和审查。你自己发的消息不受上限限制。"
        + "只在开始新工作前检查，不在回合中途停下；到上限时每天（或每月）只提醒一次。"

    /// The sidebar switch's confirmation.
    public static let pauseQuestion = "暂停所有 Agent？"
    public static let pauseDetail = "正在跑的回合会立刻停下，之后发的消息不收，定时任务到点跳过。随时可以恢复。"
}

/// E6: the caps, on top of the 用量与成本 tab: what unattended work spent,
/// the gate's colour, adding and removing caps.
struct BudgetSection: View {
    let book: BudgetBook
    @State private var period = "day"
    @State private var tokens = ""
    @State private var cost = ""

    private var tone: Color {
        switch book.budget.gate {
        case "stop", "paused": AeroricColor.danger
        case "near": AeroricColor.warning
        default: AeroricColor.textSecondary
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            Text("无人值守的用量上限").font(.system(size: AeroricType.secondary, weight: .semibold))
            Text(BudgetText.gate(book.budget))
                .font(.system(size: AeroricType.caption, weight: .medium))
                .foregroundStyle(tone)
                .accessibilityIdentifier("agent-config.budget.gate")
            Text(BudgetText.spent(book.budget))
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
            ForEach(Array(book.budget.caps.enumerated()), id: \.offset) { index, cap in
                HStack {
                    Text(BudgetText.cap(cap)).font(.system(size: AeroricType.caption))
                    Spacer()
                    Button("删除") {
                        var caps = book.budget.caps
                        caps.remove(at: index)
                        Task { await book.saveCaps(caps) }
                    }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .fixedSize()
                }
            }
            HStack(spacing: AeroricSpace.s) {
                Picker("周期", selection: $period) {
                    Text("每天").tag("day")
                    Text("每月").tag("month")
                }
                .labelsHidden()
                .fixedSize()
                TextField("token 上限", text: $tokens).textFieldStyle(.roundedBorder).frame(width: 110)
                TextField("美元上限", text: $cost).textFieldStyle(.roundedBorder).frame(width: 90)
                Button("添加上限") {
                    let cap = LiveUsageCap(period: period, tokens: Int(tokens.trimmingCharacters(in: .whitespaces)),
                                           costUsd: Double(cost.trimmingCharacters(in: .whitespaces)))
                    Task {
                        if await book.saveCaps(book.budget.caps + [cap]) { tokens = ""; cost = "" }
                    }
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                .fixedSize()
                .disabled(Int(tokens) == nil && Double(cost) == nil)
                .accessibilityIdentifier("agent-config.budget.add")
            }
            Text(BudgetText.note)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .fixedSize(horizontal: false, vertical: true)
            if let error = book.errorText {
                Text(error).font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.danger)
            }
        }
        .padding(AeroricSpace.l)
        .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.l))
        .task { await book.refresh() }
    }
}

/// E6: 暂停所有 Agent in the sidebar footer. Pausing asks first; resuming
/// does not.
struct PauseAllButton: View {
    let book: BudgetBook
    @State private var asking = false

    var body: some View {
        Group {
            if book.budget.paused {
                Button { Task { await book.setPaused(false) } } label: {
                    Label("已暂停 · 恢复", systemImage: "play.circle")
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                .help("恢复所有 Agent")
            } else {
                Button { asking = true } label: {
                    Image(systemName: "pause.circle")
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                .help("暂停所有 Agent")
            }
        }
        .fixedSize()
        .accessibilityIdentifier("sidebar.pause-all")
        .confirmationDialog(BudgetText.pauseQuestion, isPresented: $asking) {
            Button("暂停", role: .destructive) { Task { await book.setPaused(true) } }
            Button("取消", role: .cancel) {}
        } message: {
            Text(BudgetText.pauseDetail)
        }
    }
}
