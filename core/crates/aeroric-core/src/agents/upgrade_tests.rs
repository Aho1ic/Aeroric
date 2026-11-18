use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tempdir() -> TempDir {
    let dir = std::env::temp_dir().join(format!("aeroric-upgrade-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

#[test]
fn versions_compare_numerically_and_prereleases_sort_first() {
    assert_eq!(compare_versions("0.162.0", "0.99.9"), Ordering::Greater);
    assert_eq!(compare_versions("2.1.310", "2.1.0"), Ordering::Greater);
    assert_eq!(compare_versions("v18.12", "18.12.0"), Ordering::Equal);
    assert_eq!(compare_versions("0.2.1-rc.2", "0.2.1"), Ordering::Less);
    assert_eq!(
        compare_versions("0.2.1-rc.3", "0.2.1-rc.2"),
        Ordering::Greater
    );
}

#[test]
fn status_follows_installed_minimum_and_latest() {
    use UpgradeStatus::*;
    let cases = [
        (None, Some("1.0.0"), None, NotInstalled),
        (Some("18.10.2"), Some("18.13.1"), Some("18.12.0"), Required),
        (
            Some("18.12.3"),
            Some("18.13.1"),
            Some("18.12.0"),
            UpdateAvailable,
        ),
        (Some("18.13.1"), Some("18.13.1"), Some("18.12.0"), UpToDate),
        (Some("18.14.0"), Some("18.13.1"), None, UpToDate),
        (Some("1.0.0"), None, None, Unknown),
    ];
    for (installed, latest, minimum, expected) in cases {
        assert_eq!(
            status(installed, latest, minimum),
            expected,
            "{installed:?} {latest:?}"
        );
    }
}

#[test]
fn reminders_respect_off_and_a_skipped_version_until_a_newer_one() {
    let on = ReminderPref::default();
    let off = ReminderPref {
        remind: false,
        skip_version: None,
    };
    let skipped = ReminderPref {
        remind: true,
        skip_version: Some("18.13.1".into()),
    };
    let available = UpgradeStatus::UpdateAvailable;
    assert!(should_remind(available, &on, Some("18.13.1")));
    assert!(!should_remind(available, &off, Some("18.13.1")));
    assert!(!should_remind(available, &skipped, Some("18.13.1")));
    assert!(should_remind(available, &skipped, Some("18.14.0")));
    assert!(
        should_remind(UpgradeStatus::Required, &off, Some("18.14.0")),
        "required always shows"
    );
    assert!(!should_remind(UpgradeStatus::UpToDate, &on, Some("1.0.0")));
}

fn fake_deps(
    installed: Arc<Mutex<HashMap<Harness, Option<String>>>>,
    script: &'static str,
) -> UpgradeDeps {
    let probe = Arc::clone(&installed);
    UpgradeDeps {
        latest: Arc::new(|package| {
            Box::pin(async move {
                if package == "@deepseek-ai/dsh" {
                    Err("offline".to_string())
                } else {
                    Ok("99.0.0".to_string())
                }
            })
        }),
        installed: Arc::new(move |harness| probe.lock().get(&harness).cloned().flatten()),
        channel: Arc::new(|_| "npm".to_string()),
        plan: Arc::new(move |harness| {
            Ok((harness != Harness::Dsh).then(|| {
                vec![AgentUpgradeStep {
                    channel: "npm".into(),
                    program: "/bin/sh".into(),
                    args: vec!["-c".into(), script.into()],
                }]
            }))
        }),
        install: Arc::new(|_| Err("no npm".to_string())),
        env: Arc::new(|_| Vec::new()),
        forget_versions: Arc::new(move || {
            installed
                .lock()
                .insert(Harness::Omp, Some("99.0.0".to_string()));
        }),
    }
}

fn wait_done(center: &UpgradeCenter, harness: Harness) -> HarnessView {
    for _ in 0..200 {
        let view = center
            .snapshot()
            .agents
            .into_iter()
            .find(|view| view.agent == harness.id())
            .unwrap();
        if view
            .job
            .as_ref()
            .is_some_and(|job| job.state != JobState::Running)
        {
            return view;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("job did not finish");
}

#[tokio::test]
async fn check_records_latest_and_errors_and_prefs_persist() {
    let dir = tempdir();
    let installed = Arc::new(Mutex::new(HashMap::from([
        (Harness::Omp, Some("18.12.3".to_string())),
        (Harness::Claude, Some("2.1.5".to_string())),
    ])));
    let center = UpgradeCenter::new(dir.path(), fake_deps(installed, "true"));
    center.check(false).await;
    let snapshot = center.snapshot();
    assert!(snapshot.checked_at.is_some());
    assert!(snapshot.check_error.as_deref().unwrap().contains("DSH"));
    let omp = snapshot.agents.iter().find(|a| a.agent == "omp").unwrap();
    assert_eq!(omp.status, UpgradeStatus::UpdateAvailable);
    assert!(omp.should_remind);

    center
        .set_reminder(Harness::Omp, ReminderMode::SkipVersion)
        .unwrap();
    let again = UpgradeCenter::new(dir.path(), fake_deps(Arc::default(), "true"));
    again.check(false).await;
    let omp = again
        .snapshot()
        .agents
        .into_iter()
        .find(|a| a.agent == "omp")
        .unwrap();
    assert_eq!(omp.skip_version.as_deref(), Some("99.0.0"));
    assert!(!omp.should_remind);
}

#[test]
fn an_upgrade_streams_output_and_reports_the_new_version() {
    let dir = tempdir();
    let installed = Arc::new(Mutex::new(HashMap::from([(
        Harness::Omp,
        Some("18.12.3".to_string()),
    )])));
    let center = UpgradeCenter::new(
        dir.path(),
        fake_deps(installed, "echo downloading; printf 'a\\rb 99%%\\n' >&2"),
    );
    center.start(Harness::Omp).unwrap();
    let view = wait_done(&center, Harness::Omp);
    let job = view.job.unwrap();
    assert_eq!(job.state, JobState::Succeeded, "{:?}", job.message);
    assert!(job.log.iter().any(|line| line == "downloading"));
    assert!(
        job.log.iter().any(|line| line == "b 99%"),
        "keeps the last progress frame"
    );
    assert_eq!(view.installed.as_deref(), Some("99.0.0"));
}

#[test]
fn a_failing_command_fails_with_its_last_line_and_missing_install_needs_npm() {
    let dir = tempdir();
    let installed = Arc::new(Mutex::new(HashMap::from([(
        Harness::Claude,
        Some("2.1.5".to_string()),
    )])));
    let center = UpgradeCenter::new(
        dir.path(),
        fake_deps(installed, "echo network timeout; exit 3"),
    );
    center.start(Harness::Claude).unwrap();
    let job = wait_done(&center, Harness::Claude).job.unwrap();
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.message.as_deref(), Some("network timeout"));
    assert_eq!(center.start(Harness::Dsh).unwrap_err(), "no npm");
}

/// A11: deps whose every harness runs `program -c script`, `script` writing
/// `in <id>` and `out <id>` around a pause into `trace`.
fn tracing_deps(trace: &Path, program: fn(Harness) -> &'static str) -> UpgradeDeps {
    let installed = Arc::new(Mutex::new(HashMap::new()));
    let mut deps = fake_deps(installed, "true");
    let trace = trace.to_string_lossy().into_owned();
    deps.plan = Arc::new(move |harness| {
        let script = format!(
            "echo in {id} >> '{trace}'; sleep 0.4; echo out {id} >> '{trace}'",
            id = harness.id()
        );
        Ok(Some(vec![AgentUpgradeStep {
            channel: "npm".into(),
            program: program(harness).into(),
            args: vec!["-c".into(), script],
        }]))
    });
    deps
}

fn job_of(center: &UpgradeCenter, harness: Harness) -> Option<JobView> {
    center
        .snapshot()
        .agents
        .into_iter()
        .find(|view| view.agent == harness.id())
        .and_then(|view| view.job)
}

/// A11: two harnesses upgraded through the same installer never run at the
/// same time; the second says whom it waits for.
#[test]
fn upgrades_through_one_installer_take_turns_and_say_whom_they_wait_for() {
    let dir = tempdir();
    let trace = dir.path().join("trace");
    let center = UpgradeCenter::new(dir.path(), tracing_deps(&trace, |_| "/bin/sh"));
    center.start(Harness::Claude).unwrap();
    // Let the first take the installer before the second asks.
    for _ in 0..100 {
        if std::fs::read_to_string(&trace).is_ok_and(|t| t.contains("in claude")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    center.start(Harness::Codex).unwrap();
    let mut said = None;
    for _ in 0..100 {
        said = job_of(&center, Harness::Codex).and_then(|job| job.waiting_for);
        if said.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(said, Some("Claude Code"));
    wait_done(&center, Harness::Claude);
    wait_done(&center, Harness::Codex);
    let lines: Vec<String> = std::fs::read_to_string(&trace)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines, ["in claude", "out claude", "in codex", "out codex"]);
    assert_eq!(job_of(&center, Harness::Codex).unwrap().waiting_for, None);
}

/// A11: different installers do not wait for each other.
#[test]
fn upgrades_through_different_installers_run_side_by_side() {
    let dir = tempdir();
    let trace = dir.path().join("trace");
    let program = |harness| match harness {
        Harness::Claude => "/bin/sh",
        _ => "/bin/bash",
    };
    if installer_key(&AgentUpgradeStep {
        channel: String::new(),
        program: "/bin/sh".into(),
        args: Vec::new(),
    }) == installer_key(&AgentUpgradeStep {
        channel: String::new(),
        program: "/bin/bash".into(),
        args: Vec::new(),
    }) {
        return; // one shell behind both names here: nothing to show
    }
    let center = UpgradeCenter::new(dir.path(), tracing_deps(&trace, program));
    center.start(Harness::Claude).unwrap();
    center.start(Harness::Codex).unwrap();
    wait_done(&center, Harness::Claude);
    wait_done(&center, Harness::Codex);
    let text = std::fs::read_to_string(&trace).unwrap();
    let first_out = text.find("out ").unwrap();
    assert_eq!(
        text[..first_out].matches("in ").count(),
        2,
        "both started before either finished: {text}"
    );
}

/// A job thread that panics while holding an installer gives it back, and
/// its job ends `Failed` instead of `Running` for ever.
#[test]
fn a_panicking_job_gives_back_its_installer_and_ends_failed() {
    let installers = Arc::new(Installers::default());
    let jobs: Arc<Mutex<HashMap<Harness, Job>>> = Arc::new(Mutex::new(HashMap::from([(
        Harness::Claude,
        Job {
            state: JobState::Running,
            installing: false,
            log: VecDeque::new(),
            message: None,
            waiting_for: None,
            started_at: 0,
            finished_at: None,
        },
    )])));
    let keys = vec!["/bin/sh".to_string()];
    let crashed = {
        let (installers, jobs, keys) = (Arc::clone(&installers), Arc::clone(&jobs), keys.clone());
        std::thread::spawn(move || {
            let _ends = JobEnds {
                jobs,
                harness: Harness::Claude,
            };
            let _held = installers.take(&keys, Harness::Claude, |_| {});
            panic!("a step blew up");
        })
        .join()
    };
    assert!(crashed.is_err());
    {
        let jobs = jobs.lock();
        let job = jobs.get(&Harness::Claude).unwrap();
        assert_eq!(job.state, JobState::Failed);
        assert!(job.message.as_deref().unwrap().contains("没有完成"));
    }
    // The next job through the same installer gets it at once.
    let (tx, rx) = std::sync::mpsc::channel();
    let next = Arc::clone(&installers);
    std::thread::spawn(move || {
        let _held = next.take(&keys, Harness::Codex, |_| {});
        tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(2))
        .expect("the installer must be free after the panic");
}

/// Two quick starts of the same harness run one install, not two.
#[test]
fn two_quick_starts_of_one_harness_run_one_install() {
    let dir = tempdir();
    let trace = dir.path().join("trace");
    let center = UpgradeCenter::new(dir.path(), tracing_deps(&trace, |_| "/bin/sh"));
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let starts: Vec<_> = (0..2)
        .map(|_| {
            let (center, barrier) = (Arc::clone(&center), Arc::clone(&barrier));
            std::thread::spawn(move || {
                barrier.wait();
                center.start(Harness::Claude)
            })
        })
        .collect();
    for start in starts {
        start.join().unwrap().unwrap();
    }
    wait_done(&center, Harness::Claude);
    let text = std::fs::read_to_string(&trace).unwrap();
    assert_eq!(text.matches("in claude").count(), 1, "{text}");
}

/// A plan that cannot be made leaves the harness as it was.
#[test]
fn a_start_whose_plan_fails_leaves_no_running_job() {
    let dir = tempdir();
    let mut deps = fake_deps(Arc::default(), "true");
    deps.plan = Arc::new(|_| Err("no installer found".to_string()));
    let center = UpgradeCenter::new(dir.path(), deps);
    assert_eq!(
        center.start(Harness::Claude),
        Err("no installer found".to_string())
    );
    assert!(job_of(&center, Harness::Claude).is_none());
}
