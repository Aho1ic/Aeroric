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

/// B5-3b: an accepted learned skill joins the offers for its project's
/// folders (or everywhere when global) in all three mounts.
#[test]
fn learned_skills_are_offered_where_they_belong() {
    let data = temp();
    let project = temp();
    let elsewhere = temp();
    let skill = learned_dir(&data).join("learned-abc123").join("v2");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: learned-abc123\n---\n# x\n",
    )
    .unwrap();
    write_learned_index(
        &data,
        &LearnedIndex {
            skills: vec![LearnedEntry {
                name: "learned-abc123".into(),
                dir: skill.clone(),
                version: 2,
                global: false,
                project_path: Some(project.clone()),
            }],
        },
    )
    .unwrap();
    let here = learned_offers(&data, Some(&project.join("sub")));
    assert_eq!(here.len(), 1);
    assert_eq!(here[0].version, "learned-v2");
    assert!(learned_offers(&data, Some(&elsewhere)).is_empty());

    let home = temp();
    mount(&data, "omp", Some(&project), Mount::Home(&home)).unwrap();
    assert_eq!(
        std::fs::read_link(home.join("skills/learned-abc123")).unwrap(),
        skill
    );
    let plugin = mount(&data, "claude", Some(&project), Mount::ClaudePlugin)
        .unwrap()
        .unwrap();
    assert!(plugin.join("skills/learned-abc123/SKILL.md").is_file());
    git_init(&project);
    mount(&data, "codex", Some(&project), Mount::CodexProject).unwrap();
    let copy = project.join(".agents/skills/learned-abc123");
    assert_eq!(
        std::fs::read_to_string(copy.join(MARKER)).unwrap(),
        "learned-v2"
    );
    // Elsewhere: not offered, and the link goes away.
    let other_home = temp();
    mount(&data, "omp", Some(&elsewhere), Mount::Home(&other_home)).unwrap();
    assert!(!other_home.join("skills/learned-abc123").exists());
}

/// CU 2026-10-31: a configured Codex server is shared by every project, so
/// its home gets no learned skill; each session's project gets its own as a
/// marked copy, next to (not instead of) the built-in copies.
#[test]
fn a_configured_codex_gets_learned_skills_in_its_project_only() {
    let data = temp();
    let project = temp();
    git_init(&project);
    let skill = learned_dir(&data).join("learned-cfg001").join("v1");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "---\nname: learned-cfg001\n---\n").unwrap();
    write_learned_index(
        &data,
        &LearnedIndex {
            skills: vec![LearnedEntry {
                name: "learned-cfg001".into(),
                dir: skill.clone(),
                version: 1,
                global: false,
                project_path: Some(project.clone()),
            }],
        },
    )
    .unwrap();
    let home = temp();
    mount(&data, "codex", None, Mount::Home(&home)).unwrap();
    assert!(
        !home.join("skills/learned-cfg001").exists(),
        "a shared home gets none"
    );
    // A direct Codex session left built-in copies here.
    mount(&data, "codex", Some(&project), Mount::CodexProject).unwrap();
    let builtin = project_copies(&project).len();
    mount(&data, "codex", Some(&project), Mount::CodexProjectLearned).unwrap();
    let copy = project.join(".agents/skills/learned-cfg001");
    assert_eq!(
        std::fs::read_to_string(copy.join(MARKER)).unwrap(),
        "learned-v1"
    );
    assert_eq!(
        project_copies(&project).len(),
        builtin,
        "the built-in copies stay"
    );
    // Disabled (out of the index): the learned copy goes, the rest stays.
    write_learned_index(&data, &LearnedIndex::default()).unwrap();
    mount(&data, "codex", Some(&project), Mount::CodexProjectLearned).unwrap();
    assert!(!copy.exists());
    assert_eq!(project_copies(&project).len(), builtin - 1);
}

/// C 批 (2026-11-08): an update is swapped in whole; a copy someone edited
/// goes to the Trash instead of being lost; a copy that fails halfway
/// leaves the old one as it was, and the next sync finishes the job.
#[test]
fn copies_are_replaced_whole_and_edited_ones_go_to_the_trash() {
    use std::os::unix::fs::PermissionsExt;
    let project = temp();
    git_init(&project);
    let source = temp();
    std::fs::write(source.join("SKILL.md"), "v1").unwrap();
    let offer = |version: &str| Offer {
        id: "probe".into(),
        dir: source.clone(),
        version: version.into(),
    };
    let trashed = std::sync::Mutex::new(Vec::new());
    let trash = |path: &Path| {
        trashed
            .lock()
            .unwrap()
            .push(std::fs::read_to_string(path.join("SKILL.md")).unwrap());
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())
    };
    let copy = project.join(".agents/skills/probe");

    sync_offers(&project, &[offer("v1")], &trash).unwrap();
    sync_offers(&project, &[offer("v2")], &trash).unwrap();
    assert!(
        trashed.lock().unwrap().is_empty(),
        "an untouched copy is just replaced"
    );

    std::fs::write(copy.join("SKILL.md"), "我改过").unwrap();
    sync_offers(&project, &[offer("v3")], &trash).unwrap();
    assert_eq!(*trashed.lock().unwrap(), vec!["我改过".to_string()]);
    assert_eq!(
        std::fs::read_to_string(copy.join("SKILL.md")).unwrap(),
        "v1"
    );
    assert_eq!(std::fs::read_to_string(copy.join(MARKER)).unwrap(), "v3");

    let locked = source.join("locked.md");
    std::fs::write(&locked, "x").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(sync_offers(&project, &[offer("v4")], &trash).is_err());
    assert_eq!(
        std::fs::read_to_string(copy.join(MARKER)).unwrap(),
        "v3",
        "the old copy stays"
    );
    assert!(!project.join(".agents/.aeroric-staging").exists());
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    sync_offers(&project, &[offer("v4")], &trash).unwrap();
    assert_eq!(std::fs::read_to_string(copy.join(MARKER)).unwrap(), "v4");
    assert!(copy.join("locked.md").is_file());
    assert!(project_copies(&project).contains("probe"));
}

/// A copy someone edited, whose way to the Trash fails, is kept where it
/// waited rather than cleared with the staging area (夜间自查 2026-11-09).
#[test]
fn an_edited_copy_that_cannot_go_to_the_trash_is_kept() {
    let project = temp();
    git_init(&project);
    let source = temp();
    std::fs::write(source.join("SKILL.md"), "v1").unwrap();
    let offer = |version: &str| Offer {
        id: "probe".into(),
        dir: source.clone(),
        version: version.into(),
    };
    let keep = |_: &Path| Ok(());
    sync_offers(&project, &[offer("v1")], &keep).unwrap();
    std::fs::write(project.join(".agents/skills/probe/SKILL.md"), "我改过").unwrap();
    let no_trash = |_: &Path| Err("废纸篓不可用".to_string());
    let error = sync_offers(&project, &[offer("v2")], &no_trash).unwrap_err();
    assert!(error.contains("改过的旧副本留在"), "{error}");
    let kept = error.rsplit("留在 ").next().unwrap();
    assert_eq!(
        std::fs::read_to_string(Path::new(kept).join("SKILL.md")).unwrap(),
        "我改过"
    );
}
