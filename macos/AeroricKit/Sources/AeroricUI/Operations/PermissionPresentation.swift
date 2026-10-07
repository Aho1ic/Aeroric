import Foundation

/// P8-04 刀五：系统权限面的**呈现策略**（纯函数，零系统调用）。
///
/// 这一层回答的是"界面该说什么"，不是"系统到底给了什么"（后者是
/// [`SystemPermissionProbe`] 的实现）。分开的理由是本任务最要紧的一条约束：
/// **ad-hoc 重签之后界面不许显示成永久授权**——那是一个判定，不是一个探测结果，
/// 所以它必须能被普通断言钉住，而不是靠手动重签一遍去看。
///
/// 术语照抄宿主 `src-tauri/src/permissions/mod.rs` 与 README 的用户可见承诺：
/// - `unstableSignature`：授权绑当次构建的 cdhash，升级/重签后系统设置里开关还在，
///   但应用已经拿不到——这是"设置显示已开放、应用报未获取"的主因。
/// - `notBundled`：没跑在 app bundle 里（`tauri dev` 一类），授权记在终端/IDE 身上，
///   与本应用不共享。
public enum PermissionPresentation {

    /// 本平台是否有可枚举的权限清单。macOS 有；没有清单的平台（示例：未来的其它
    /// 平台实现）显示"本平台无需授权"，而不是一张空表。
    public static func supports(platform: String) -> Bool {
        platform == "macos"
    }

    /// 身份的呈现计划。
    ///
    /// `bundleIdentifier` 由调用方给（**当前 app 自己的**身份，不是宿主的）：native
    /// 客户端的值是 `com.aeroric.native.dev`，Tauri 是 `com.aeroric.desktop`。两边
    /// 混用会让用户按着"已授权"的提示去系统设置里找一个根本不存在的条目。
    public static func plan(
        platform: String,
        bundleIdentifier: String?,
        signature: String,
        stableAcrossUpdates: Bool,
        resettableServices: [String],
        reRequest: [String]
    ) -> PermissionPlan {
        let subject = bundleIdentifier ?? "(未以 app bundle 运行)"

        guard supports(platform: platform) else {
            return PermissionPlan(
                headline: "本平台没有可管理的系统授权",
                explanation: "\(platform) 上 Aeroric 不需要单独的系统权限。",
                reauthorize: nil,
                bundleIdentifier: subject
            )
        }

        guard let bundleIdentifier, !bundleIdentifier.isEmpty else {
            // `notBundled` 优先于签名不稳定：dev 跑法的授权根本不记在本应用名下，
            // 这时候再讲签名只会把用户引到错误的方向。
            return PermissionPlan(
                headline: "权限记在启动应用的终端/IDE 名下",
                explanation: "当前不是以应用包（.app）运行，系统把授权记在启动它的程序身上，"
                    + "与安装版不共享。要让系统设置里的开关真正生效，请运行打包后的应用。",
                reauthorize: nil,
                bundleIdentifier: subject
            )
        }

        guard stableAcrossUpdates else {
            return PermissionPlan(
                headline: "当前构建的授权升级后会失效",
                explanation: "这一次构建用的是临时签名（\(signature)）：系统把每一项授权都绑在"
                    + "当前这份构建的指纹上。界面显示「已授权」是系统设置里的旧记录，重新构建或升级后"
                    + "需要重新授权——本面板不会把这种状态显示成永久授权。",
                reauthorize: ReauthorizePlan(
                    summary: "清除这些服务的授权记录并重新请求。系统设置里对应的开关会先变灰，"
                        + "随后由本应用重新发起请求。",
                    services: resettableServices,
                    reRequest: reRequest,
                    bundleIdentifier: bundleIdentifier
                ),
                bundleIdentifier: bundleIdentifier
            )
        }

        return PermissionPlan(
            headline: "签名稳定（\(signature)）",
            explanation: "授权跨版本升级保留。若系统设置里显示已开启而这里显示未授权，"
                + "通常是被同一个主体名下的另一份构建覆盖了记录，可重新授权一次。",
            reauthorize: ReauthorizePlan(
                summary: "清除这些服务的授权记录并重新请求。",
                services: resettableServices,
                reRequest: reRequest,
                bundleIdentifier: bundleIdentifier
            ),
            bundleIdentifier: bundleIdentifier
        )
    }

    /// 一个权限项该显示成什么。
    ///
    /// **`unknown` 不算已授权**：探测不到（缺少签名身份、系统查询失败）与系统明确说
    /// "没给"是两件事，但都不能画成绿勾——那是本任务的反例清单里指名的一条。
    public static func statusLabel(_ status: PermissionStatus) -> String {
        status.label
    }

    /// Re-authorize 能不能做。
    ///
    /// 三条否决：不在 app bundle 里（没有记录可清）、没有可清的服务（点了没用）、
    /// 用户没确认（**任何系统级改动都不能在后台静默发生**）。
    public static func canReauthorize(_ plan: PermissionPlan?, confirmedByUser: Bool) -> Bool {
        guard confirmedByUser, let plan, let reauthorize = plan.reauthorize else { return false }
        return !reauthorize.services.isEmpty
    }

    /// 一个权限项要不要显示"请求"按钮。
    ///
    /// `reportOnly`（示例：Linux 的会话/用户组类能力）与"本平台不支持"都不给按钮——
    /// 点了没反应的按钮比没有按钮更糟。
    public static func showsRequestButton(
        reportOnly: Bool,
        canRequestInApp: Bool,
        canOpenSettings: Bool
    ) -> Bool {
        !reportOnly && (canRequestInApp || canOpenSettings)
    }

    /// 请求之后要不要提示重启。
    ///
    /// 事实：系统状态已给、进程内状态还没变（多数 TCC 只在进程启动时读一次），
    /// 这时重启应用立刻生效，比让用户反复点"请求"强。
    public static func needsRestart(
        systemStatus: PermissionStatus,
        processStatus: PermissionStatus
    ) -> Bool {
        systemStatus == .granted && processStatus == .notGranted
    }

    /// 权限面板要显示的服务名（`tccutil` 的服务名，用户看得懂的那种）。
    ///
    /// 这个映射与宿主 `permissions/macos.rs::tcc_service` 同表：两处都要给 `tccutil`
    /// 传同一个字符串，对不上的后果是"点了重新授权但什么都没清"。
    public static func tccService(for permissionID: String) -> String? {
        switch permissionID {
        case "screen-recording": "ScreenCapture"
        case "accessibility": "Accessibility"
        case "input-monitoring": "ListenEvent"
        case "automation": "AppleEvents"
        case "microphone": "Microphone"
        case "camera": "Camera"
        case "full-disk-access": "SystemPolicyAllFiles"
        case "folder-desktop": "SystemPolicyDesktopFolder"
        case "folder-documents": "SystemPolicyDocumentsFolder"
        case "folder-downloads": "SystemPolicyDownloadsFolder"
        default: nil
        }
    }

    /// 设置面板的锚点（`open x-apple.systempreferences:…` 的后半段）。
    /// 与宿主同表，理由同上：字符串写错就是"点了设置但跳到隐私页首页"。
    public static func settingsAnchor(for permissionID: String) -> String? {
        switch permissionID {
        case "screen-recording": "Privacy_ScreenCapture"
        case "accessibility": "Privacy_Accessibility"
        case "input-monitoring": "Privacy_ListenEvent"
        case "automation": "Privacy_Automation"
        case "microphone": "Privacy_Microphone"
        case "camera": "Privacy_Camera"
        case "full-disk-access": "Privacy_AllFiles"
        default: nil
        }
    }

    /// 从探测结果装配一份报告。**不做任何系统调用**，只做形状转换与筛序。
    public static func report(
        platform: String,
        probe: ProbeSnapshot
    ) -> PermissionReport {
        let entries = probe.permissions.map { row in
            PermissionReport.Entry(
                id: row.descriptor.id,
                status: row.status,
                detail: row.detail,
                canRequestInApp: row.descriptor.canRequestInApp,
                canOpenSettings: row.descriptor.canOpenSettings,
                canReset: row.descriptor.canReset
            )
        }
        let resettable = probe.permissions
            .filter { $0.descriptor.canReset && $0.status == .granted }
            .compactMap { tccService(for: $0.descriptor.id) }
        let reRequest = probe.permissions
            .filter { !$0.descriptor.reportOnly }
            .map(\.descriptor.id)

        let plan = plan(
            platform: platform,
            bundleIdentifier: probe.bundleIdentifier,
            signature: probe.signature,
            stableAcrossUpdates: probe.stableAcrossUpdates,
            resettableServices: resettable,
            reRequest: reRequest
        )
        return PermissionReport(
            platform: platform,
            permissions: entries,
            identity: PermissionReport.Identity(
                warning: probe.warning,
                bundleIdentifier: probe.bundleIdentifier ?? "",
                plan: plan
            )
        )
    }
}

// MARK: - 探测端口

/// 一个权限项的静态描述（宿主 `PermissionDescriptor` 的同形）。
public struct PermissionDescriptor: Sendable, Equatable {
    public var id: String
    public var canRequestInApp: Bool
    public var canOpenSettings: Bool
    public var canReset: Bool
    /// 只报告、无动作（没有"请求"也没有"打开设置"）。
    public var reportOnly: Bool

    public init(
        id: String, canRequestInApp: Bool = false, canOpenSettings: Bool = false,
        canReset: Bool = false, reportOnly: Bool = false
    ) {
        self.id = id
        self.canRequestInApp = canRequestInApp
        self.canOpenSettings = canOpenSettings
        self.canReset = canReset
        self.reportOnly = reportOnly
    }
}

/// 探测结果快照：身份 + 每项的状态。装配报告只需要这些。
public struct ProbeSnapshot: Sendable, Equatable {
    public struct Row: Sendable, Equatable {
        public var descriptor: PermissionDescriptor
        public var status: PermissionStatus
        public var detail: String?

        public init(descriptor: PermissionDescriptor, status: PermissionStatus, detail: String? = nil) {
            self.descriptor = descriptor
            self.status = status
            self.detail = detail
        }
    }

    public var bundleIdentifier: String?
    public var signature: String
    public var stableAcrossUpdates: Bool
    /// `unstableSignature` / `notBundled`；nil＝这次构建的授权是稳定的。
    public var warning: String?
    public var permissions: [Row]

    public init(
        bundleIdentifier: String?,
        signature: String,
        stableAcrossUpdates: Bool,
        warning: String?,
        permissions: [Row]
    ) {
        self.bundleIdentifier = bundleIdentifier
        self.signature = signature
        self.stableAcrossUpdates = stableAcrossUpdates
        self.warning = warning
        self.permissions = permissions
    }
}

/// 系统权限的**动作**端口。实现者在拥有当前 app 身份的那个进程里跑（GUI 适配器）；
/// 无头宿主没有身份，因此没有实现——那是"能力不存在"，不是"能力失败"。
///
/// 三个动作都必须由用户显式触发：`request` 会弹系统弹窗、`openSettings` 会跳走、
/// `reset` 会**删掉授权记录**。端口本身不做任何自动调用（这是计划 Action 3/4 里
/// "不能认为 daemon 的授权等于 app 的授权"的落地形态）。
public protocol SystemPermissionProbe: Sendable {
    /// 当前身份（bundle id、签名种类、是否跨升级稳定、告警）。
    func snapshot() -> ProbeSnapshot
    /// 在应用内请求某个权限（会弹系统弹窗）。
    func request(_ permissionID: String) -> PermissionStatus
    /// 打开系统设置里对应的面板。
    func openSettings(_ permissionID: String)
    /// 清除本应用在该服务上的授权记录（**只在用户确认之后调用**）。
    func reset(_ permissionID: String) throws
}

/// Re-authorize 的执行序（**纯函数产出计划**，由调用方按序执行）。
///
/// 顺序是刻意的：先 reset（否则系统认为"已授权"而不会再弹），再逐项 request
/// （用户得在系统弹窗里重新点一遍），最后才是"哪些要打开设置手工勾"的提示。
public enum ReauthorizeSequence {
    public struct Step: Sendable, Equatable {
        public enum Kind: String, Sendable {
            case reset
            case request
            case openSettings
        }
        public var kind: Kind
        public var permissionID: String
        /// `reset` 用的 `tccutil` 服务名（其余步骤为 nil）。
        public var service: String?
    }

    /// 产出执行序。`plan` 为 nil 或用户未确认时返回空——**没有确认就没有动作**。
    public static func steps(for plan: ReauthorizePlan?, confirmedByUser: Bool) -> [Step] {
        guard confirmedByUser, let plan else { return [] }
        let mapping = Dictionary(
            uniqueKeysWithValues: plan.reRequest.compactMap { id in
                PermissionPresentation.tccService(for: id).map { (id, $0) }
            }
        )
        var steps: [Step] = []
        for service in plan.services {
            // 只 reset 那些**真的属于这次会重新请求的项**的服务：多清一项等于多毁一项
            // 用户此前手工给出的授权。
            guard mapping.values.contains(service) else { continue }
            let id = mapping.first(where: { $0.value == service })?.key ?? service
            steps.append(Step(kind: .reset, permissionID: id, service: service))
        }
        for id in plan.reRequest {
            steps.append(Step(kind: .request, permissionID: id, service: nil))
        }
        return steps
    }
}
