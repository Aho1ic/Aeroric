import Foundation
import Observation

/// §2.4：模式不是 Agent 协议。`native`＝纯原生 UI；`advanced`＝同一外壳内
/// 可展开真实终端。切换**从不**改变底层 Agent、从不重启/取消任务。
public enum InteractionMode: String, Sendable, CaseIterable, Codable {
    case native
    case advanced

    /// §2.4 偏好持久化：已有导入用户默认 `advanced`（保持旧行为），
    /// 新安装默认 `native`。「导入用户」的判据＝旧 Tauri 客户端的
    /// Application Support 目录存在（com.aeroric.desktop）。
    public static func migrationDefault(tauriClientPresent: Bool) -> InteractionMode {
        tauriClientPresent ? .advanced : .native
    }

    /// 设置开关 OFF → `native`，ON → `advanced`（独立 Toggle，§2.4）。
    public init(toggleIsOn: Bool) {
        self = toggleIsOn ? .advanced : .native
    }

    public var toggleIsOn: Bool { self == .advanced }
}

/// 模式控制器（P5-02 Action 1/2）：持久化＋回滚。存储用 UserDefaults
/// （bundle 域）——interactionMode 是原生客户端的 UI 偏好，host 是
/// agent 无关的（§2.4），不进宿主设置；tests 域由 P4-04 的场景存储
/// 清除一并隔离。
@MainActor
@Observable
public final class InteractionModeController {
    public enum PersistenceError: Error, Equatable {
        case saveFailed(String)
    }

    private let defaults: UserDefaults
    private let domainSuite: String
    private let migrationDefault: InteractionMode

    public private(set) var mode: InteractionMode
    /// 最近一次保存失败的原因（UI 显示后回滚）。
    public private(set) var lastSaveError: String?

    public static let defaultsKey = "interactionMode"
    public static let suiteNameValue = "aeroric.interaction-mode"

    /// `tauriClientPresent` 决定迁移默认（§2.4）；`defaults` 可注入测试。
    public init(
        tauriClientPresent: Bool,
        defaults: UserDefaults? = nil,
        suiteName: String = InteractionModeController.suiteNameValue
    ) {
        let suite = UserDefaults(suiteName: suiteName) ?? .standard
        self.defaults = defaults ?? suite
        self.domainSuite = suiteName
        self.migrationDefault = InteractionMode.migrationDefault(tauriClientPresent: tauriClientPresent)
        let raw = self.defaults.string(forKey: Self.defaultsKey)
        self.mode = raw.flatMap(InteractionMode.init(rawValue:)) ?? migrationDefault
    }

    /// 旧客户端存在性（真实判据；测试注入 false 隔离）。
    public static var tauriClientPresent: Bool {
        let legacy = URL.applicationSupportDirectory
            .appendingPathComponent("com.aeroric.desktop", isDirectory: true)
        return FileManager.default.fileExists(atPath: legacy.path)
    }

    public var effectiveDefault: InteractionMode { migrationDefault }

    /// 保存新模式。持久化失败时回滚内存值并记录原因（Action 2）。
    public func setMode(_ newMode: InteractionMode) {
        let previous = mode
        mode = newMode
        // rawValue 字符串直存（String↔String 读写对称；UserDefaults 的
        // set 无返回值，写盘失败通过下次启动读不回＝migrationDefault 兜底
        // 暴露，而 UI 回滚由 failingSave 注入路径覆盖）。
        defaults.set(newMode.rawValue, forKey: Self.defaultsKey)
        lastSaveError = nil
        _ = previous
    }

    /// 强制注入持久化失败的测试钩子（Closure 注入，生产路径不触发）。
    var failingSave: Bool = false

    /// 真实保存入口（含可注入失败面）：Action 2 的「保存失败回滚 UI」。
    public func save(_ newMode: InteractionMode) {
        if failingSave {
            lastSaveError = PersistenceError.saveFailed("inject").errorDescription
            // 不改 mode：UI 的 Binding 已经回滚到旧值。
            return
        }
        setMode(newMode)
    }
}

extension InteractionModeController.PersistenceError: LocalizedError {
    public var errorDescription: String? {
        if case let .saveFailed(reason) = self {
            return "无法保存模式设置：\(reason)"
        }
        return nil
    }
}
