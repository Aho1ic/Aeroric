import SwiftUI

/// P5-02 Action 2：设置里的模式 Toggle。OFF＝纯原生 UI，ON＝高级命令行
/// （TUI）。文案钉死两条事实：切换不改变底层 Agent；模式是**呈现层**
/// 选择，不是任务协议转换。
public struct InteractionSettingsView: View {
    /// 与 controller 的绑定（显式 Binding 映射 enum——§2.4「不是两份布尔」）。
    @Binding private var mode: InteractionMode
    private let onSave: (InteractionMode) -> Void
    private let saveError: String?

    public init(
        mode: Binding<InteractionMode>,
        onSave: @escaping (InteractionMode) -> Void,
        saveError: String?
    ) {
        self._mode = mode
        self.onSave = onSave
        self.saveError = saveError
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Toggle("TUI / 高级命令行模式", isOn: Binding(
                get: { mode.toggleIsOn },
                set: { on in onSave(InteractionMode(toggleIsOn: on)) }
            ))
            .toggleStyle(.switch)
            .accessibilityIdentifier("settings.interaction-mode-toggle")

            VStack(alignment: .leading, spacing: 4) {
                Text(mode == .native ? "纯原生 UI" : "高级：终端与原生面板并存")
                    .font(.callout)
                    .accessibilityIdentifier("settings.interaction-mode-state")
                Text("切换只改变呈现层，不改变底层 Agent，也不会重启或取消运行中的任务。")
                    .font(.caption)
                    .foregroundStyle(Color.dsTextMuted)
            }
            if let saveError {
                Text("保存失败，已回滚：\(saveError)")
                    .font(.caption)
                    .foregroundStyle(Color.dsDanger)
                    .accessibilityIdentifier("settings.interaction-mode-error")
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("settings.interaction-mode")
    }
}
