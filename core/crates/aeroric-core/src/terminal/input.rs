//! 输入与 resize 的许可面(P3-03 Action 2/3)。
//!
//! 分工口径:**租约与"谁能改这条 PTY"的决定在这里,"有没有活跃 master、该不该把尺寸
//! 暂存到下一次运行"仍在宿主**——后者是窗口/进程事实,core 看不到,也不该猜。
//! 于是宿主的两条既有兜底被搬成一张可数的表,一条行为都没顺手改:
//!
//! - 宿主今天就在 `resize_task_pty` 里挡畸形尺寸(`cols < 2 || rows < 2 || > 10_000` →
//!   静默 `Ok(())`),理由是 FitAddon 在容器 `display:none` 时会算出退化尺寸,一旦打到
//!   PTY,SIGWINCH 会把全屏 TUI 排版打散到一字一行且**不可恢复**。这里把同一件事拆成
//!   两个具名理由:`ZeroExtent`(0×0/0 行——隐藏 view 的形状)与 `Malformed`(其余越界),
//!   对调用方仍是"忽略而不是报错",IPC 契约一字不改。
//! - 隐藏 view 不得持有控制权([`take_control`]):这才真正兑现"不能两个窗口同时改 PTY
//!   大小"与"隐藏终端无写权限"——前台切过去时旧窗口的手柄自动作废(被顶替者写入
//!   报 [`InputRejection::NotHolder`]),而隐藏窗口连取得租约都不行。
//!
//! 只读订阅者不受这道门影响:读流永远不需要租约(Action 3 的后半句)。

use super::lease::LeaseHandle;
use super::pty::TerminalService;
use super::stream::TerminalIdentity;

/// 与宿主今天同样的下限/上限:低于下限就是 FitAddon 那个退化尺寸。
pub const MIN_PTY_DIM: u16 = 2;
pub const MAX_PTY_DIM: u16 = 10_000;

/// 请求来自哪种视图。**只有前台视图有写权限。**
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewFocus {
    Foreground,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtySizeRequest {
    pub rows: u16,
    pub cols: u16,
}

/// 尺寸请求被忽略的理由。两者对调用方都是"忽略",但诊断与测试要能分开看。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeRejection {
    /// 隐藏视图发来的 resize——哪怕形状合法也不写。
    HiddenView,
    /// 0×0 或某一边为 0:容器没布局时的形状,写下去会把 TUI 打散。
    ZeroExtent,
    /// 其余非法形状(低于 [`MIN_PTY_DIM`] 或超过 [`MAX_PTY_DIM`])。
    Malformed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeDecision {
    Apply(PtySizeRequest),
    Ignored(ResizeRejection),
}

/// 唯一的尺寸裁决入口。规则顺序是有意义的:隐藏视图优先,它不该因为"形状合法"就拿到写权限。
pub fn decide_resize(request: PtySizeRequest, focus: ViewFocus) -> ResizeDecision {
    if focus == ViewFocus::Hidden {
        return ResizeDecision::Ignored(ResizeRejection::HiddenView);
    }
    if request.rows == 0 || request.cols == 0 {
        return ResizeDecision::Ignored(ResizeRejection::ZeroExtent);
    }
    if request.rows < MIN_PTY_DIM
        || request.cols < MIN_PTY_DIM
        || request.rows > MAX_PTY_DIM
        || request.cols > MAX_PTY_DIM
    {
        return ResizeDecision::Ignored(ResizeRejection::Malformed);
    }
    ResizeDecision::Apply(request)
}

/// 写入被拒的理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRejection {
    /// 这个视图不前台:连控制租约都不给它取得。
    HiddenView,
    /// 手柄不是当前持有者(被别的窗口顶替,或流上根本没有租约记录)。
    NotHolder,
}

/// 取得该流控制权的唯一入口:隐藏视图一律拒。
pub fn take_control(
    service: &TerminalService,
    identity: &TerminalIdentity,
    focus: ViewFocus,
) -> Result<LeaseHandle, InputRejection> {
    if focus == ViewFocus::Hidden {
        return Err(InputRejection::HiddenView);
    }
    Ok(service.acquire_control(identity))
}

/// 写操作(输入/resize 落盘之前)的统一闸门:先看视图是不是前台,再问租约。
pub fn check_write(
    service: &TerminalService,
    identity: &TerminalIdentity,
    handle: &LeaseHandle,
    focus: ViewFocus,
) -> Result<(), InputRejection> {
    if focus == ViewFocus::Hidden {
        return Err(InputRejection::HiddenView);
    }
    service
        .can_write(identity, handle)
        .map_err(|_| InputRejection::NotHolder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(rows: u16, cols: u16) -> PtySizeRequest {
        PtySizeRequest { rows, cols }
    }

    #[test]
    fn the_foreground_view_applies_ordinary_sizes() {
        assert_eq!(
            decide_resize(size(24, 80), ViewFocus::Foreground),
            ResizeDecision::Apply(size(24, 80))
        );
    }

    #[test]
    fn a_hidden_view_is_rejected_even_with_a_perfect_shape() {
        // 这条是"隐藏终端无写权限"的实质:不是因为尺寸退化才被挡。
        assert_eq!(
            decide_resize(size(24, 80), ViewFocus::Hidden),
            ResizeDecision::Ignored(ResizeRejection::HiddenView)
        );
        assert_eq!(
            decide_resize(size(0, 0), ViewFocus::Hidden),
            ResizeDecision::Ignored(ResizeRejection::HiddenView),
            "两个理由都成立时先报隐藏,别把'本来就不该写'说成'形状不对'"
        );
    }

    #[test]
    fn the_zero_zero_shape_has_its_own_reason() {
        assert_eq!(
            decide_resize(size(0, 0), ViewFocus::Foreground),
            ResizeDecision::Ignored(ResizeRejection::ZeroExtent)
        );
        assert_eq!(
            decide_resize(size(24, 0), ViewFocus::Foreground),
            ResizeDecision::Ignored(ResizeRejection::ZeroExtent)
        );
    }

    #[test]
    fn degenerate_and_oversized_shapes_are_malformed() {
        // 宿主今天那三条边界一字不变地搬过来。
        assert_eq!(
            decide_resize(size(24, 1), ViewFocus::Foreground),
            ResizeDecision::Ignored(ResizeRejection::Malformed)
        );
        assert_eq!(
            decide_resize(size(1, 80), ViewFocus::Foreground),
            ResizeDecision::Ignored(ResizeRejection::Malformed)
        );
        assert_eq!(
            decide_resize(size(10_001, 80), ViewFocus::Foreground),
            ResizeDecision::Ignored(ResizeRejection::Malformed)
        );
        assert_eq!(
            decide_resize(size(10_000, 10_000), ViewFocus::Foreground),
            ResizeDecision::Apply(size(10_000, 10_000)),
            "边界值本身是合法的,今天也是这么算的"
        );
    }

    #[test]
    fn every_shape_and_focus_lands_on_a_named_decision() {
        // 表必须是全函数:任何输入都有具名去处,不留"没定义"的分支。
        for focus in [ViewFocus::Foreground, ViewFocus::Hidden] {
            for rows in [0u16, 1, 2, 24, 10_000, 10_001] {
                for cols in [0u16, 1, 2, 80, 10_000, 10_001] {
                    let decision = decide_resize(size(rows, cols), focus);
                    match decision {
                        ResizeDecision::Apply(echo) => assert_eq!(echo, size(rows, cols)),
                        ResizeDecision::Ignored(_) => {}
                    }
                }
            }
        }
    }

    #[test]
    fn a_hidden_view_cannot_take_the_lease_at_all() {
        let service = TerminalService::new();
        let identity = TerminalIdentity::new("gate", 1);
        let holder = take_control(&service, &identity, ViewFocus::Foreground).expect("holder");
        assert_eq!(
            take_control(&service, &identity, ViewFocus::Hidden),
            Err(InputRejection::HiddenView)
        );
        // 被拒的那次动作**不得动到现有控制权**(写反成"先 acquire 再判 focus"时这里红)。
        assert_eq!(
            check_write(&service, &identity, &holder, ViewFocus::Foreground),
            Ok(())
        );
        assert!(take_control(&service, &identity, ViewFocus::Foreground).is_ok());
    }

    #[test]
    fn superseded_handles_and_empty_lease_records_are_both_not_holder() {
        let service = TerminalService::new();
        let identity = TerminalIdentity::new("gate", 1);
        let first = take_control(&service, &identity, ViewFocus::Foreground).expect("first");
        assert_eq!(
            check_write(&service, &identity, &first, ViewFocus::Foreground),
            Ok(())
        );
        // 另一个窗口顶替:旧手柄写入被拒。
        let second = take_control(&service, &identity, ViewFocus::Foreground).expect("second");
        assert_eq!(
            check_write(&service, &identity, &first, ViewFocus::Foreground),
            Err(InputRejection::NotHolder)
        );
        assert_eq!(
            check_write(&service, &identity, &second, ViewFocus::Foreground),
            Ok(())
        );
        // 流上没有任何租约记录时也必须回答 NotHolder,而不是"没人写过所以随便"。
        let stranger = TerminalIdentity::new("no-lease", 1);
        assert_eq!(
            check_write(&service, &stranger, &second, ViewFocus::Foreground),
            Err(InputRejection::NotHolder)
        );
        // 只读订阅不需要租约:发布之后随便谁都能读流。
        service.publish(&identity, b"\x1b[1mread-only ok\x1b[0m");
        let chunk = service
            .read_from(&identity, 0, 10)
            .expect("read without a lease");
        assert_eq!(chunk.bytes, b"\x1b[1mread-o");
    }

    #[test]
    fn a_hidden_writer_is_stopped_before_the_lease_is_even_consulted() {
        let service = TerminalService::new();
        let identity = TerminalIdentity::new("gate", 1);
        let holder = take_control(&service, &identity, ViewFocus::Foreground).expect("holder");
        // 同一个持有者,从隐藏视图发来 —— 拒因是 HiddenView 而不是 NotHolder。
        assert_eq!(
            check_write(&service, &identity, &holder, ViewFocus::Hidden),
            Err(InputRejection::HiddenView)
        );
    }
}
