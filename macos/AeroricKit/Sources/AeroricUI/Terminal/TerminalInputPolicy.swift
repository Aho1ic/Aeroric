import Foundation

/// 输入策略（P5-01 Action 3/6）：纯决策，无副作用，单测全量覆盖。
/// 规约的边界在这里钉死：
/// * 输入必须经租约（隐藏窗口/无租约不得写 PTY——P3-03 lease 语义的 UI 面）；
/// * IME 组合中的按键不提交（Enter 先完成 composition，防误发）；
/// * 粘贴走 bracketed paste（renderer 侧包帧，策略只放行有租约的粘贴）；
/// * OSC 52 剪贴板写入**永不**自动执行（Action 6：默认不自动，用户触发才走，
///   且当前版本连用户触发也不执行——真实现推迟到有确认对话框的任务）；
/// * 终端里出现的 URL **永不**自动打开；用户点击时校验 scheme 白名单后才交
///   NSWorkspace，http/https 之外一律拒绝。
public enum TerminalInputPolicy {

    // MARK: - 输入放行

    /// 输入放行判定。`holdsLease` 来自 attach 时的 acquire_control 结果；
    /// `isHidden` 来自窗口可见性；`isComposing` 来自 NSTextInputClient 的
    /// marked-text 状态（renderer 转发）。
    public static func allowsInput(
        holdsLease: Bool,
        isHidden: Bool,
        isComposing: Bool
    ) -> Bool {
        holdsLease && !isHidden && !isComposing
    }

    // MARK: - 粘贴

    /// 粘贴放行：与输入同闸（租约+可见），且内容非空。bracketed paste 的
    /// 包帧由 renderer/上游完成，策略只决定「这一贴能不能进 PTY」。
    public static func allowsPaste(holdsLease: Bool, isHidden: Bool, content: String) -> Bool {
        !content.isEmpty && allowsInput(holdsLease: holdsLease, isHidden: isHidden, isComposing: false)
    }

    // MARK: - OSC 52 剪贴板

    /// OSC 52 请求的处置。**默认拒绝**（Action 6）：终端程序不能改用户剪贴板。
    /// 返回 (是否写入, 用户确认后是否放行)——当前阶段恒为 (false, false)，
    /// 确认对话框 UI 落地后第二位才有机会变 true。
    public static func osc52ClipboardRequest() -> (writesClipboard: Bool, userApproved: Bool) {
        (false, false)
    }

    // MARK: - 链接打开

    /// 用户点击终端链接时的 scheme 白名单校验（Action 6：用户触发＋scheme 校验）。
    /// file/ftp/javascript 等一律拒绝；只放行 http/https/mailto。
    public static func allowsOpeningLink(_ link: String) -> Bool {
        guard let url = URL(string: link), let scheme = url.scheme?.lowercased() else {
            return false
        }
        switch scheme {
        case "http", "https", "mailto":
            return true
        default:
            return false
        }
    }
}
