//! 退出结算的唯一决策面(P3-03 Action 验收第二条)。
//!
//! 计划原文要的是:"SIGTERM/非零退出/EOF/正常退出/迟到 hook 均只产生一次结算,
//! **不能统一"有 session 就 done"覆盖现有家族差异**"。前半句由 ADR-030 的
//! `settle_marks_for_exit`(宿主侧唯一调用点在退出监控里)+ ADR-034 的迟到观察
//! 守卫负责;本模块负责后半句:**把家族差异从宿主管道里搬出来,变成一张可数的表**。
//!
//! 表的内容照迁移前宿主 `finalize_task_exit` 的判断逐条搬,行为一个字符都不改:
//! - `Dsh`/`Omp`:状态**完全由退出码驱动**。会话文件存在不代表任务成功——非零退出
//!   就是 failed,哪怕 transcript 已经写下去了。omp 的交互式 TUI 正常退出返回 0,
//!   所以非零一定是真崩溃,不该被"已写过会话文件"掩盖。这与 Claude 相反是刻意的。
//! - `Codex`:该代运行留下过会话条目 ⇒ 成功。
//! - `Claude`:**占位条目不算**——lazy attach 注入的占位行不是"曾真正建立过会话",
//!   否则 Claude 异常退出会被误标 done。
//! - 取消 / 手动完成:**不产生状态**(标记已被消费,状态由那次操作自己负责)。
//!
//! reason 这里返回**枚举而不是字符串**:今天宿主说 `"Process exited with code 3"`,
//! core 的未入账路径([`crate::tasks::settled_by_exit`])说 `"the process exited with
//! code 3"`,两句都是用户可见文本。合并成一句是文案改动,得由拷问轮定,不在这里偷偷
//! 选一个——所以决策面先统一,**渲染仍各自保留**(`host_sentence` 逐字保住宿主那句)。

use aeroric_protocol::task::{ProtocolFamily, TaskStatus};

/// 该代运行留下的会话证据。**由宿主如实上报**,家族规则在这里才敢不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvidence {
    /// 没有任何会话条目。
    Absent,
    /// 只有占位条目(lazy attach),不算真正建立过会话。
    PlaceholderOnly,
    /// 真正存在过会话条目。
    Real,
}

/// 一次退出的全部事实。`exit_ok`/`exit_code` 的取值口径与 portable-pty 的
/// `ExitStatus` 一致:成功即 `exit_ok = true` 且 `exit_code = None`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitFacts {
    pub family: ProtocolFamily,
    pub exit_ok: bool,
    pub exit_code: Option<u32>,
    pub session: SessionEvidence,
    /// 用户取消过这一次运行。
    pub cancelled: bool,
    /// 用户手动标记完成过这一次运行。
    pub manually_completed: bool,
}

impl ExitFacts {
    /// 会话条目在这个家族里算不算"真的建立过会话"。家族差异只在这一处。
    pub fn session_counts_as_success_for_family(&self) -> bool {
        match self.family {
            // 退出码驱动,会话真实性一律不参与。
            ProtocolFamily::Dsh | ProtocolFamily::Omp => false,
            ProtocolFamily::Codex | ProtocolFamily::Claude => self.session == SessionEvidence::Real,
        }
    }
}

/// 非零退出的原因。**枚举不是字符串**:渲染口径目前两侧不同,见模块头。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// 有明确退出码。
    Code(u32),
    /// 非零但没有码(被信号终止等)。
    NonZeroWithoutCode,
}

impl ExitReason {
    /// 宿主管道今天用的文案(逐字保持,包括首字母大写)。
    pub fn host_sentence(&self) -> String {
        match self {
            ExitReason::Code(code) => format!("Process exited with code {code}"),
            ExitReason::NonZeroWithoutCode => "Process exited with non-zero status".to_string(),
        }
    }
}

/// 一次退出的结算结果。三个变体就是全部可能——`(Done, 带理由)` 这种自相矛盾的
/// 形状不该存在,所以调用方不需要为它写兜底分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitDecision {
    /// 取消/手动完成:标记已消费,**不发状态**。
    NoStatus,
    Done,
    Failed {
        reason: ExitReason,
    },
}

impl ExitDecision {
    /// 这条结算对应的状态词;`NoStatus` 没有状态。
    pub fn task_status(&self) -> Option<TaskStatus> {
        match self {
            ExitDecision::Done => Some(TaskStatus::Done),
            ExitDecision::Failed { .. } => Some(TaskStatus::Failed),
            ExitDecision::NoStatus => None,
        }
    }
}

/// 唯一入口:事实进,结算决定出。
pub fn settle_exit(facts: &ExitFacts) -> ExitDecision {
    if facts.cancelled || facts.manually_completed {
        return ExitDecision::NoStatus;
    }
    if facts.exit_ok || facts.session_counts_as_success_for_family() {
        return ExitDecision::Done;
    }
    let reason = match facts.exit_code {
        Some(code) if code != 0 => ExitReason::Code(code),
        // `exit_ok == false` 却没有码(或码是 0 这对矛盾输入):被信号杀,或平台给不出码。
        _ => ExitReason::NonZeroWithoutCode,
    };
    ExitDecision::Failed { reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(family: ProtocolFamily) -> ExitFacts {
        ExitFacts {
            family,
            exit_ok: true,
            exit_code: None,
            session: SessionEvidence::Absent,
            cancelled: false,
            manually_completed: false,
        }
    }

    fn crashed(family: ProtocolFamily, code: Option<u32>, session: SessionEvidence) -> ExitFacts {
        ExitFacts {
            family,
            exit_ok: false,
            exit_code: code,
            session,
            cancelled: false,
            manually_completed: false,
        }
    }

    fn reason_of(settlement: ExitDecision) -> ExitReason {
        match settlement {
            ExitDecision::Failed { reason } => reason,
            other => panic!("期望 failed,实际 {other:?}"),
        }
    }

    #[test]
    fn a_clean_exit_is_done_for_every_family() {
        for family in ProtocolFamily::ALL {
            assert_eq!(
                settle_exit(&facts(*family)),
                ExitDecision::Done,
                "{family:?} 正常退出必须是 done"
            );
        }
    }

    #[test]
    fn dsh_and_omp_are_driven_by_the_exit_code_alone() {
        // 验收点名的反例:不许把"有 session 就 done"套到这两个家族上。
        for family in [ProtocolFamily::Dsh, ProtocolFamily::Omp] {
            let crashed_with_transcript = crashed(family, Some(137), SessionEvidence::Real);
            let settlement = settle_exit(&crashed_with_transcript);
            assert_eq!(
                settlement.task_status(),
                Some(TaskStatus::Failed),
                "{family:?} 非零退出必须是 failed,哪怕 transcript 已写入"
            );
            assert_eq!(reason_of(settlement), ExitReason::Code(137));
            assert!(
                !crashed_with_transcript.session_counts_as_success_for_family(),
                "{family:?} 会话真实性不得参与这两个家族的成败判断"
            );
        }
    }

    #[test]
    fn codex_and_claude_can_be_rescued_by_a_real_session() {
        for family in [ProtocolFamily::Codex, ProtocolFamily::Claude] {
            assert_eq!(
                settle_exit(&crashed(family, Some(1), SessionEvidence::Real)),
                ExitDecision::Done,
                "{family:?} 真实会话存在过就按 done(迁移前的口径)"
            );
        }
    }

    #[test]
    fn a_claude_placeholder_entry_never_fakes_a_done() {
        // lazy attach 注入的占位条目不是真会话:异常退出必须 failed。
        let settlement = settle_exit(&crashed(
            ProtocolFamily::Claude,
            Some(2),
            SessionEvidence::PlaceholderOnly,
        ));
        assert_eq!(
            settlement.task_status(),
            Some(TaskStatus::Failed),
            "占位条目不得把崩溃洗成 done"
        );
        assert_eq!(reason_of(settlement), ExitReason::Code(2));
    }

    #[test]
    fn codex_placeholder_only_is_not_a_real_entry() {
        // Codex 今天没有占位条目这一说,但事实面若真报 PlaceholderOnly,
        // 也不能被算成"建立过会话"——否则等于给没见过的形状发明豁免。
        assert_eq!(
            settle_exit(&crashed(
                ProtocolFamily::Codex,
                Some(2),
                SessionEvidence::PlaceholderOnly
            ))
            .task_status(),
            Some(TaskStatus::Failed)
        );
    }

    #[test]
    fn cancel_and_manual_completion_settle_no_status_at_all() {
        for family in ProtocolFamily::ALL {
            for (cancelled, completed) in [(true, false), (false, true)] {
                let f = ExitFacts {
                    family: *family,
                    exit_ok: false,
                    exit_code: Some(143),
                    session: SessionEvidence::Real,
                    cancelled,
                    manually_completed: completed,
                };
                let settlement = settle_exit(&f);
                assert_eq!(
                    settlement,
                    ExitDecision::NoStatus,
                    "{family:?} 取消/手动完成的状态由那次操作自己负责,退出不得覆盖"
                );
                assert_eq!(settlement.task_status(), None);
            }
        }
    }

    #[test]
    fn a_signal_death_without_a_code_still_reports_failed() {
        let settlement = settle_exit(&crashed(
            ProtocolFamily::Claude,
            None,
            SessionEvidence::Absent,
        ));
        assert_eq!(settlement.task_status(), Some(TaskStatus::Failed));
        assert_eq!(reason_of(settlement), ExitReason::NonZeroWithoutCode);
        assert_eq!(
            reason_of(settlement).host_sentence(),
            "Process exited with non-zero status"
        );
    }

    #[test]
    fn host_wording_is_preserved_verbatim() {
        // 这两句是用户可见文本:搬迁决策面不等于搬迁文案,改动要过拷问轮。
        assert_eq!(
            ExitReason::Code(3).host_sentence(),
            "Process exited with code 3"
        );
        assert_eq!(
            ExitReason::NonZeroWithoutCode.host_sentence(),
            "Process exited with non-zero status"
        );
    }

    #[test]
    fn a_zero_code_without_success_flag_is_not_invented_as_done() {
        // 反例守卫:`exit_ok == false` 而 `code == Some(0)` 这对矛盾输入不该被悄悄
        // 解释成"其实成功了"。宿主今天不会这么发,但若发了就该看到 failed。
        let settlement = settle_exit(&crashed(
            ProtocolFamily::Dsh,
            Some(0),
            SessionEvidence::Absent,
        ));
        assert_eq!(settlement.task_status(), Some(TaskStatus::Failed));
        assert_eq!(reason_of(settlement), ExitReason::NonZeroWithoutCode);
    }

    #[test]
    fn every_family_pair_of_session_evidence_lands_on_a_defined_settlement() {
        // 表必须是全函数:4 家族 × 3 种会话证据 × {正常, 非零} 全部有确定去处,
        // 不允许任何一组事实落进"没定义"的分支。
        for family in ProtocolFamily::ALL {
            for session in [
                SessionEvidence::Absent,
                SessionEvidence::PlaceholderOnly,
                SessionEvidence::Real,
            ] {
                for exit_ok in [true, false] {
                    let f = ExitFacts {
                        family: *family,
                        exit_ok,
                        exit_code: if exit_ok { None } else { Some(7) },
                        session,
                        cancelled: false,
                        manually_completed: false,
                    };
                    let s = settle_exit(&f);
                    assert!(s.task_status().is_some(), "{family:?} 无标记时必须有结论");
                    assert_eq!(
                        matches!(s, ExitDecision::Done),
                        exit_ok || f.session_counts_as_success_for_family()
                    );
                }
            }
        }
    }
}
