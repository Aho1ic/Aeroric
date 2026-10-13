//! Real process/parser boundary, entirely within owned scratch roots.
#![cfg(unix)]

#[path = "common/conda_fixture.rs"]
mod conda_fixture;
#[path = "common/scratch.rs"]
mod scratch;

use conda_fixture::{script, FakeConda};
use scratch::Scratch;

#[test]
fn owned_executable_to_bare_camel_case_array() {
    let root = Scratch::new("conda-discovery");
    let fake = FakeConda::new(root.path());
    assert_eq!(
        serde_json::to_value(fake.probe().detect()).unwrap(),
        fake.wire
    );
    assert_eq!(
        std::fs::read_to_string(&fake.marker).unwrap(),
        "env list --json\n"
    );
}

#[test]
fn candidate_loop_skips_failures_and_stops_at_first_nonempty_list() {
    use std::os::unix::fs::PermissionsExt;
    let root = Scratch::new("conda-order");
    let fake = FakeConda::new(root.path());
    let denied = script(root.path(), "denied", "exit 0");
    std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o600)).unwrap();
    let nonzero = script(
        root.path(),
        "nonzero",
        r#"
printf 'nonzero\n' >> "$FAKE_CONDA_MARKER"
printf '%s\n' "$FAKE_CONDA_JSON"
exit 13
"#,
    );
    let bad = script(
        root.path(),
        "bad",
        r#"
printf 'bad\n' >> "$FAKE_CONDA_MARKER"
printf 'not json\n'
"#,
    );
    let empty = script(
        root.path(),
        "empty",
        r#"
printf 'empty\n' >> "$FAKE_CONDA_MARKER"
printf '{"envs":[]}\n'
"#,
    );
    let later = script(
        root.path(),
        "must-not-run",
        r#"
printf 'LATE\n' >> "$FAKE_CONDA_MARKER"
printf '%s\n' "$FAKE_CONDA_JSON"
"#,
    );
    let mut probe = fake.probe();
    probe.candidates = Some(vec![
        root.path().join("unavailable"),
        denied,
        nonzero,
        bad,
        empty,
        fake.program.clone(),
        later,
    ]);
    assert_eq!(serde_json::to_value(probe.detect()).unwrap(), fake.wire);
    assert_eq!(
        std::fs::read_to_string(&fake.marker).unwrap(),
        "nonzero\nbad\nempty\nenv list --json\n",
    );
}

#[test]
fn all_failed_candidates_return_empty_not_an_error() {
    let root = Scratch::new("conda-empty");
    let fake = FakeConda::new(root.path());
    for (raw, status) in [
        (fake.raw.as_str(), 13),
        ("not json", 0),
        ("{}", 0),
        ("{\"envs\":[]}", 0),
    ] {
        assert!(fake.probe_with_output(raw, status).detect().is_empty());
    }
    let mut missing = fake.probe();
    missing.candidates = Some(vec![root.path().join("not-installed")]);
    assert!(missing.detect().is_empty());
    assert_eq!(
        std::fs::read_to_string(&fake.marker)
            .unwrap()
            .lines()
            .count(),
        4
    );
}

#[test]
fn explicit_empty_candidates_do_not_fall_back_to_system_tools() {
    let root = Scratch::new("conda-no-probe");
    let fake = FakeConda::new(root.path());
    let mut probe = fake.probe();
    probe.candidates = Some(Vec::new());
    assert!(probe.detect().is_empty());
    assert!(!fake.marker.exists());
}
