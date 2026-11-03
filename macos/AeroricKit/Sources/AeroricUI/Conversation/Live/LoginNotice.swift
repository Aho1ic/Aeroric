import AeroricProtocol
import AppKit
import SwiftUI

/// R1-3a: the login the agent refused, above the composer (Q-S11).
struct LoginBar: View {
    let login: ExpiredLogin
    let help: () -> Void

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            Image(systemName: "key.slash")
            Text("登录已失效：\(login.label)")
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: AeroricSpace.s)
            Button("去登录", action: help)
                .buttonStyle(.plain)
                .foregroundStyle(AeroricColor.accent)
                .fixedSize()
                .accessibilityIdentifier("session.login.help")
        }
        .font(.system(size: AeroricType.caption))
        .foregroundStyle(AeroricColor.danger)
        .padding(.horizontal, AeroricSpace.l)
        .frame(height: 30)
        .background(Capsule(style: .continuous).fill(AeroricColor.danger.opacity(0.12)))
        .help(login.reason)
        .accessibilityIdentifier("session.login.expired")
    }
}

/// R1-3a (Q6): how to sign in again. Aeroric shows what to run and opens the
/// place to do it; the credentials go from the person to the agent only.
public struct LoginHelpSheet: View {
    let login: ExpiredLogin
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openSettings) private var openSettings

    public init(login: ExpiredLogin) {
        self.login = login
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("\(login.label) 的登录已失效")
                .font(.system(size: AeroricType.body, weight: .semibold))
            Text("Agent 返回：\(login.reason)")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .lineLimit(2)
            if let command = login.command {
                Text(login.connectionId.map { "在远程主机（连接 \($0)）上运行下面的命令，按提示登录：" }
                    ?? "在终端里运行下面的命令，按提示登录：")
                    .font(.system(size: AeroricType.secondary))
                HStack(spacing: AeroricSpace.s) {
                    Text(command)
                        .font(.system(size: AeroricType.secondary, design: .monospaced))
                        .textSelection(.enabled)
                    Spacer(minLength: AeroricSpace.s)
                    Button("复制") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(command, forType: .string)
                    }
                    .accessibilityIdentifier("session.login.copy")
                }
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 34)
                .background(AeroricColor.layer2, in: Capsule(style: .continuous))
                if login.family == "claude" {
                    Text("进入 Claude 后输入 /login。")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary)
                }
                if login.connectionId == nil {
                    Button("打开终端") {
                        NSWorkspace.shared.open(URL(fileURLWithPath: "/System/Applications/Utilities/Terminal.app"))
                    }
                    .accessibilityIdentifier("session.login.terminal")
                }
            }
            if login.configId != nil {
                Text("这个 Agent 配置的密钥被拒绝了，到 设置 › Agents 里检查它。")
                    .font(.system(size: AeroricType.secondary))
                Button("打开设置") { openSettings() }
                    .accessibilityIdentifier("session.login.settings")
            }
            Text("登录成功、下一轮跑完后，这个提示会自动消失。")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
            HStack {
                Spacer()
                Button("好") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 440)
        .accessibilityIdentifier("session.login.sheet")
    }
}
