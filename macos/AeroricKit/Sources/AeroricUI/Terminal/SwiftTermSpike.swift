import Foundation
import SwiftTerm

/// P5-01 Action 1 能力 spike（编译期契约，永久保留）：本文件引用
/// `SwiftTerm` 1.2.5（Package.swift 钉 exact 版本）的真实 API 面——
/// coordinator 将要用到的每个符号都在这里出现一次。上游小版本若改动
/// delegate 签名或 feed/resize 形状，本文件编译失败，而不是
/// `TerminalCoordinator` 在运行期静默失联。签名摘自
/// `.build/checkouts/SwiftTerm/Sources/SwiftTerm`（1.2.5 实测，非记忆）：
/// delegate 十个要求方法（`TerminalViewDelegate.swift`），
/// `TerminalView(frame:font:)`（`Mac/MacTerminalView.swift`），
/// `getTerminal()`（`Apple/AppleTerminalView.swift`），
/// `Terminal.feed(byteArray:)/feed(text:)/feed(buffer:)`、
/// `Terminal.resize(cols:rows:)`、`Terminal.resetToInitialState()`。
enum SwiftTermSpike {
    /// 编译期验证：delegate 协议可被我们的类型完整实现。
    @MainActor
    static func conformsToRealDelegate(_ any: Any) -> Bool {
        any is TerminalViewDelegate
    }

    /// 编译期验证：renderer 可从字节缓冲喂入并按列/行 resize。
    @MainActor
    static func feedAndResize(view: TerminalView, bytes: [UInt8], cols: Int, rows: Int) {
        view.getTerminal().feed(byteArray: bytes)
        view.getTerminal().resize(cols: cols, rows: rows)
    }

    /// 编译期验证：整屏复位存在（P5-01 Action 5 的重置视图用）。
    @MainActor
    static func resetRenderer(view: TerminalView) {
        view.getTerminal().resetToInitialState()
    }
}
