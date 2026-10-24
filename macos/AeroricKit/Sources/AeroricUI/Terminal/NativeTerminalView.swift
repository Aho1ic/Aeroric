import SwiftUI
import SwiftTerm

/// P5-01 Action 2：`NSViewRepresentable` 只创建 renderer 并挂 coordinator——
/// **不用会自行 spawn 进程的 SwiftTerm 视图变体**，进程由宿主管理，这里
/// 只有一个渲染器视图。
public struct NativeTerminalView: NSViewRepresentable {
    public let coordinator: TerminalCoordinator

    public init(coordinator: TerminalCoordinator) {
        self.coordinator = coordinator
    }

    public func makeNSView(context: Context) -> TerminalView {
        let view = coordinator.renderer
        Self.applyFont(size: AeroricTypeScale.shared.terminalSize, to: view)
        return view
    }

    public func updateNSView(_ nsView: TerminalView, context: Context) {
        // 字节/输入全走 coordinator；这里只跟随终端字号档位（M3）。
        Self.applyFont(size: AeroricTypeScale.shared.terminalSize, to: nsView)
    }

    /// JetBrains Mono when the app bundle registered it, else the system mono.
    static func applyFont(size: CGFloat, to view: TerminalView) {
        guard view.font.pointSize != size else { return }
        view.font = NSFont(name: "JetBrainsMono-Regular", size: size)
            ?? NSFont.monospacedSystemFont(ofSize: size, weight: .regular)
    }
}
