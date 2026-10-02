import Foundation
import Observation

/// P5-05 Action 2：设置的**映射簿记**。每项设置映射到 core schema/command；
/// 分 UI-only（窗口尺寸等，UserDefaults）与 shared business（agent、proxy、
/// MCP，core 配置——**不用 `@AppStorage` 另存一份共享业务配置**）。本类型
/// 是纯模型＋纯决策（无 UI；wire 调用由调用方提交）。
@MainActor
@Observable
public final class SettingsStore {
    /// 设置面板（与 React `panelRegistry` 的 key 集对齐；G11：全部 19 个
    /// panel 必须有映射，platforms 声明保留——wsl 是 windows-only）。
    public struct Panel: Identifiable, Equatable, Sendable {
        public var id: String
        public var section: SettingsSection
        /// platforms 声明（nil＝全平台可见；G11 唯一 windows-only 是 wsl）。
        public var platforms: [String]?

        public init(id: String, section: SettingsSection, platforms: [String]? = nil) {
            self.id = id
            self.section = section
            self.platforms = platforms
        }
    }

    /// G11：panelRegistry 的 19 条 key 逐条映射（section 与 labelKey 分节
    /// 对齐；wsl 带 platforms: ["windows"]——台账 :263 的唯一平台门控）。
    public static let panelRegistry: [Panel] = [
        Panel(id: "general", section: .application),
        Panel(id: "theme", section: .application),
        Panel(id: "fonts", section: .application),
        Panel(id: "shortcuts", section: .application),
        Panel(id: "proxy", section: .application),
        Panel(id: "local-router", section: .application),
        Panel(id: "remote", section: .application),
        Panel(id: "mcp", section: .application),
        Panel(id: "notebook-embedding", section: .application),
        Panel(id: "cleanup-report", section: .application),
        Panel(id: "wsl", section: .application, platforms: ["windows"]),
        Panel(id: "permissions", section: .application),
        Panel(id: "all-agent-configs", section: .agents),
        Panel(id: "usage", section: .agents),
        Panel(id: "agent-updates", section: .agents),
        Panel(id: "hooks", section: .agents),
        Panel(id: "skills", section: .agents),
        Panel(id: "dsh-plugins", section: .agents),
        Panel(id: "about", section: .about),
    ]

    /// 当前平台（AppPlatform 字符串；"macos"/"windows"/"linux"）。
    public let platform: String

    public init(platform: String = "macos") {
        self.platform = platform
    }

    /// G11 验收：所有原 panel 有映射（React key 集 ↔ Swift registry 逐条
    /// 对齐；wsl 的 platforms 门控保留）。
    public func availablePanels() -> [Panel] {
        Self.panelRegistry.filter { panel in
            guard let platforms = panel.platforms else { return true }
            return platforms.contains(platform)
        }
    }

    /// 映射完整性守卫：React registry 的 key 集必须全部出现在 Swift
    /// panelRegistry（缺一条＝映射不完整）。
    public static func mapsAllReactPanels(_ reactKeys: [String]) -> Bool {
        let swiftKeys = Set(panelRegistry.map(\.id))
        return reactKeys.allSatisfy { swiftKeys.contains($0) }
    }

    // MARK: - Secret 策略（Action 3）

    /// Secret 只显示 configured/masked 状态；替换写入传一次后清内存；
    /// 不预填原 key，不进日志/快照/剪贴板/崩溃报告（本类型不持有明文）。
    public struct SecretState: Equatable, Sendable {
        public var configured: Bool
        /// masked 展示（如 "sk-…abc" 的尾 3 位）；未配置＝nil。
        public var maskedTail: String?

        public init(configured: Bool, maskedTail: String? = nil) {
            self.configured = configured
            self.maskedTail = maskedTail
        }
    }

    /// masked 展示的纯函数：只保留尾 `visibleTail` 位（默认 3），其余打码；
    /// 空串/短串全打码。**输出永远不还原原 key**。
    public static func maskSecret(_ secret: String, visibleTail: Int = 3) -> SecretState {
        guard !secret.isEmpty else { return SecretState(configured: false) }
        guard secret.count > visibleTail else {
            // 短串全打码（尾 3 位可见会泄露整个 secret）。
            return SecretState(configured: true, maskedTail: "…\u{2022}\u{2022}\u{2022}")
        }
        let tail = secret.suffix(visibleTail)
        return SecretState(configured: true, maskedTail: "…\(tail)")
    }

    /// 写入后的内存清理：返回值即「传给调用方一次的明文」，本类型不存。
    public static func secretForSingleWrite(_ secret: String) -> String {
        // 调用方拿到后立即发送；store 不持有（纯透传语义的显式化）。
        secret
    }

    // MARK: - UI-only vs shared business（Action 2 分界）

    /// 该设置项是否 UI-only（窗口尺寸等：UserDefaults 合法）；shared
    /// business（agent/proxy/MCP）必须走 core 配置。
    public static func isUIOnly(panelID: String) -> Bool {
        switch panelID {
        case "general", "theme", "fonts", "shortcuts":
            return true
        default:
            return false
        }
    }

    // MARK: - 保存守卫（验收：无效值保存失败不污染旧配置）

    /// 无效值守卫：保存前校验，失败返回原因（不写盘——不污染旧配置）。
    public static func validateSave(panelID: String, value: String) -> String? {
        switch panelID {
        case "proxy":
            // proxy URL 必须带 scheme。
            guard value.contains("://") else { return "proxy 需要带 scheme（如 http://…）" }
            return nil
        case "agent-updates":
            return nil
        default:
            return nil
        }
    }
}
