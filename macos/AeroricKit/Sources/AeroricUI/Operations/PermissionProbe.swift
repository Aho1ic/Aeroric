import ApplicationServices
import AVFoundation
import CoreGraphics
import Foundation
import IOKit.hid
import Security

/// P8-04 刀五：macOS 侧的系统权限探针（**在拥有当前 app 身份的进程里跑**）。
///
/// 三条边界，逐条对计划 Action 3/4：
///
/// 1. **身份是"现在跑着的这个 app"**：bundle id 取 `Bundle.main`，签名种类取
///    `SecCodeCopySelf` 的 csflags。native 客户端的值是 `com.aeroric.native.dev`；
///    拿 Tauri 的 `com.aeroric.desktop` 去查会查到别人的记录（或什么也查不到）。
/// 2. **请求/打开设置/清记录全是显式动作**：这个类型不自动调任何一个。`reset` 只在
///    用户确认后由 [`ReauthorizeSequence`] 的执行者调用——计划验收明写"没有在后台
///    静默 `tccutil reset`"。
/// 3. **不猜状态**：查不出来的项返回 `.unknown` 而不是 `.granted`/`.notGranted`。
public struct MacOSPermissionProbe: SystemPermissionProbe {
    /// 探测哪些权限。默认六项与宿主 `permissions/macos.rs` 的前六项同表
    /// （文件夹类需要逐个目录读试探，宿主那边也是独立分支，暂不在 native 面里）。
    let descriptors: [PermissionDescriptor]

    public init(descriptors: [PermissionDescriptor] = MacOSPermissionProbe.defaultDescriptors) {
        self.descriptors = descriptors
    }

    public static let defaultDescriptors: [PermissionDescriptor] = [
        // 屏幕录制：能请求（CGRequestScreenCaptureAccess 会弹窗），能清记录。
        PermissionDescriptor(
            id: "screen-recording", canRequestInApp: true, canOpenSettings: true, canReset: true
        ),
        // 辅助功能：AXIsProcessTrustedWithOptions(prompt) 会弹窗。
        PermissionDescriptor(
            id: "accessibility", canRequestInApp: true, canOpenSettings: true, canReset: true
        ),
        // 输入监控：无弹窗式请求 API，但 IOHIDRequestAccess 会弹；清记录后**必须重启**
        // 才生效（IOKit 在进程启动时缓存）。
        PermissionDescriptor(
            id: "input-monitoring", canRequestInApp: true, canOpenSettings: true, canReset: true
        ),
        // 麦克风 / 摄像头：AVFoundation 的 requestAccess 是异步弹窗，统一走异步包装。
        PermissionDescriptor(
            id: "microphone", canRequestInApp: true, canOpenSettings: true, canReset: true
        ),
        PermissionDescriptor(
            id: "camera", canRequestInApp: true, canOpenSettings: true, canReset: true
        ),
        // 完全磁盘访问：只能手工在设置里勾（没有请求 API）。
        PermissionDescriptor(
            id: "full-disk-access", canOpenSettings: true, canReset: true
        ),
    ]

    // MARK: - 身份

    public func snapshot() -> ProbeSnapshot {
        let bundle = Bundle.main.bundleIdentifier
        let (signature, stable, team) = Self.signingFacts()
        let warning: String?
        if bundle == nil {
            // 没跑在 bundle 里：授权记在启动它的终端/IDE 名下。这条优先于签名不稳定。
            warning = "notBundled"
        } else if !stable {
            warning = "unstableSignature"
        } else {
            warning = nil
        }
        _ = team
        return ProbeSnapshot(
            bundleIdentifier: bundle,
            signature: signature,
            stableAcrossUpdates: stable,
            warning: warning,
            permissions: descriptors.map { descriptor in
                ProbeSnapshot.Row(
                    descriptor: descriptor,
                    status: status(of: descriptor.id),
                    detail: detail(of: descriptor.id)
                )
            }
        )
    }

    /// `(签名种类, 是否跨升级稳定, team id)`。
    ///
    /// 分类照抄宿主 `permissions/macos.rs::classify_signature`：ad-hoc 与 linker-signed
    /// 都**不稳定**（requirement 是一个裸 cdhash，重签即失效），有 team id 的是
    /// Developer ID。
    static func signingFacts() -> (String, Bool, String?) {
        var code: SecCode?
        guard SecCodeCopySelf([], &code) == errSecSuccess, let code else {
            return ("unsigned", false, nil)
        }
        var staticCode: SecStaticCode?
        guard SecCodeCopyStaticCode(code, [], &staticCode) == errSecSuccess, let staticCode else {
            return ("unsigned", false, nil)
        }
        var information: CFDictionary?
        let flagsMask = SecCSFlags(rawValue: kSecCSSigningInformation)
        guard SecCodeCopySigningInformation(staticCode, flagsMask, &information) == errSecSuccess,
              let info = information as? [String: Any]
        else {
            return ("unsigned", false, nil)
        }
        let flags = (info[kSecCodeInfoFlags as String] as? NSNumber)?.int64Value ?? 0
        let team = info[kSecCodeInfoTeamIdentifier as String] as? String
        return classify(flags: flags, team: team)
    }

    /// csflags → 签名种类。`CS_LINKER_SIGNED = 0x20000`、`CS_ADHOC = 0x2`。
    static func classify(flags: Int64, team: String?) -> (String, Bool, String?) {
        let linkerSigned: Int64 = 0x20000
        let adhoc: Int64 = 0x2
        if flags & linkerSigned != 0 {
            // 链接器自动加的签名：连 Info.plist 都没覆盖，requirement 就是一个裸 cdhash。
            return ("linker-signed", false, team)
        }
        if flags & adhoc != 0 {
            return ("adhoc", false, team)
        }
        if let team, !team.isEmpty {
            return ("developer-id", true, team)
        }
        return ("signed", true, team)
    }

    // MARK: - 状态

    func status(of permissionID: String) -> PermissionStatus {
        switch permissionID {
        case "screen-recording":
            return CGPreflightScreenCaptureAccess() ? .granted : .notGranted
        case "accessibility":
            return AXIsProcessTrusted() ? .granted : .notGranted
        case "input-monitoring":
            switch IOHIDCheckAccess(kIOHIDRequestTypeListenEvent) {
            case kIOHIDAccessTypeGranted: return .granted
            case kIOHIDAccessTypeDenied: return .notGranted
            default: return .unknown
            }
        case "microphone":
            return Self.avStatus(.audio)
        case "camera":
            return Self.avStatus(.video)
        case "full-disk-access":
            return Self.fullDiskAccessGranted() ? .granted : .notGranted
        default:
            // 没有探测实现 ⇒ 未知。**不猜**：猜错的方向总是"看起来已经给了"。
            return .unknown
        }
    }

    func detail(of permissionID: String) -> String? {
        switch permissionID {
        case "input-monitoring":
            // 没有弹窗式授权 API 的平台事实：清记录之后必须重启才生效。
            return "清记录后需要重启应用才生效"
        case "full-disk-access":
            return "只能在系统设置里手工勾选"
        default:
            return nil
        }
    }

    static func avStatus(_ media: AVMediaType) -> PermissionStatus {
        switch AVCaptureDevice.authorizationStatus(for: media) {
        case .authorized: return .granted
        case .denied, .restricted: return .notGranted
        default: return .unknown
        }
    }

    /// 完全磁盘访问没有查询 API；事实上的判据是"能不能读到 TCC 的库"。
    static func fullDiskAccessGranted() -> Bool {
        let probe = "/Library/Application Support/com.apple.TCC/TCC.db"
        guard let handle = FileHandle(forReadingAtPath: probe) else { return false }
        defer { try? handle.close() }
        // 打不开内容（权限）与根本不存在要分开：不存在时这是"没给"，不是崩溃。
        _ = try? handle.read(upToCount: 1)
        return true
    }

    // MARK: - 动作

    /// 应用内请求。返回值是**请求之后重新探测的结果**，不是"用户点了同意"的假设。
    public func request(_ permissionID: String) -> PermissionStatus {
        switch permissionID {
        case "screen-recording":
            _ = CGRequestScreenCaptureAccess()
        case "accessibility":
            // `kAXTrustedCheckOptionPrompt` 是 C 的全局 `CFStringRef`（Swift 侧是 `var`，
            // 因此在严格并发下要显式声明"读它就是读一个常量"）。键值本身是文档固定的
            // 字符串 "AXTrustedCheckOptionPrompt"。
            let options = ["AXTrustedCheckOptionPrompt": true] as CFDictionary
            _ = AXIsProcessTrustedWithOptions(options)
        case "input-monitoring":
            _ = IOHIDRequestAccess(kIOHIDRequestTypeListenEvent)
        case "microphone":
            Self.requestAV(.audio)
        case "camera":
            Self.requestAV(.video)
        default:
            // 没有应用内请求路径（完全磁盘访问属此列）：交回 unknown，由视图给
            // "打开设置"这一条路。
            return .unknown
        }
        return status(of: permissionID)
    }

    static func requestAV(_ media: AVMediaType) {
        // 异步弹窗：结果通过回调来，但界面上的"已授权"始终以**下一次探测**为准，
        // 所以这里不等回调（等它会让调用方卡在 UI 线程上）。
        AVCaptureDevice.requestAccess(for: media) { _ in }
    }

    public func openSettings(_ permissionID: String) {
        guard let anchor = PermissionPresentation.settingsAnchor(for: permissionID) else { return }
        let url = "x-apple.systempreferences:com.apple.preference.security?\(anchor)"
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/open")
        process.arguments = [url]
        try? process.run()
    }

    /// 清除本应用在该服务上的授权记录。
    ///
    /// 三条守卫，任何一条不满足就抛错而不是"尽力而为"：
    /// ① 必须知道 bundle id（没有它 `tccutil` 会清错对象或什么都不清）；
    /// ② 必须能把这个权限 id 映射到 TCC 服务名；
    /// ③ **调用方必须已经拿到用户确认**——这条由类型系统外的调用序保证
    ///    （`ReauthorizeSequence.steps(for:confirmedByUser:)` 返回空表即无动作）。
    public func reset(_ permissionID: String) throws {
        guard let bundle = Bundle.main.bundleIdentifier, !bundle.isEmpty else {
            throw PermissionProbeError.notBundled
        }
        guard let service = PermissionPresentation.tccService(for: permissionID) else {
            throw PermissionProbeError.unknownService(permissionID)
        }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/tccutil")
        process.arguments = ["reset", service, bundle]
        let pipe = Pipe()
        process.standardError = pipe
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            let message = String(
                data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8
            ) ?? ""
            throw PermissionProbeError.tccutilFailed(message.trimmingCharacters(in: .whitespacesAndNewlines))
        }
    }
}

public enum PermissionProbeError: Error, Equatable {
    /// 不在 app bundle 里：没有"本应用的记录"可清。
    case notBundled
    case unknownService(String)
    case tccutilFailed(String)
}
