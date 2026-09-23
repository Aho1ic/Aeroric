//! Task P2-03's parity and write-path acceptance.
//!
//! Part 1 replays `contracts/native/v1/fixtures/projects.json` — 40 cases whose expected
//! output was produced by the shipping client's own TypeScript — through the Rust port.
//! Part 2 covers what a fixture cannot: that a real change costs exactly one revision, that
//! a no-op costs none, and that removing a project with live tasks is refused.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aeroric_core::location::{StorageLocation, StorageLocationProbe, VolumeKind};
use aeroric_core::projects::{mutations, GroupConfig, ProjectError, ProjectService};
use aeroric_core::storage::runtime;
use aeroric_core::storage::Record;
use aeroric_core::{AppPaths, Clock, CoreContext, FixedClock, LeaseOwner, SystemClock};
use serde_json::{json, Value};

struct LocalVolume;

impl StorageLocationProbe for LocalVolume {
    fn probe(&self, data_dir: &Path) -> StorageLocation {
        StorageLocation::new(data_dir, VolumeKind::Local)
    }
}

fn records(value: &Value) -> Vec<Record> {
    value
        .as_array()
        .expect("expected a JSON array of project records")
        .iter()
        .map(|item| Record(item.as_object().expect("record must be an object").clone()))
        .collect()
}

fn records_json(records: &[Record]) -> Value {
    Value::Array(
        records
            .iter()
            .map(|record| Value::Object(record.0.clone()))
            .collect(),
    )
}

fn location_json(project: &Record) -> Value {
    // Serialised through the contract type, so the parity case checks the same spelling the
    // wire carries rather than a test-local rendering of it.
    serde_json::to_value(mutations::resolve_location(project)).expect("target is serialisable")
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../contracts/native/v1/fixtures/projects.json")
}

fn run_case(operation: &str, input: &Value) -> Value {
    let field = |key: &str| input.get(key).cloned().unwrap_or(Value::Null);
    let string = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{operation}: {key} must be a string"))
            .to_string()
    };
    let now = || input.get("now").and_then(Value::as_i64).expect("now");
    match operation {
        "sortProjectsForRail" => {
            records_json(&mutations::sort_for_rail(&records(&field("projects"))))
        }
        "normalizeProjectOrder" => {
            records_json(&mutations::normalize_order(&records(&field("projects"))).0)
        }
        "applyProjectOrder" => {
            let ids: Vec<String> = field("orderedProjectIds")
                .as_array()
                .expect("orderedProjectIds")
                .iter()
                .map(|id| id.as_str().expect("id string").to_string())
                .collect();
            records_json(&mutations::apply_order(&records(&field("projects")), &ids))
        }
        "groupProjectsForRail" => {
            let projects = records(&field("projects"));
            let configured: Vec<String> = field("configuredNames")
                .as_array()
                .expect("configuredNames")
                .iter()
                .map(|name| name.as_str().unwrap_or_default().to_string())
                .collect();
            Value::Array(
                mutations::group_projects_for_rail(&projects, &configured)
                    .into_iter()
                    .map(|view| {
                        let grouped: Vec<Record> = view
                            .project_ids
                            .iter()
                            .filter_map(|id| {
                                projects
                                    .iter()
                                    .find(|project| {
                                        mutations::text(project, "id") == Some(id.as_str())
                                    })
                                    .cloned()
                            })
                            .collect();
                        json!({
                            "name": view.name,
                            "projects": records_json(&grouped),
                            "isUngrouped": view.is_ungrouped,
                        })
                    })
                    .collect(),
            )
        }
        "mergeProjectGroupNames" => {
            let configured: Vec<String> = field("configuredNames")
                .as_array()
                .expect("configuredNames")
                .iter()
                .map(|name| name.as_str().unwrap_or_default().to_string())
                .collect();
            Value::Array(
                mutations::merge_group_names(&records(&field("projects")), &configured)
                    .into_iter()
                    .map(Value::from)
                    .collect(),
            )
        }
        "normalizeProjectGroupNames" => Value::Array(
            mutations::normalize_group_names(&field("value"))
                .into_iter()
                .map(Value::from)
                .collect(),
        ),
        "projectGroupForProject" => Value::from(mutations::group_of(&Record(
            field("project").as_object().expect("project").clone(),
        ))),
        "upsertLocalProject" => {
            // The id comes from the caller in the core (ADR-013 Q28). Feeding the parity run
            // the same `String(now)` the client would invent keeps the two comparable: the id
            // *shape* is asserted separately, in
            // `new_ids_come_from_the_caller_and_must_be_unique`.
            let out = mutations::upsert_local(
                &records(&field("projects")),
                &string("path"),
                &now().to_string(),
                now(),
            );
            json!({ "project": records_json(&[out.project])[0], "projects": records_json(&out.projects) })
        }
        "upsertSshProject" => {
            let args = field("input");
            let out = mutations::upsert_ssh(
                &records(&field("projects")),
                args["name"].as_str().unwrap_or_default(),
                args["connectionId"].as_str().unwrap_or_default(),
                args["remotePath"].as_str().unwrap_or_default(),
                &now().to_string(),
                now(),
            );
            json!({ "project": records_json(&[out.project])[0], "projects": records_json(&out.projects) })
        }
        "upsertWslProject" => {
            let args = field("input");
            let out = mutations::upsert_wsl(
                &records(&field("projects")),
                args["name"].as_str().unwrap_or_default(),
                args["distribution"].as_str().unwrap_or_default(),
                args["linuxPath"].as_str().unwrap_or_default(),
                &now().to_string(),
                now(),
            );
            json!({
                "project": records_json(&[out.project])[0],
                "projects": records_json(&out.projects),
                "reused": out.reused,
            })
        }
        "touchProjectInList" => {
            let out = mutations::touch(&records(&field("projects")), &string("projectId"), now());
            json!({ "project": records_json(&[out.project])[0], "projects": records_json(&out.projects) })
        }
        "assignProjectGroup" => records_json(&mutations::assign_group(
            &records(&field("projects")),
            &string("projectId"),
            input.get("groupName"),
        )),
        "renameProjectGroupInList" => {
            let (next, changed) = mutations::rename_group(
                &records(&field("projects")),
                &string("oldName"),
                &string("nextName"),
            );
            json!({ "projects": records_json(&next), "groupChanged": changed, "groupsNext": [] })
        }
        "clearProjectGroupFromList" => records_json(&mutations::clear_group(
            &records(&field("projects")),
            &string("groupName"),
        )),
        "renameProjectInList" => records_json(&mutations::rename(
            &records(&field("projects")),
            &string("projectId"),
            &string("name"),
        )),
        "toggleProjectHiddenInList" => records_json(&mutations::toggle_hidden(
            &records(&field("projects")),
            &string("projectId"),
        )),
        "toggleProjectPinnedInList" => {
            let (next, _) =
                mutations::toggle_pinned(&records(&field("projects")), &string("projectId"));
            records_json(&next)
        }
        "setProjectAvatarInList" => {
            let avatar = match input.get("avatar") {
                Some(value) if !value.is_null() => Some(value.clone()),
                _ => None,
            };
            records_json(&mutations::set_avatar(
                &records(&field("projects")),
                &string("projectId"),
                avatar,
            ))
        }
        "deriveProjectName" => Value::from(mutations::derive_project_name(&string("path"))),
        "normalizeRemotePath" => Value::from(mutations::normalize_remote_path(&string("path"))),
        "normalizeWslProjectPath" => {
            Value::from(mutations::normalize_wsl_path(&string("linuxPath")))
        }
        "sshProjectPath" => Value::from(mutations::ssh_path(
            &string("connectionId"),
            &string("remotePath"),
        )),
        "wslProjectPath" => Value::from(mutations::wsl_path(
            &string("distribution"),
            &string("linuxPath"),
        )),
        "resolveProjectLocation" => location_json(&Record(
            field("project").as_object().expect("project").clone(),
        )),
        other => panic!("no Rust implementation is wired for operation {other:?}"),
    }
}

#[test]
fn every_published_project_case_reproduces_the_shipping_clients_result() {
    let path = fixture_path();
    let document: Value =
        serde_json::from_slice(&fs::read(&path).expect("fixtures/projects.json must exist"))
            .expect("valid JSON");
    let cases = document["cases"].as_array().expect("cases").clone();
    assert!(
        cases.len() >= 40,
        "the parity table shrank to {} cases; deleting a case deletes a guarantee",
        cases.len()
    );
    let mut failures: Vec<String> = Vec::new();
    let mut divergences = 0usize;
    for case in &cases {
        let name = case["name"].as_str().unwrap_or_default();
        let operation = case["operation"].as_str().unwrap_or_default();
        let actual = run_case(operation, &case["input"]);
        // `coreExpected` is a *declared* divergence from the client (ADR-013): it has to
        // differ from `expected`, carry a reason, and be reviewed as such. Comparing against
        // it silently would be the same sin as editing an expectation by hand.
        let declared = case.get("coreExpected");
        let reason = case.get("divergenceReason").and_then(Value::as_str);
        if let Some(core_expected) = declared {
            divergences += 1;
            assert_ne!(
                core_expected,
                &case["expected"],
                "{name}: coreExpected identical to the client result is not a divergence, it is a stale row"
            );
            assert!(
                reason.is_some_and(|text| text.len() > 20),
                "{name}: a declared divergence needs a reason a reviewer can read"
            );
            if actual != *core_expected {
                failures.push(format!(
                    "{name}\n    operation: {operation}\n    coreExpected: {core_expected}\n    actual:       {actual}"
                ));
            }
            continue;
        }
        assert!(reason.is_none(), "{name}: a reason without coreExpected");
        if actual != case["expected"] {
            failures.push(format!(
                "{name}\n    operation: {operation}\n    expected: {}\n    actual:   {}",
                case["expected"], actual
            ));
        }
    }
    assert!(
        divergences >= 2,
        "the declared divergences disappeared ({divergences}); ADR-013's decisions must stay visible"
    );
    assert!(
        failures.is_empty(),
        "{} of {} project cases disagree with the client:\n\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n\n")
    );
}

#[test]
fn the_parity_table_is_not_silently_reduced() {
    let document: Value =
        serde_json::from_slice(&fs::read(fixture_path()).expect("fixture")).expect("valid JSON");
    assert_eq!(
        document["caseCount"].as_u64().expect("caseCount") as usize,
        document["cases"].as_array().expect("cases").len(),
        "caseCount must equal the number of rows, or the table was truncated"
    );
    let mut names: Vec<&str> = document["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|case| case["name"].as_str().unwrap_or_default())
        .collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), before, "case names must be unique");
}

// --------------------------------------------------------------------------- write path

fn service(tag: &str) -> (PathBuf, Arc<CoreContext>) {
    let root = std::env::temp_dir().join(format!(
        "aeroric-projects-{tag}-{}-{}",
        std::process::id(),
        SystemClock.now_ms()
    ));
    fs::remove_dir_all(&root).ok();
    let paths = AppPaths::with_data_dir(&root, Some(&root.join("managed")));
    let context = Arc::new(
        CoreContext::acquire(
            paths,
            Arc::new(FixedClock::at(1_700_000_000_000)),
            &LeaseOwner {
                application: "aeroric-core-test".into(),
                version: "0.0.0-test".into(),
            },
            &LocalVolume,
        )
        .expect("lease"),
    );
    (root, context)
}

fn seed(context: &CoreContext, projects: &[Record]) {
    aeroric_core::storage::save_projects(context, projects).expect("seed projects");
}

#[test]
fn opening_a_project_writes_once_and_moves_the_revision_exactly_one_step() {
    let (root, context) = service("open");
    let service = ProjectService::new(context.clone());
    seed(
        &context,
        &records(
            &json!([{ "id": "1", "name": "one", "path": "/repo/one", "lastOpenedAt": 1, "orderIndex": 0 }]),
        ),
    );

    let (_, applied) = service
        .open_local(
            "/repo/two",
            "10000000000000000000000000000001",
            1_700_000_000_500,
        )
        .expect("open");
    assert!(applied.changed);
    assert_eq!(
        applied.revision.as_deref(),
        Some("1"),
        "one write, one revision"
    );
    let (_, again) = service
        .open_local(
            "/repo/two",
            "10000000000000000000000000000001",
            1_700_000_000_900,
        )
        .expect("reopen");
    assert_eq!(again.revision.as_deref(), Some("2"));
    assert_eq!(
        again.projects.len(),
        2,
        "the same path must not create a second record"
    );
    let listed = service.list().expect("list");
    assert_eq!(listed.len(), 2, "and the saved state agrees");
    fs::remove_dir_all(root).ok();
}

#[test]
fn a_no_op_mutation_does_not_consume_a_revision() {
    let (root, context) = service("noop");
    let service = ProjectService::new(context.clone());
    seed(
        &context,
        &records(&json!([
            { "id": "a", "name": "a", "path": "/repo/a", "lastOpenedAt": 1, "orderIndex": 0 }
        ])),
    );
    service.set_pinned("a", false).expect("already unpinned");
    let revision = runtime::load(&context)
        .expect("load")
        .found()
        .map(|m| m.revision);
    assert_eq!(
        revision, None,
        "a mutation that changed nothing must not look like new state"
    );
    service.set_pinned("a", true).expect("pin");
    let after = runtime::load(&context)
        .expect("load")
        .found()
        .expect("present");
    assert_eq!(after.revision, "1");
    assert_eq!(
        after.volume_kind, "local",
        "the lease's volume answer is recorded"
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn removing_a_project_with_live_tasks_is_refused_and_changes_nothing() {
    let (root, context) = service("remove");
    let service = ProjectService::new(context.clone());
    seed(
        &context,
        &records(&json!([
            { "id": "p1", "name": "one", "path": "/repo/p1", "lastOpenedAt": 1, "orderIndex": 0 }
        ])),
    );
    let tasks = records(&json!([
        { "id": "t1", "projectId": "p1", "status": "running", "prompt": "x", "agent": "codex",
          "permissionMode": "ask", "createdAt": 1 },
        { "id": "t2", "projectId": "p1", "status": "done", "prompt": "y", "agent": "codex",
          "permissionMode": "ask", "createdAt": 2 }
    ]));
    aeroric_core::storage::save_tasks(&context, "p1", &tasks).expect("seed tasks");

    let error = service
        .remove("p1")
        .expect_err("a running task blocks removal");
    match &error {
        ProjectError::TasksStillRunning {
            live_count,
            statuses,
            ..
        } => {
            assert_eq!(*live_count, 1, "only `running` is live; `done` is not");
            assert_eq!(statuses, "running");
        }
        other => panic!("expected a refusal naming the live tasks, got {other:?}"),
    }
    assert_eq!(service.list().expect("still listed").len(), 1);
    assert!(
        runtime::load(&context)
            .expect("no revision")
            .found()
            .is_none(),
        "a refused removal must not bump the revision either"
    );

    let settled = records(&json!([
        { "id": "t1", "projectId": "p1", "status": "cancelled", "prompt": "x", "agent": "codex",
          "permissionMode": "ask", "createdAt": 1 }
    ]));
    aeroric_core::storage::save_tasks(&context, "p1", &settled).expect("cancel first");
    let applied = service.remove("p1").expect("remove once settled");
    assert!(applied.projects.is_empty());
    assert_eq!(applied.revision.as_deref(), Some("1"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn removal_keeps_the_project_directory_and_every_other_record() {
    let (root, context) = service("remove-keeps");
    let service = ProjectService::new(context.clone());
    seed(
        &context,
        &records(&json!([
            { "id": "p1", "name": "one", "path": "/repo/p1", "lastOpenedAt": 1, "orderIndex": 0, "branch": "main" },
            { "id": "p2", "name": "two", "path": "/repo/p2", "lastOpenedAt": 2, "orderIndex": 1 }
        ])),
    );
    fs::create_dir_all(context.paths.project_dir("p1").unwrap()).expect("project dir");
    fs::write(context.paths.tasks_file("p1").unwrap(), b"[]\n").expect("task file");
    service.remove("p1").expect("remove");
    let listed = service.list().expect("list");
    assert_eq!(
        mutations::text(&listed[0], "id"),
        Some("p2"),
        "only the named record is dropped"
    );
    assert!(
        context.paths.tasks_file("p1").unwrap().exists(),
        "removing a record must not delete anything on disk (plan P2-03 Action 3)"
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn the_rail_can_be_computed_without_any_window_state() {
    let (root, context) = service("rail");
    let service = ProjectService::new(context.clone());
    seed(
        &context,
        &records(&json!([
            { "id": "a", "name": "a", "path": "/repo/a", "lastOpenedAt": 1, "orderIndex": 2, "group": "Work" },
            { "id": "b", "name": "b", "path": "/repo/b", "lastOpenedAt": 2, "pinned": true },
            { "id": "c", "name": "c", "path": "/repo/c", "lastOpenedAt": 3, "orderIndex": 0, "hiddenFromRail": true }
        ])),
    );
    let rail = service
        .rail(&GroupConfig {
            names: vec!["Work".into(), "Work".into(), "  ".into()],
        })
        .expect("rail");
    assert_eq!(
        rail.groups.len(),
        2,
        "Work + the implicit ungrouped section"
    );
    assert_eq!(rail.groups[0].name, "Work");
    assert!(rail.groups[1].is_ungrouped);
    assert_eq!(
        rail.groups[1]
            .projects
            .iter()
            .map(|p| mutations::text(p, "id").unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["b", "c"],
        "pinned first, and hidden is a view concern, not a data one"
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn a_corrupt_projects_file_is_refused_before_any_mutation() {
    let (root, context) = service("corrupt");
    context.paths.ensure_private().expect("private");
    fs::write(context.paths.projects_file(), b"[{\"id\":").expect("corrupt");
    let service = ProjectService::new(context.clone());
    let error = service
        .open_local("/repo/x", "10000000000000000000000000000003", 5)
        .expect_err("cannot write over garbage");
    assert!(
        error.to_string().contains("refusing to overwrite"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(context.paths.projects_file()).expect("still there"),
        "[{\"id\":",
        "and the unreadable bytes are untouched"
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn an_ssh_path_is_never_a_local_path() {
    // Plan P2-03 Action 2 names this exactly: `ssh://conn/app` looks like a path and is
    // not one. Identity comes from the location; the `path` field is display only.
    let remote = Record(
        json!({
            "id": "s", "name": "s", "path": "ssh://conn-a/srv/app", "lastOpenedAt": 1,
            "location": { "kind": "ssh", "connectionId": "conn-a", "remotePath": "/srv/app" }
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    assert_eq!(mutations::identity_of(&remote), "ssh:conn-a:/srv/app");
    let local_lookalike = Record(
        json!({ "id": "l", "name": "l", "path": "ssh://conn-a/srv/app", "lastOpenedAt": 1 })
            .as_object()
            .unwrap()
            .clone(),
    );
    assert_eq!(
    mutations::identity_of(&local_lookalike),
    "local:ssh://conn-a/srv/app",
    "a legacy record with no location is keyed as local by its literal path — that is          what the shipping client does, and inventing remote identity here would silently          merge two different projects"
);
    assert_ne!(
    mutations::identity_of(&remote),
    mutations::identity_of(&local_lookalike),
    "the same display string must not make one project of two records that mean          different things"
);
}

#[test]
fn a_location_the_core_cannot_name_falls_back_to_local_instead_of_guessing() {
    let odd = Record(
        json!({
            "id": "o", "name": "o", "path": "/repo/o", "lastOpenedAt": 1,
            "location": { "kind": "carrier-pigeon", "remotePath": "/srv/app" }
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    assert_eq!(mutations::identity_of(&odd), "local:/repo/o");
    assert!(
        !mutations::is_hidden(&odd),
        "and the record still reads normally: an unknown location is not an error"
    );
}

// ------------------------------------------------------- ADR-013 decisions, one test each

#[test]
fn local_identity_normalises_losslessly_and_no_further() {
    assert_eq!(mutations::normalise_local_path("/repo/one/"), "/repo/one");
    assert_eq!(
        mutations::normalise_local_path("C:\\repo\\app\\"),
        "C:/repo/app"
    );
    assert_eq!(
        mutations::normalise_local_path("/"),
        "/",
        "the root is still the root"
    );
    // Case and symlink spellings are *not* folded: that is a filesystem property, and
    // guessing it would merge two directories that really are different on some volumes.
    assert_ne!(
        mutations::normalise_local_path("/Repo/One"),
        mutations::normalise_local_path("/repo/one")
    );
}

#[test]
fn reopening_a_differently_spelled_path_touches_the_same_project() {
    let (root, context) = service("normalise");
    let service = ProjectService::new(context.clone());
    service
        .open_local(
            "/repo/one/",
            "11111111-1111-4111-8111-111111111111",
            1_700_000_000_000,
        )
        .expect("first open");
    let (again, applied) = service
        .open_local(
            "/repo/one",
            "22222222-2222-4222-8222-222222222222",
            1_700_000_000_500,
        )
        .expect("reopen without the slash");
    assert!(again.reused, "one record, not two");
    assert_eq!(applied.projects.len(), 1);
    assert_eq!(
        mutations::text(&applied.projects[0], "id").unwrap_or_default(),
        "11111111-1111-4111-8111-111111111111",
        "touching keeps the original identity"
    );
    assert_eq!(applied.revision.as_deref(), Some("2"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn reorder_with_repeated_ids_writes_one_record_per_id() {
    let (root, context) = service("dedupe");
    let service = ProjectService::new(context.clone());
    seed(
        &context,
        &records(&json!([
            { "id": "a", "name": "a", "path": "/repo/a", "lastOpenedAt": 1, "orderIndex": 0 },
            { "id": "b", "name": "b", "path": "/repo/b", "lastOpenedAt": 2, "orderIndex": 1 }
        ])),
    );
    let applied = service
        .reorder(&["a".to_string(), "a".to_string(), "b".to_string()])
        .expect("deduped reorder");
    assert_eq!(
        applied.projects.len(),
        2,
        "a duplicated id must not duplicate a record"
    );
    assert_eq!(
        applied
            .projects
            .iter()
            .map(|p| mutations::text(p, "id").unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert!(applied.projects[1].get("orderIndex").is_some());
    fs::remove_dir_all(root).ok();
}

#[test]
fn new_ids_come_from_the_caller_and_a_duplicate_is_refused() {
    let (root, context) = service("dup-id");
    let service = ProjectService::new(context.clone());
    seed(
        &context,
        &records(&json!([
            { "id": "same", "name": "a", "path": "/repo/a", "lastOpenedAt": 1, "orderIndex": 0 },
            { "id": "same", "name": "b", "path": "/repo/b", "lastOpenedAt": 2, "orderIndex": 1 }
        ])),
    );
    let error = service
        .rename("same", "renamed")
        .expect_err("two records cannot share one id");
    assert!(
        matches!(&error, ProjectError::DuplicateId { project_id } if project_id == "same"),
        "{error:?}"
    );
    assert_eq!(
        service.list().expect("list still readable").len(),
        2,
        "the refusal is not a rewrite: the file is untouched"
    );
    assert!(
        runtime::load(&context)
            .expect("no metadata")
            .found()
            .is_none(),
        "and nothing was bumped"
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn the_group_list_is_its_own_file_and_settings_is_not_touched() {
    let (root, context) = service("groups");
    let service = ProjectService::new(context.clone());
    seed(
        &context,
        &records(&json!([
            { "id": "a", "name": "a", "path": "/repo/a", "lastOpenedAt": 1, "orderIndex": 0 }
        ])),
    );
    let created = service.create_group("  Work  ").expect("create group");
    assert_eq!(
        created.groups_next,
        vec!["Work".to_string()],
        "trimmed, then stored"
    );
    assert_eq!(created.revision.as_deref(), Some("1"));
    let groups_file = context.paths.project_groups_file();
    assert!(
        groups_file.exists(),
        "core-owned file, next to projects.json"
    );
    assert!(
        !context.paths.settings_file().exists(),
        "settings.json must not be involved: AppSettings would drop an unknown key on its next save"
    );
    // A group survives with no project in it; assigning a project to it does not re-add it.
    let assigned = service
        .assign_group("a", Some(json!("Work")))
        .expect("assign");
    assert_eq!(assigned.groups_next, vec!["Work".to_string()]);
    let renamed = service
        .rename_group("Work", "Work 2026")
        .expect("rename group");
    assert_eq!(renamed.groups_next, vec!["Work 2026".to_string()]);
    assert_eq!(
        mutations::text(&renamed.projects[0], "group"),
        Some("Work 2026"),
        "members follow the rename"
    );
    let cleared = service.clear_group("Work 2026").expect("clear group");
    assert!(
        cleared.groups_next.is_empty(),
        "the stored list is maintained, not recomputed from members"
    );
    assert_eq!(
        mutations::text(&cleared.projects[0], "group"),
        None,
        "and the project stops pointing at it"
    );
    let reread = ProjectService::new(context.clone())
        .stored_groups()
        .expect("read back");
    assert!(reread.is_empty());
    fs::remove_dir_all(root).ok();
}

#[test]
fn an_unaddressable_record_is_listed_and_removable_but_nothing_else() {
    let (root, context) = service("unaddressable");
    let service = ProjectService::new(context.clone());
    seed(
        &context,
        &records(&json!([
            { "id": "o", "name": "o", "path": "/repo/o", "lastOpenedAt": 1, "orderIndex": 0,
              "location": { "kind": "carrier-pigeon", "remotePath": "/srv/app" } },
            { "id": "ok", "name": "ok", "path": "/repo/ok", "lastOpenedAt": 2, "orderIndex": 1 }
        ])),
    );
    assert_eq!(
        service.list().expect("listed").len(),
        2,
        "the bad row is not hidden"
    );
    let rail = service.rail(&GroupConfig::default()).expect("rail");
    assert_eq!(
        rail.groups.iter().map(|g| g.projects.len()).sum::<usize>(),
        2
    );

    let error = service
        .touch("o", 1_700_000_000_777)
        .expect_err("touching needs to know where it is");
    assert!(
        matches!(&error, ProjectError::Unaddressable { kind, .. } if kind == "carrier-pigeon"),
        "{error:?}"
    );
    assert!(
        service
            .rename("o", "x")
            .expect_err("rename is refused too")
            .to_string()
            .contains("unrecognised location kind"),
        "and says why"
    );
    assert!(
        service
            .reorder(&["o".to_string()])
            .expect_err("reorder is refused too")
            .to_string()
            .contains("carrier-pigeon"),
        "naming the offending kind, not a generic failure"
    );
    // Deleting does not address the location: it drops a record by id.
    let removed = service.remove("o").expect("removal is always possible");
    assert_eq!(removed.projects.len(), 1, "only the bad row went away");
    assert_eq!(
        mutations::text(&removed.projects[0], "id"),
        Some("ok"),
        "the healthy record is untouched"
    );
    fs::remove_dir_all(root).ok();
}
