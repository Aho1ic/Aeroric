//! 终端控制租约(P3-03 Action 3):一条流同时只有一个"输入/resize 控制者"。
//!
//! 规则来自计划:控制权的取得与释放**显式**;被顶替的旧租约再写操作返回错误;
//! 只读订阅者(回放/查看)不需要租约。前台窗口持有租约,隐藏窗口拿不到——
//! 这就是"隐藏的终端不能写 PTY/不能改大小"的机制形状。
//!
//! 实现口径:票据**全局单调递增且永不复用**。释放只是把控制权置空,下一次
//! acquire 拿到的是新票据——所以"释放后重新取得"绝不会让调用方手里那把旧
//! handle 复活,这比用一个会归零的计数器更难写错。

/// 一份控制租约。持有期间可写输入、可 resize;`release` 后或被顶替后,
/// 再用同一 handle 写操作返回 [`LeaseError::Superseded`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LeaseHandle {
    ticket: u64,
}

impl LeaseHandle {
    /// 票据值。协议面（ADR-038）只传这个数，租约本体不出 core。
    pub fn ticket(&self) -> u64 {
        self.ticket
    }

    /// 从协议传回的票据重建 handle：`release_control` 以票据语义裁决，
    /// 重建出的 handle 与当年发出的是同一个值，顶替/释放判定不变。
    pub fn from_ticket(ticket: u64) -> Self {
        Self { ticket }
    }
}

#[derive(Debug, Default)]
pub struct ControlLease {
    /// 已发出的最大票据;只增不减,保证旧 handle 不可能与新票据同号。
    next_ticket: u64,
    /// 当前有效票据;`None` = 无人持有。
    current: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LeaseError {
    /// 本 handle 已被新的 acquire 顶替(或已显式释放,或租约已随运行结束清零)。
    Superseded,
}

impl ControlLease {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取得控制权,顶替任何现有持有者(前台窗口优先——这正是"顶替"的语义)。
    pub fn acquire(&mut self) -> LeaseHandle {
        self.next_ticket += 1;
        let handle = LeaseHandle {
            ticket: self.next_ticket,
        };
        self.current = Some(handle.ticket);
        handle
    }

    /// 显式释放。只有当前持有者的释放有效;过期的 handle 释放是 no-op,
    /// 不得把别人的控制权关掉。
    pub fn release(&mut self, handle: &LeaseHandle) -> Result<(), LeaseError> {
        self.check(handle)?;
        self.current = None;
        Ok(())
    }

    /// 当前持有者的票据号;`0` = 无人持有(票据从 1 开始发)。
    pub fn holder_ticket(&self) -> u64 {
        self.current.unwrap_or(0)
    }

    /// 是否有人持有控制权(用于"无人控制时隐藏窗口不得接手"之类的判断)。
    pub fn is_held(&self) -> bool {
        self.current.is_some()
    }

    fn check(&self, handle: &LeaseHandle) -> Result<(), LeaseError> {
        if self.current == Some(handle.ticket) {
            Ok(())
        } else {
            Err(LeaseError::Superseded)
        }
    }

    /// 写操作(输入/resize)的统一闸门:控制者写之前先过这道闸。
    pub fn write_gate(&self, handle: &LeaseHandle) -> Result<(), LeaseError> {
        self.check(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_holder_can_write_and_a_release_frees_the_lease() {
        let mut lease = ControlLease::new();
        let handle = lease.acquire();
        assert!(lease.write_gate(&handle).is_ok());
        assert!(lease.is_held());
        lease.release(&handle).expect("release");
        assert!(
            lease.write_gate(&handle).is_err(),
            "释放后的 handle 不得再写"
        );
        assert_eq!(lease.holder_ticket(), 0, "释放后无人持有");
        assert!(!lease.is_held());
    }

    #[test]
    fn a_superseded_lease_writes_are_rejected_even_before_release() {
        let mut lease = ControlLease::new();
        let first = lease.acquire();
        let second = lease.acquire(); // 前台顶替(比如旧窗口失焦/被隐藏)

        assert!(
            lease.write_gate(&first).is_err(),
            "被顶替的旧租约再写输入/resize 必须被拒——两个窗口同时改一个 PTY 是验收反例"
        );
        assert!(lease.write_gate(&second).is_ok());

        // 旧持有者连释放权都没有:释放是 no-op,不影响新持有者。
        assert_eq!(lease.release(&first), Err(LeaseError::Superseded));
        assert!(lease.write_gate(&second).is_ok());
    }

    #[test]
    fn releasing_then_reacquiring_cannot_revive_the_old_handle() {
        let mut lease = ControlLease::new();
        let a = lease.acquire();
        lease.release(&a).expect("release");
        let b = lease.acquire();
        assert_ne!(a, b, "新票据不可复用,旧 handle 不得凭票写");
        assert!(lease.write_gate(&b).is_ok());
        assert_eq!(
            lease.write_gate(&a),
            Err(LeaseError::Superseded),
            "释放→重新取得之后,手里那把旧 handle 绝不能因为号码撞上而复活"
        );
        // 旧 handle 也不能把新持有者的控制权释放掉。
        assert_eq!(lease.release(&a), Err(LeaseError::Superseded));
        assert!(lease.write_gate(&b).is_ok());
    }

    #[test]
    fn tickets_never_repeat_across_many_generation_switches() {
        let mut lease = ControlLease::new();
        let mut seen = Vec::new();
        for _ in 0..32 {
            let handle = lease.acquire();
            lease.release(&handle).expect("release");
            seen.push(handle.ticket);
        }
        let mut sorted = seen.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(seen.len(), sorted.len(), "票据一旦重复就是租约别名缺陷");
    }

    #[test]
    fn write_gate_fails_when_nobody_holds_the_lease() {
        let mut lease = ControlLease::new();
        let handle = lease.acquire();
        // 模拟"租约随运行结束整体清零"而 handle 还在调用方手里。
        lease.current = None;
        assert_eq!(lease.write_gate(&handle), Err(LeaseError::Superseded));
    }

    #[test]
    fn a_double_release_is_rejected_and_leaves_the_lease_empty() {
        let mut lease = ControlLease::new();
        let handle = lease.acquire();
        lease.release(&handle).expect("first release");
        assert_eq!(
            lease.release(&handle),
            Err(LeaseError::Superseded),
            "重复释放不得被当成成功,否则调用方以为自己关掉了一份还在别人手里的租约"
        );
        assert!(!lease.is_held());
    }
}
