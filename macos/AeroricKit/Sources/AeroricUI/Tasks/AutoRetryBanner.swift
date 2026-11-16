import AeroricClient
import SwiftUI

/// 2026-11-14 无人值守自动重试: above an unattended session whose turn did
/// not run — when it is tried again, and 取消.
public struct AutoRetryBanner: View {
    let retry: AutoRetryInfo
    let onCancel: () async -> Void
    @State private var busy = false

    public init(retry: AutoRetryInfo, onCancel: @escaping () async -> Void) {
        self.retry = retry
        self.onCancel = onCancel
    }

    public var body: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "clock.arrow.circlepath")
                .foregroundStyle(AeroricColor.warning)
            Text(AutoRetryText.banner(retry))
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .lineLimit(2)
            Spacer(minLength: AeroricSpace.m)
            Button(busy ? "取消中…" : "取消") {
                busy = true
                Task {
                    await onCancel()
                    busy = false
                }
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .fixedSize()
            .disabled(busy)
            .help("不再自动重试；之后由你决定")
            .accessibilityIdentifier("session.autoRetry.cancel")
        }
        .padding(.horizontal, AeroricSpace.l)
        .padding(.vertical, AeroricSpace.s)
        .background(Capsule(style: .continuous).fill(AeroricColor.warning.opacity(0.1)))
        .padding(.horizontal, AeroricSpace.l)
        .padding(.top, AeroricSpace.s)
        .accessibilityIdentifier("session.autoRetry.banner")
    }
}

/// The words of 无人值守自动重试.
public enum AutoRetryText {
    public static func reason(_ reason: String) -> String {
        switch reason {
        case "quota": "额度用完"
        case "service": "服务出错"
        case "network": "网络不通"
        default: "出错"
        }
    }

    /// "14:30".
    public static func clock(_ ms: UInt64) -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm"
        return formatter.string(from: Date(timeIntervalSince1970: Double(ms) / 1000))
    }

    /// "上一轮因为网络不通没跑成，14:30 自动重试（第 2/3 次）".
    public static func banner(_ retry: AutoRetryInfo) -> String {
        let at = retry.dueMs.map { clock($0) + " " } ?? ""
        return "上一轮因为\(reason(retry.reason))没跑成，\(at)自动重试（第 \(retry.count + 1)/\(retry.max) 次）"
    }

    /// The 需要我处理 row's detail once the tries ran out.
    public static func gaveUp(_ retry: AutoRetryInfo) -> String {
        var text = "自动重试 \(retry.count) 次后仍因为\(reason(retry.reason))没跑成"
        if let error = retry.error, !error.isEmpty { text += "：\(error)" }
        return text
    }
}
