use super::*;

/// A throwaway directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-auto-check-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn shell(script: &str) -> CheckSpec {
    CheckSpec {
        kind: "tests",
        profile: "shell".into(),
        label: "Shell".into(),
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
    }
}

fn run(spec: &CheckSpec, timeout: Duration) -> AutoCheckItem {
    let dir = Scratch::new();
    run_check(spec, dir.path(), &[], &AtomicBool::new(false), timeout)
}

#[test]
fn an_unparsed_profile_is_judged_by_its_exit_status() {
    let passed = run(&shell("echo fine"), CHECK_TIMEOUT);
    assert_eq!(passed.state, "passed");
    assert_eq!(passed.output_tail, "fine\n");
    let failed = run(&shell("echo bad >&2; exit 3"), CHECK_TIMEOUT);
    assert_eq!(failed.state, "failed");
    assert_eq!(failed.output_tail, "bad\n");
}

#[test]
fn a_check_past_its_deadline_is_stopped_and_reported() {
    let started = Instant::now();
    let item = run(&shell("echo begun; sleep 30"), Duration::from_millis(300));
    assert_eq!(item.state, "timedOut");
    assert!(item.summary.contains("秒"), "{}", item.summary);
    assert!(item.output_tail.contains("begun"));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn cancelling_kills_the_whole_process_group() {
    let dir = Scratch::new();
    let marker = dir.path().join("survived");
    // The grandchild would write the marker if the group were not killed.
    let script = format!("(sleep 1; touch '{}') & wait", marker.display());
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::SeqCst);
    });
    let item = run_check(&shell(&script), dir.path(), &[], &cancel, CHECK_TIMEOUT);
    canceller.join().unwrap();
    assert_eq!(item.state, "cancelled");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!marker.exists(), "a grandchild outlived the cancel");
}

#[test]
fn a_missing_program_is_an_error_not_a_panic() {
    let mut spec = shell("");
    spec.program = "aeroric-no-such-program".into();
    let item = run(&spec, CHECK_TIMEOUT);
    assert_eq!(item.state, "error");
    assert!(item.summary.contains("aeroric-no-such-program"));
}

#[test]
fn output_and_failures_are_bounded() {
    let item = run(
        &shell("i=0; while [ $i -lt 2000 ]; do echo 0123456789; i=$((i+1)); done; exit 1"),
        CHECK_TIMEOUT,
    );
    assert_eq!(item.output_tail.chars().count(), OUTPUT_TAIL_CHARS);
    assert_eq!(
        head(&"字".repeat(400), MAX_FAILURE_CHARS).chars().count(),
        MAX_FAILURE_CHARS + 1
    );
    assert_eq!(tail("abcdef", 3), "def");
}

#[test]
fn failing_pytest_output_names_the_failed_tests() {
    let dir = Scratch::new();
    let spec = CheckSpec {
        kind: "tests",
        profile: "python".into(),
        label: "Pytest".into(),
        program: "sh".into(),
        args: vec![
            "-c".into(),
            "echo 'FAILED tests/test_calc.py::test_add - assert 3 == 4'; echo '1 failed, 1 passed in 0.01s'; exit 1".into(),
        ],
    };
    let item = run_check(
        &spec,
        dir.path(),
        &[],
        &AtomicBool::new(false),
        CHECK_TIMEOUT,
    );
    assert_eq!(item.state, "failed", "{item:?}");
    assert!(
        item.failures.iter().any(|f| f.contains("test_add")),
        "{item:?}"
    );
}

#[test]
fn diagnostics_fail_only_on_errors() {
    let dir = Scratch::new();
    let diagnostics = |script: &str| CheckSpec {
        kind: "diagnostics",
        profile: "tsc".into(),
        label: "tsc".into(),
        program: "sh".into(),
        args: vec!["-c".into(), script.into()],
    };
    let error = run_check(
        &diagnostics("echo \"src/a.ts(3,5): error TS2322: Type 'string' is not assignable to type 'number'.\"; exit 2"),
        dir.path(),
        &[],
        &AtomicBool::new(false),
        CHECK_TIMEOUT,
    );
    assert_eq!(error.state, "failed", "{error:?}");
    assert!(error.failures[0].contains("TS2322") || error.failures[0].contains("assignable"));
    let clean = run_check(
        &diagnostics("true"),
        dir.path(),
        &[],
        &AtomicBool::new(false),
        CHECK_TIMEOUT,
    );
    assert_eq!(clean.state, "passed");
}

#[test]
fn discovery_follows_the_selection_and_the_project_files() {
    let dir = Scratch::new();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let none = discover(dir.path(), AutoCheckSelection::default());
    assert!(none.is_empty());
    let diagnostics = discover(
        dir.path(),
        AutoCheckSelection {
            tests: false,
            diagnostics: true,
        },
    );
    assert_eq!(
        diagnostics
            .iter()
            .map(|s| (s.kind, s.profile.as_str()))
            .collect::<Vec<_>>(),
        vec![("diagnostics", "cargo")]
    );
    let both = discover(
        dir.path(),
        AutoCheckSelection {
            tests: true,
            diagnostics: true,
        },
    );
    assert_eq!(both.first().map(|s| s.kind), Some("diagnostics"));
    assert!(both
        .iter()
        .any(|s| s.kind == "tests" && s.profile == "cargo"));
    let empty = Scratch::new();
    assert!(discover(
        empty.path(),
        AutoCheckSelection {
            tests: true,
            diagnostics: true
        }
    )
    .is_empty());
}

#[test]
fn the_overall_state_is_the_worst_item() {
    let item = |state: &str| AutoCheckItem {
        kind: "tests".into(),
        profile: "p".into(),
        label: "P".into(),
        state: state.into(),
        summary: String::new(),
        failures: Vec::new(),
        output_tail: String::new(),
    };
    assert_eq!(overall(&[item("passed"), item("passed")]), "passed");
    assert_eq!(overall(&[item("passed"), item("timedOut")]), "failed");
    assert_eq!(overall(&[item("failed"), item("cancelled")]), "cancelled");
}
