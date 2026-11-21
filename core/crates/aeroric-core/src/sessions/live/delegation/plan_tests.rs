use std::collections::HashMap;

use aeroric_protocol::artifacts::ArtifactRef;
use aeroric_protocol::checkpoint::ObligationState;
use aeroric_protocol::delegation::{AcceptanceResult, DelegationState, Workspace};
use aeroric_protocol::work_plan::{UnitState, WorkPlan, WorkUnit, MAX_RUNNING};

use super::{brief, inputs, schedule, unit_state, validate, Observed};

fn unit(id: &str, deps: &[&str], workspace: Workspace) -> WorkUnit {
    WorkUnit {
        id: id.into(),
        title: format!("单元{id}"),
        brief: format!("做 {id}"),
        workspace,
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        ..WorkUnit::default()
    }
}

fn plan(units: Vec<WorkUnit>) -> WorkPlan {
    WorkPlan {
        id: "plan-1".into(),
        parent_task_id: "t-parent".into(),
        title: "拆开做".into(),
        units,
        confirmed: true,
        created_ms: 1,
    }
}

fn started(mut u: WorkUnit) -> WorkUnit {
    u.delegation_id = Some(format!("del-{}", u.id));
    u
}

fn accepted(state: ObligationState, stale: bool) -> AcceptanceResult {
    AcceptanceResult {
        id: "a1".into(),
        text: "测试通过".into(),
        state,
        evidence: Vec::new(),
        checked_revision: None,
        stale,
    }
}

fn seen(state: DelegationState, acceptance: Vec<AcceptanceResult>) -> Observed {
    Observed {
        state,
        acceptance,
        outputs: Vec::new(),
    }
}

#[test]
fn a_plan_must_be_small_named_and_without_cycles() {
    let ro = Workspace::ReadOnly;
    assert!(validate(&plan(vec![]))
        .unwrap_err()
        .contains("没有工作单元"));
    let nine: Vec<WorkUnit> = (0..9).map(|i| unit(&i.to_string(), &[], ro)).collect();
    assert!(validate(&plan(nine)).unwrap_err().contains("最多 8 个"));
    assert!(
        validate(&plan(vec![unit("a", &[], ro), unit("a", &[], ro)]))
            .unwrap_err()
            .contains("重复")
    );
    assert!(validate(&plan(vec![unit("a", &["a"], ro)]))
        .unwrap_err()
        .contains("依赖它自己"));
    assert!(validate(&plan(vec![unit("a", &["zz"], ro)]))
        .unwrap_err()
        .contains("不在这个计划里"));
    let cycle = validate(&plan(vec![
        unit("a", &["c"], ro),
        unit("b", &["a"], ro),
        unit("c", &["b"], ro),
        unit("d", &[], ro),
    ]))
    .unwrap_err();
    assert!(cycle.starts_with("计划里有环："), "{cycle}");
    for name in ["「单元a」", "「单元b」", "「单元c」"] {
        assert!(cycle.contains(name), "{cycle}");
    }
    assert!(!cycle.contains("单元d"));
    assert!(validate(&plan(vec![unit("a", &[], ro), unit("b", &["a"], ro)])).is_ok());
}

/// Nothing starts before the user confirms; then the roots start, at most
/// three at a time, and their dependents wait saying for whom.
#[test]
fn roots_start_once_confirmed_and_within_the_limit() {
    let ro = Workspace::ReadOnly;
    let mut p = plan(vec![
        unit("a", &[], ro),
        unit("b", &[], ro),
        unit("c", &[], ro),
        unit("d", &[], ro),
        unit("e", &["a"], ro),
    ]);
    p.confirmed = false;
    let s = schedule(&p, &HashMap::new());
    assert!(s.start.is_empty());
    assert_eq!(s.of("a").unwrap().reason, "计划还没确认");
    assert_eq!(s.of("e").unwrap().state, UnitState::WaitingDeps);
    assert_eq!(s.of("e").unwrap().reason, "等「单元a」通过验收");

    p.confirmed = true;
    let s = schedule(&p, &HashMap::new());
    assert_eq!(s.start, ["a", "b", "c"]);
    assert_eq!(s.start.len(), MAX_RUNNING);
    assert!(s.of("d").unwrap().reason.contains("同时最多运行 3 个"));
}

/// One unit writes in the parent's directory at a time; read-only ones and
/// those in a copy run beside it.
#[test]
fn one_writer_in_the_directory_at_a_time() {
    let p = plan(vec![
        unit("w1", &[], Workspace::Shared),
        unit("w2", &[], Workspace::Shared),
        unit("copy", &[], Workspace::Copy),
    ]);
    let s = schedule(&p, &HashMap::new());
    assert_eq!(s.start, ["w1", "copy"]);
    assert_eq!(
        s.of("w2").unwrap().reason,
        "等「单元w1」写完：同一个目录同时只能有一个单元写文件"
    );

    // Already running: a writer started earlier holds the directory.
    let p = plan(vec![
        started(unit("w1", &[], Workspace::Shared)),
        unit("w2", &[], Workspace::Shared),
    ]);
    let running = HashMap::from([("w1".to_string(), seen(DelegationState::Running, vec![]))]);
    let s = schedule(&p, &running);
    assert!(s.start.is_empty());
    assert_eq!(s.of("w1").unwrap().state, UnitState::Running);
    // Once it reported and passed, the next writer goes.
    let done = HashMap::from([(
        "w1".to_string(),
        seen(
            DelegationState::Reported,
            vec![accepted(ObligationState::Passed, false)],
        ),
    )]);
    assert_eq!(schedule(&p, &done).start, ["w2"]);
}

/// A prerequisite is met only when every criterion passed and none is
/// stale; a failed one keeps its dependents waiting, saying so.
#[test]
fn a_prerequisite_is_met_when_its_acceptance_passed() {
    let ro = Workspace::ReadOnly;
    let p = plan(vec![started(unit("a", &[], ro)), unit("b", &["a"], ro)]);
    let with = |o: Observed| schedule(&p, &HashMap::from([("a".to_string(), o)]));

    let s = with(seen(
        DelegationState::Reported,
        vec![accepted(ObligationState::Passed, false)],
    ));
    assert_eq!(s.start, ["b"]);
    assert_eq!(s.of("a").unwrap().state, UnitState::Passed);

    let s = with(seen(
        DelegationState::Reported,
        vec![accepted(ObligationState::Passed, true)],
    ));
    assert!(s.start.is_empty(), "stale is not passed");
    assert_eq!(s.of("a").unwrap().state, UnitState::AwaitingAcceptance);
    assert_eq!(s.of("b").unwrap().reason, "等「单元a」通过验收");

    let s = with(seen(
        DelegationState::Reported,
        vec![accepted(ObligationState::Failed, false)],
    ));
    assert_eq!(s.of("a").unwrap().state, UnitState::Failed);
    assert_eq!(s.of("b").unwrap().state, UnitState::WaitingDeps);
    assert_eq!(
        s.of("b").unwrap().reason,
        "前置「单元a」失败或停止了，等你处理"
    );

    let s = with(seen(DelegationState::Stopped, vec![]));
    assert_eq!(s.of("a").unwrap().state, UnitState::Cancelled);
    assert!(s.start.is_empty(), "never skipped");

    // No criteria: reporting is passing.
    assert_eq!(with(seen(DelegationState::Reported, vec![])).start, ["b"]);
    assert_eq!(
        unit_state(&seen(DelegationState::OverBudget, vec![])),
        UnitState::Failed
    );
    assert_eq!(
        unit_state(&seen(DelegationState::Checking, vec![])),
        UnitState::Running
    );
}

/// A unit's brief lists its prerequisites' outputs; when they change after
/// it started, its input is out of date.
#[test]
fn inputs_are_outputs_and_go_stale_when_they_change() {
    let ro = Workspace::ReadOnly;
    let out = |v: u32| ArtifactRef {
        artifact_id: "art-1".into(),
        version: v,
        path: "docs/plan.md".into(),
    };
    let mut b = unit("b", &["a"], ro);
    let p = plan(vec![started(unit("a", &[], ro)), b.clone()]);
    let mut a_done = seen(DelegationState::Reported, vec![]);
    a_done.outputs = vec![out(1)];
    let observed = HashMap::from([("a".to_string(), a_done.clone())]);
    let given = inputs(&p, &observed, &b);
    assert_eq!(given, [out(1)]);
    let text = brief(&p, &b, &given);
    assert!(text.starts_with("做 b"));
    assert!(text.contains("- docs/plan.md：art-1@1"), "{text}");
    assert!(brief(&p, &b, &[]).contains("前置单元 「单元a」 没有登记产物"));

    // Started with version 1; the prerequisite's output is now version 2.
    b = started(b);
    b.started_with = vec![out(1)];
    let p = plan(vec![started(unit("a", &[], ro)), b]);
    let mut changed = a_done;
    changed.outputs = vec![out(2)];
    let observed = HashMap::from([
        ("a".to_string(), changed),
        ("b".to_string(), seen(DelegationState::Running, vec![])),
    ]);
    let status = schedule(&p, &observed).of("b").unwrap().clone();
    assert!(status.inputs_stale);
    assert!(status.reason.ends_with("输入已过期"), "{}", status.reason);
}
