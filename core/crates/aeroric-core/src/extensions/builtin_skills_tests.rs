//! All in temporary folders; nothing touches the real home or a real project.

use super::*;

fn temp() -> PathBuf {
    let dir = std::env::temp_dir()
        .join("aeroric-builtin-skills-tests")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git_init(dir: &Path) {
    let ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[test]
fn the_catalog_lists_all_27_skills_and_each_has_a_skill_md() {
    let skills = catalog();
    assert_eq!(skills.len(), 27);
    assert_eq!(skills.iter().filter(|s| s.group == "develop").count(), 14);
    assert_eq!(skills.iter().filter(|s| s.group == "science").count(), 13);
    let data = temp();
    let root = materialize(&data).unwrap();
    for skill in &skills {
        assert!(
            skill_dir(&root, skill).join("SKILL.md").is_file(),
            "{}",
            skill.id
        );
        assert!(
            !skill.title.is_empty() && !skill.detail.is_empty(),
            "{}",
            skill.id
        );
    }
    assert!(root.join("develop/NOTICE.md").is_file());
    assert!(root.join("science/NOTICE.md").is_file());
}

#[cfg(unix)]
#[test]
fn materializing_keeps_scripts_executable_and_is_done_once() {
    use std::os::unix::fs::PermissionsExt;
    let data = temp();
    let root = materialize(&data).unwrap();
    let script = root.join("develop/systematic-debugging/find-polluter.sh");
    assert!(std::fs::metadata(&script).unwrap().permissions().mode() & 0o111 != 0);
    std::fs::write(root.join("develop/brainstorming/SKILL.md"), "touched").unwrap();
    assert_eq!(materialize(&data).unwrap(), root);
    assert_eq!(
        std::fs::read_to_string(root.join("develop/brainstorming/SKILL.md")).unwrap(),
        "touched"
    );
}

#[test]
fn turned_off_and_shadowed_skills_are_not_offered() {
    let data = temp();
    let mut settings = SkillSettings::default();
    settings.disabled.insert("peer-review".into());
    save_settings(&data, &settings).unwrap();
    let user: BTreeSet<String> = ["brainstorming".to_string()].into();
    let ids: Vec<String> = offered(&data, &user).into_iter().map(|s| s.id).collect();
    assert_eq!(ids.len(), 25);
    assert!(!ids.contains(&"peer-review".to_string()));
    assert!(!ids.contains(&"brainstorming".to_string()));
}

#[cfg(unix)]
#[test]
fn the_users_own_skills_are_found_but_aeroric_copies_and_links_are_not() {
    let home = temp();
    let user = home.join(".codex/skills/brainstorming");
    std::fs::create_dir_all(&user).unwrap();
    std::fs::write(user.join("SKILL.md"), "mine").unwrap();
    let cwd = temp();
    let copy = cwd.join(".agents/skills/tdd-copy");
    std::fs::create_dir_all(&copy).unwrap();
    std::fs::write(copy.join("SKILL.md"), "x").unwrap();
    std::fs::write(copy.join(MARKER), "v").unwrap();
    let names = user_skill_names(&user_skill_dirs("codex", Some(&cwd), Some(&home)));
    assert_eq!(names, ["brainstorming".to_string()].into());
}

#[cfg(unix)]
#[test]
fn linking_adds_and_prunes_only_aerorics_links() {
    let data = temp();
    let root = materialize(&data).unwrap();
    let home = temp().join("skills");
    std::fs::create_dir_all(home.join("my-own")).unwrap();
    let all = catalog();
    link_into(&home, &root, &all, &BTreeSet::new()).unwrap();
    assert_eq!(
        std::fs::read_link(home.join("brainstorming")).unwrap(),
        skill_dir(&root, &all[0])
    );
    let skip: BTreeSet<String> = ["brainstorming".to_string()].into();
    link_into(&home, &root, &all[..3], &skip).unwrap();
    assert!(
        std::fs::symlink_metadata(home.join("brainstorming")).is_err(),
        "skipped link removed"
    );
    assert!(home.join("writing-plans").exists());
    assert!(
        std::fs::symlink_metadata(home.join("peer-review")).is_err(),
        "not wanted any more"
    );
    assert!(home.join("my-own").is_dir(), "the user's folder stays");
}

#[cfg(unix)]
#[test]
fn a_claude_plugin_names_itself_aeroric() {
    let data = temp();
    let root = materialize(&data).unwrap();
    let dir = claude_plugin(&data, &root, &catalog()[..2]).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join(".claude-plugin/plugin.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["name"], "aeroric");
    assert!(dir.join("skills/brainstorming/SKILL.md").is_file());
    assert_ne!(dir, claude_plugin(&data, &root, &catalog()[..3]).unwrap());
}

#[test]
fn project_copies_are_marked_updated_removed_and_kept_out_of_git() {
    let data = temp();
    let root = materialize(&data).unwrap();
    let cwd = temp();
    git_init(&cwd);
    let mine = cwd.join(".agents/skills/writing-plans");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("SKILL.md"), "the user's").unwrap();
    let all = catalog();
    let removed = std::sync::Mutex::new(Vec::new());
    let remove = |path: &Path| {
        removed.lock().unwrap().push(path.to_path_buf());
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())
    };
    sync_project(&cwd, &root, &all[..3], &remove).unwrap();
    assert!(cwd.join(".agents/skills/brainstorming/SKILL.md").is_file());
    assert_eq!(
        std::fs::read_to_string(mine.join("SKILL.md")).unwrap(),
        "the user's"
    );
    assert_eq!(project_copies(&cwd).len(), 2);
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(&cwd)
        .output()
        .unwrap();
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(!status.contains("brainstorming"), "{status}");
    assert!(
        status.contains("writing-plans"),
        "the user's own skill is not hidden: {status}"
    );
    // Outdated copy is replaced; a turned-off one goes to `remove`.
    std::fs::write(cwd.join(".agents/skills/brainstorming").join(MARKER), "old").unwrap();
    sync_project(&cwd, &root, &all[..1], &remove).unwrap();
    assert_eq!(
        std::fs::read_to_string(cwd.join(".agents/skills/brainstorming").join(MARKER)).unwrap(),
        content_hash()
    );
    assert_eq!(removed.lock().unwrap().len(), 1);
    assert_eq!(remove_from_project(&cwd, &remove).unwrap(), 1);
    let exclude = std::fs::read_to_string(cwd.join(".git/info/exclude")).unwrap();
    assert!(!exclude.contains("Aeroric"), "{exclude}");
}

#[test]
fn a_project_without_skills_is_left_untouched() {
    let data = temp();
    let root = materialize(&data).unwrap();
    let cwd = temp();
    sync_project(&cwd, &root, &[], &|_| Ok(())).unwrap();
    assert!(!cwd.join(".agents").exists());
}

#[test]
fn the_exclude_block_is_replaced_and_the_rest_kept() {
    let ids: BTreeSet<String> = ["a".to_string(), "b".to_string()].into();
    let once = exclude_with("# git default\n*.log\n", &ids, "/.agents/skills");
    assert!(once.starts_with("# git default\n*.log\n\n# >>> Aeroric"));
    assert!(once.contains("/.agents/skills/a/\n/.agents/skills/b/\n"));
    let fewer: BTreeSet<String> = ["b".to_string()].into();
    let twice = exclude_with(&once, &fewer, "/.agents/skills");
    assert!(!twice.contains("/skills/a/"));
    assert_eq!(
        exclude_with(&twice, &BTreeSet::new(), "/.agents/skills"),
        "# git default\n*.log\n"
    );
}

#[test]
fn invocations_follow_each_harness() {
    assert_eq!(
        invocation("codex", "systematic-debugging", false),
        "$systematic-debugging"
    );
    assert_eq!(
        invocation("claude", "systematic-debugging", true),
        "/aeroric:systematic-debugging"
    );
    assert_eq!(
        invocation("claude", "systematic-debugging", false),
        "/systematic-debugging"
    );
    assert_eq!(invocation("omp", "peer-review", false), "/peer-review");
}
