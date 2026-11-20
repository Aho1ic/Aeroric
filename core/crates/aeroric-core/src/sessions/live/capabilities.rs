//! R1-3a (2026-11-03): the capability table — every family, place and
//! feature has an answer, so a new family will not compile until it is
//! filled in (Q-S11). Facts come from the drivers: Codex runs in an OS
//! sandbox, Claude and omp keep their own approvals; omp fixes its
//! permission at start; Claude takes a steer as its next turn. DSH runs
//! over ACP (2026-11-13): its own sandbox holds the tier, fixed at start,
//! and a write outside it asks first; local workspaces only.
//!
//! A4 (2026-11-17): a read-only session (`plan`) counts as read-only only
//! when the harness itself holds it there — Codex's and DSH's read-only
//! sandboxes, Claude's plan mode. omp gets `always-ask` for `plan`, which
//! leaves every write to the user's refusal, so it is not.
//!
//! H1 (2026-11-20): the surfaces through which each harness uses Aeroric
//! (`Feature::SURFACES`); a launch's own record is in `surfaces.rs`.

use aeroric_protocol::live_capability::{CapabilityEntry, Feature, Place, SupportState};
use aeroric_protocol::project::ProjectTarget;

use crate::tasks::ProtocolFamily;

fn yes(note: Option<&str>) -> (SupportState, Option<String>) {
    (SupportState::Yes, note.map(str::to_string))
}

fn no(why: &str) -> (SupportState, Option<String>) {
    (SupportState::No, Some(why.to_string()))
}

fn degraded(how: &str) -> (SupportState, Option<String>) {
    (SupportState::Degraded, Some(how.to_string()))
}

fn unknown(what: &str) -> (SupportState, Option<String>) {
    (SupportState::Unknown, Some(what.to_string()))
}

/// Who holds a permission tier to its word.
fn enforced_by(family: ProtocolFamily) -> &'static str {
    match family {
        ProtocolFamily::Codex => "沙盒强制",
        ProtocolFamily::Claude => "由 Claude 把关 · 无沙盒",
        ProtocolFamily::Omp => "由 omp 把关 · 无沙盒",
        ProtocolFamily::Dsh => "由 DSH 沙盒把关 · 越权写入先问你",
    }
}

pub fn support(family: ProtocolFamily, place: Place, feature: Feature) -> CapabilityEntry {
    use Feature as F;
    use ProtocolFamily as P;
    let enforced = enforced_by(family);
    let (state, note) = match (family, feature) {
        (P::Dsh, _) if place == Place::Remote => no("DSH 只在本机工作区上运行原生会话"),
        // C9 (2026-11-20): who is given the context server at start; the
        // executor's routes agree (tested below).
        (_, F::ContextTools) if place == Place::Remote => {
            no("远端会话还没有接入 Aeroric 工具：工具服务器要能连回这台 Mac 的宿主（批次 D）")
        }
        (P::Omp, F::ContextTools) => no(
            "OMP 还不能在启动时接入 Aeroric 工具：查不到记忆和 Wiki，也不能自己发起委派、读写黑板",
        ),
        (P::Codex, F::ContextTools) => yes(Some("启动时在 thread/start 里接入")),
        (P::Claude, F::ContextTools) => yes(Some("启动时用 --mcp-config 接入")),
        (P::Dsh, F::ContextTools) => yes(Some("经 ACP 的 session/new 接入")),
        // H1 (2026-11-20): the other surfaces. A launch's own record
        // (`surfaces.rs`) overrides the opening, tools and skills with what
        // really happened; the rest rest on these rows and their tests.
        (P::Codex, F::SurfaceInstructions) => yes(Some("thread/start 的 developerInstructions")),
        (P::Claude, F::SurfaceInstructions) => yes(Some("--append-system-prompt")),
        (P::Omp, F::SurfaceInstructions) => no(
            "还没接：OMP 有 --append-system-prompt，Aeroric 没传（H2）；交接简报放在第一条消息前，记忆档案不给",
        ),
        (P::Dsh, F::SurfaceInstructions) => no(
            "DSH 的 ACP 没有声明 systemPrompt，没有系统层入口；交接简报放在第一条消息前，记忆档案不给",
        ),
        (_, F::SurfaceSkills) if place == Place::Remote => no("Aeroric 不给远端会话挂技能"),
        (P::Codex, F::SurfaceSkills) => yes(Some("项目 .agents/skills 下带标记的副本")),
        (P::Claude, F::SurfaceSkills) => yes(Some("会话插件（--plugin-dir）或受管 home")),
        (P::Omp, F::SurfaceSkills) => yes(Some("受管 home")),
        (P::Dsh, F::SurfaceSkills) => yes(Some("受管配置目录")),
        (_, F::SurfaceEvents) if place == Place::Remote => no("远端会话没有接 Harness 自己的 Hook"),
        (P::Claude, F::SurfaceEvents) => {
            no("还没接：会话插件可以带 hooks/hooks.json，Aeroric 没放（H4）")
        }
        (P::Codex, F::SurfaceEvents) => no(
            "还没接：Hook 可以经 thread/start 的 config 按次下发，要同时给出信任哈希，未实测（H4）",
        ),
        (P::Omp, F::SurfaceEvents) => no("还没接：OMP 可以按次传 --hook，Aeroric 没传（H4）"),
        (P::Dsh, F::SurfaceEvents) => {
            no("DSH 没有能从外部配置的 Hook：它的扩展点是同进程插件")
        }
        (P::Claude, F::SurfaceSubagents) => {
            yes(Some("Task、Agent 工具和 task_* 事件显示为子代理"))
        }
        (P::Codex, F::SurfaceSubagents) => {
            yes(Some("subAgentActivity、collabAgentToolCall 显示为子代理"))
        }
        (P::Omp, F::SurfaceSubagents) => yes(Some("启动时订阅子代理事件")),
        (P::Dsh, F::SurfaceSubagents) => {
            no("DSH 的 ACP 不发子代理事件，只有消息、思考、工具调用和用量")
        }
        (_, F::SurfaceCli) => no("命令行入口（aeroric-cli ctx）还没做（H2）"),
        (P::Dsh, F::PermChange) => degraded("DSH 启动时固定权限：下次启动生效"),
        (P::Dsh, F::PlanMode) => degraded("用只读沙盒代替规划：写入要你批准"),
        (P::Dsh, F::Compact) => no("DSH 没有通过 ACP 提供压缩上下文"),
        (P::Dsh, F::Review) => no("DSH 没有内置审查，可用“审查”让另一族来看"),
        (P::Dsh, F::Steer) => degraded("DSH 会作为下一轮发出"),
        (P::Dsh, F::DelegateChild | F::Reviewer) => no("还没加进审查者和子会话的候选"),
        (P::Dsh, F::Effort | F::Receipts) => yes(None),
        (P::Dsh, F::StrictReadOnly) => yes(Some("DSH 只读沙盒强制 · 越权写入先问你")),
        (P::Codex, F::StrictReadOnly) => yes(Some("Codex 只读沙盒强制 · 越权写入先问你")),
        (P::Claude, F::StrictReadOnly) => yes(Some("Claude 的 plan 模式把关 · 无沙盒")),
        (P::Omp, F::StrictReadOnly) => {
            no("omp 没有能在启动时固定的只读档：只读会话会落到“每次都问”，写入只能靠你拒绝")
        }
        (_, F::PermAsk | F::PermAutoEdit | F::PermFullAccess) => yes(Some(enforced)),
        (P::Omp, F::PermChange) => degraded("omp 启动时固定权限：下次启动生效"),
        (P::Codex | P::Claude, F::PermChange) => yes(None),
        (P::Codex | P::Claude, F::PlanMode) => yes(None),
        (P::Omp, F::PlanMode) => unknown("omp 的只读规划还没核对"),
        (P::Codex | P::Claude | P::Omp, F::Effort) => yes(None),
        (P::Codex | P::Claude | P::Omp, F::Compact) => yes(None),
        (P::Codex, F::Review) => yes(None),
        (P::Claude, F::Review) => no("Claude 没有内置审查，可用“审查”让另一族来看"),
        (P::Omp, F::Review) => no("omp 没有内置审查，可用“审查”让另一族来看"),
        (_, F::Goal) => no("目标模式还没接通"),
        (P::Codex | P::Omp, F::Steer) => yes(None),
        (P::Claude, F::Steer) => degraded("Claude 会作为下一轮发出"),
        (P::Omp, F::DelegateChild | F::Reviewer) if place == Place::Local => degraded(
            "能做子会话和审查者，结果照常送回；但它没有 Aeroric 工具，读写不了共享黑板、查不到记忆",
        ),
        (P::Codex | P::Claude | P::Omp, F::DelegateChild | F::Reviewer) => match place {
            Place::Local => yes(None),
            Place::Remote => unknown("远程主机上还没核对"),
        },
        (P::Codex | P::Claude | P::Omp, F::Receipts) => yes(None),
    };
    CapabilityEntry {
        feature,
        state,
        note,
    }
}

/// Where a project's sessions run, for the table.
pub fn place_of(target: &ProjectTarget) -> Place {
    match target {
        ProjectTarget::Ssh { .. } => Place::Remote,
        ProjectTarget::Local { .. } | ProjectTarget::Wsl { .. } => Place::Local,
    }
}

/// A4: why a read-only (`plan`) session of `family` at `place` cannot be
/// held read-only, or `None` when the harness itself holds it. Discussion
/// members and read-only delegations are refused on `Some`.
pub fn read_only_refusal(family: ProtocolFamily, place: Place) -> Option<String> {
    let entry = support(family, place, Feature::StrictReadOnly);
    (entry.state != SupportState::Yes)
        .then(|| entry.note.unwrap_or_else(|| "还没核对它能不能只读".into()))
}

/// One family's whole table at `place`.
pub fn table(family: ProtocolFamily, place: Place) -> Vec<CapabilityEntry> {
    Feature::ALL
        .iter()
        .map(|f| support(family, place, *f))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_and_place_answers_every_feature() {
        for family in ProtocolFamily::ALL {
            for place in [Place::Local, Place::Remote] {
                let table = table(*family, place);
                assert_eq!(table.len(), Feature::ALL.len());
                for entry in &table {
                    if entry.state != SupportState::Yes {
                        assert!(
                            entry.note.is_some(),
                            "{family:?} {place:?} {entry:?} says why"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_drivers_facts_hold() {
        let at = |f, feature| support(f, Place::Local, feature);
        assert_eq!(
            at(ProtocolFamily::Codex, Feature::Review).state,
            SupportState::Yes
        );
        assert_eq!(
            at(ProtocolFamily::Claude, Feature::Review).state,
            SupportState::No
        );
        assert_eq!(
            at(ProtocolFamily::Claude, Feature::Steer).state,
            SupportState::Degraded
        );
        assert_eq!(
            at(ProtocolFamily::Omp, Feature::PermChange).state,
            SupportState::Degraded
        );
        assert_eq!(
            at(ProtocolFamily::Codex, Feature::PermAsk).note.as_deref(),
            Some("沙盒强制")
        );
        assert_eq!(
            support(ProtocolFamily::Codex, Place::Remote, Feature::DelegateChild).state,
            SupportState::Unknown
        );
        assert_eq!(
            at(ProtocolFamily::Dsh, Feature::Steer).state,
            SupportState::Degraded
        );
        assert_eq!(
            at(ProtocolFamily::Dsh, Feature::Receipts).state,
            SupportState::Yes
        );
        assert_eq!(
            support(ProtocolFamily::Dsh, Place::Remote, Feature::Receipts).state,
            SupportState::No
        );
        // C9: OMP has no Aeroric tools yet, so as a child it is degraded,
        // not refused; DSH has them over ACP.
        assert_eq!(
            at(ProtocolFamily::Omp, Feature::ContextTools).state,
            SupportState::No
        );
        assert_eq!(
            at(ProtocolFamily::Omp, Feature::DelegateChild).state,
            SupportState::Degraded
        );
        assert_eq!(
            at(ProtocolFamily::Dsh, Feature::ContextTools).state,
            SupportState::Yes
        );
        assert_eq!(
            support(ProtocolFamily::Claude, Place::Remote, Feature::ContextTools).state,
            SupportState::No
        );
    }

    /// A4: `Yes` only where the launch mapping for `plan` really hands the
    /// harness a read-only mode; these are the mappings the table rests on.
    #[test]
    fn strict_read_only_rests_on_the_plan_mappings() {
        use crate::sessions::live::{acp::dialect, claude::map as claude, codex::map as codex};
        assert_eq!(claude::claude_permission_mode("plan"), "plan");
        let policy = codex::codex_policy("plan");
        assert_eq!(policy.sandbox_mode, "read-only");
        assert_eq!(policy.sandbox_policy["type"], "readOnly");
        assert_eq!(
            dialect::DSH.pin("plan").unwrap().env,
            vec![("DSH_PERMISSION_MODE".to_string(), "read-only".to_string())]
        );
        // omp's `plan` is its ask-everything tier: nothing it holds itself.
        assert_eq!(
            crate::agents::launch::omp_permission_flag("plan"),
            Some("always-ask")
        );

        for family in [
            ProtocolFamily::Codex,
            ProtocolFamily::Claude,
            ProtocolFamily::Dsh,
        ] {
            assert_eq!(read_only_refusal(family, Place::Local), None, "{family:?}");
        }
        let omp = read_only_refusal(ProtocolFamily::Omp, Place::Local).unwrap();
        assert!(omp.contains("omp") && omp.contains("每次都问"), "{omp}");
        let remote = read_only_refusal(ProtocolFamily::Dsh, Place::Remote).unwrap();
        assert!(remote.contains("本机"), "{remote}");
        assert_eq!(
            read_only_refusal(ProtocolFamily::Codex, Place::Remote),
            None
        );
    }

    #[test]
    fn only_an_ssh_project_is_remote() {
        let ssh = ProjectTarget::Ssh {
            connection_id: "c1".into(),
            remote_path: "/srv/app".into(),
        };
        assert_eq!(place_of(&ssh), Place::Remote);
        let local = ProjectTarget::Local {
            path: "/tmp/p".into(),
        };
        assert_eq!(place_of(&local), Place::Local);
    }
}
