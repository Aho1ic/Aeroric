//! 可取消的 watcher 生命周期令牌(P3-01 Action 3 的机制半边)。
//!
//! 规则来自计划:所有后台 watcher 接收可取消 token,结束时 join/回收,不能因视图
//! 释放而杀任务(杀的是 token 的 cancel,不是任务的进程)。令牌是一棵树:`child()`
//! 派生的子令牌随父链取消,这样"会话 watcher 属于某个 task generation"的停止条件
//! 可以挂在父令牌(运行时)与子令牌(该代)两级上,而检查方只需要 `is_cancelled()`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct TokenInner {
    cancelled: AtomicBool,
    /// 父令牌的内脏。`is_cancelled` 沿链上溯,所以 cancel 父令牌不需要通知任何子令牌。
    parent: Option<Arc<TokenInner>>,
}

impl TokenInner {
    fn is_cancelled(&self) -> bool {
        if self.cancelled.load(Ordering::Acquire) {
            return true;
        }
        match &self.parent {
            Some(parent) => parent.is_cancelled(),
            None => false,
        }
    }
}

/// 一棵取消树的节点。克隆共享同一个节点;`child()` 派生挂在自己下面的新节点。
#[derive(Clone)]
pub struct ShutdownToken {
    inner: Arc<TokenInner>,
}

impl Default for ShutdownToken {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TokenInner {
                cancelled: AtomicBool::new(false),
                parent: None,
            }),
        }
    }

    /// 派生一个随自己取消的子令牌。子令牌自己也能独立取消,不影响父与兄弟。
    pub fn child(&self) -> ShutdownToken {
        ShutdownToken {
            inner: Arc::new(TokenInner {
                cancelled: AtomicBool::new(false),
                parent: Some(Arc::clone(&self.inner)),
            }),
        }
    }

    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_token_is_not_cancelled_and_cancellation_is_visible_to_clones() {
        let token = ShutdownToken::new();
        assert!(!token.is_cancelled());
        let clone = token.clone();
        token.cancel();
        assert!(clone.is_cancelled());
    }

    #[test]
    fn cancelling_a_parent_cancels_children_but_not_the_other_way_round() {
        let root = ShutdownToken::new();
        let child = root.child();
        let grandchild = child.child();

        child.cancel();
        assert!(grandchild.is_cancelled());
        assert!(!root.is_cancelled(), "子令牌的取消不得上溯到父");

        root.cancel();
        assert!(root.is_cancelled());
    }

    #[test]
    fn sibling_cancellation_is_isolated() {
        let root = ShutdownToken::new();
        let a = root.child();
        let b = root.child();
        a.cancel();
        assert!(!b.is_cancelled());
        assert!(!root.is_cancelled());
    }

    #[test]
    fn a_deep_chain_cancels_without_any_registration() {
        // 上溯实现不维护任何" children 登记表",所以任意深度都成立,也没有
        // "cancel 先于 child() 派生"的顺序陷阱:先取消再派生,派生出的照样是取消态。
        let root = ShutdownToken::new();
        root.cancel();
        let late = root.child().child().child();
        assert!(late.is_cancelled());
    }
}
