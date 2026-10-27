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
