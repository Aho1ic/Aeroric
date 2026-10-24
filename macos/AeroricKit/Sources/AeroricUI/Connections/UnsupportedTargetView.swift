import SwiftUI

/// P7-02 Action 2：UnsupportedTargetView（macOS 打开 WSL 项目时保持元数据、
/// 显示「需要 Windows 主机」，**禁止当作本地 path**——wsl_project_is_local
/// 判定由 core 承载）。
public struct UnsupportedTargetView: View {
    public let projectName: String
    public let targetFlavor: String
    public let reason: String
    public var onOpenSettings: (() -> Void)?

    public init(
        projectName: String, targetFlavor: String, reason: String,
        onOpenSettings: (() -> Void)? = nil
    ) {
        self.projectName = projectName
        self.targetFlavor = targetFlavor
        self.reason = reason
        self.onOpenSettings = onOpenSettings
    }

    public var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "desktopcomputer.and.arrow.down")
                .font(.system(size: 32))
                .foregroundStyle(AeroricColor.textTertiary)
            Text(projectName)
                .font(.headline)
                .accessibilityIdentifier("connections.target.name")
            Text("目标形态：\(targetFlavor)")
                .font(.caption)
                .foregroundStyle(AeroricColor.textSecondary)
                .accessibilityIdentifier("connections.target.flavor")
            Text(reason)
                .font(.caption)
                .foregroundStyle(AeroricColor.warning)
                .multilineTextAlignment(.center)
                .accessibilityIdentifier("connections.target.reason")
            if let onOpenSettings {
                Button("打开设置…") { onOpenSettings() }
                    .accessibilityIdentifier("connections.target.open-settings")
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityIdentifier("connections.unsupported-target")
    }
}
