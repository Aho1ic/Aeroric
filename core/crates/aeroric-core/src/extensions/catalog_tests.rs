//! All in temporary folders; nothing reads the real home or a real project.

use std::path::{Path, PathBuf};

use aeroric_protocol::skills::{MountReason, SkillSource};

use super::*;
use crate::extensions::builtin_skills::{
    self as builtin, LearnedEntry, LearnedIndex, SkillSettings,
};

fn temp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aeroric-catalog-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn skill(dir: &Path, description: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: x\ndescription: \"{description}\"\n---\n\n# body\n"),
    )
    .unwrap();
}

fn learned(data: &Path, name: &str, version: u32, project: Option<&Path>) {
    let dir = builtin::learned_dir(data)
        .join(name)
        .join(format!("v{version}"));
    skill(&dir, &format!("{name} 第 {version} 版"));
    builtin::write_learned_index(
        data,
        &LearnedIndex {
            skills: vec![LearnedEntry {
                name: name.into(),
                dir,
                version,
                global: project.is_none(),
                project_path: project.map(Path::to_path_buf),
            }],
        },
    )
    .unwrap();
}

/// Four sources, each projected with its own version, hash and place; a
/// built-in skill lives in the App and says so.
#[test]
fn every_source_is_listed_with_its_version() {
    let (data, home, project) = (temp(), temp(), temp());
    learned(&data, "csv-bom", 2, None);
    skill(&home.join(".claude/skills/my-review"), "我自己的审查清单");
    let plugin_root = home.join("plugin-cache/tools/1.2.0");
    skill(&plugin_root.join("skills/lint"), "插件里的 lint");
    std::fs::create_dir_all(home.join(".claude/plugins")).unwrap();
    std::fs::write(
        home.join(".claude/plugins/installed_plugins.json"),
        serde_json::to_vec(&serde_json::json!({
            "version": 2,
            "plugins": {"tools@market": [{"installPath": plugin_root, "version": "1.2.0"}]}
        }))
        .unwrap(),
    )
    .unwrap();

    let all = catalog(&data, Some(&project), Some(&home));
    let of = |source| all.iter().filter(move |s| s.source == source);
    assert_eq!(of(SkillSource::Builtin).count(), builtin::catalog().len());
    let first = of(SkillSource::Builtin).next().unwrap();
    assert!(first.embedded && first.path.is_none() && first.active);
    assert_eq!(first.version, BUILTIN_VERSION);
    assert_eq!(first.content_hash, builtin::content_hash());

    let csv = of(SkillSource::Learned).next().unwrap();
    assert_eq!((csv.id.as_str(), csv.version.as_str()), ("csv-bom", "v2"));
    assert_eq!(csv.summary, "csv-bom 第 2 版");
    assert_eq!(csv.content_hash.len(), 12);
    assert!(csv.active);

    let mine = of(SkillSource::User).next().unwrap();
    assert_eq!(mine.id, "my-review");
    assert_eq!(mine.harnesses, vec!["claude"]);
    assert_eq!(mine.summary, "我自己的审查清单");

    let lint = of(SkillSource::Plugin).next().unwrap();
    assert_eq!(
        (lint.id.as_str(), lint.version.as_str()),
        ("tools:lint", "1.2.0")
    );

    // What a Claude launch is given: all four; Codex gets no plugin.
    let claude = plan(&data, "claude", Some(&project), Some(&home));
    let reasons: BTreeSet<(SkillSource, MountReason)> =
        claude.iter().map(|m| (m.source, m.reason)).collect();
    assert_eq!(
        reasons,
        BTreeSet::from([
            (SkillSource::Builtin, MountReason::Default),
            (SkillSource::Learned, MountReason::Accepted),
            (SkillSource::User, MountReason::UserFolder),
            (SkillSource::Plugin, MountReason::Plugin),
        ])
    );
    let codex = plan(&data, "codex", Some(&project), Some(&home));
    assert!(codex
        .iter()
        .all(|m| m.source != SkillSource::Plugin && m.source != SkillSource::User));
}

/// A name in two sources is listed twice; each says which wins, and what a
/// launch is given follows the same rule as mounting.
#[test]
fn a_name_in_two_sources_says_which_wins() {
    let (data, home, project) = (temp(), temp(), temp());
    skill(
        &home.join(".claude/skills/brainstorming"),
        "我改过的头脑风暴",
    );
    learned(&data, "systematic-debugging", 1, None);
    let mut off = SkillSettings::default();
    off.disabled.insert("executing-plans".into());
    builtin::save_settings(&data, &off).unwrap();

    let all = catalog(&data, Some(&project), Some(&home));
    let named = |id: &str| -> Vec<&SkillDefinition> { all.iter().filter(|s| s.id == id).collect() };
    let brainstorming = named("brainstorming");
    assert_eq!(brainstorming.len(), 2);
    let built = brainstorming
        .iter()
        .find(|s| s.source == SkillSource::Builtin)
        .unwrap();
    assert_eq!(
        built.note.as_deref(),
        Some("在 Claude 里被你自己的同名技能盖过")
    );
    assert!(built.active, "the other harnesses still get it");
    assert!(!built.harnesses.contains(&"claude".to_string()));
    let debugging = named("systematic-debugging");
    let shadowed = debugging
        .iter()
        .find(|s| s.source == SkillSource::Learned)
        .unwrap();
    assert!(!shadowed.active);
    assert_eq!(shadowed.note.as_deref(), Some("被同名的内置技能盖过"));
    let plans = named("executing-plans");
    assert!(!plans[0].active);
    assert_eq!(plans[0].note.as_deref(), Some("已关闭"));

    let claude = plan(&data, "claude", Some(&project), Some(&home));
    let source = |id: &str| {
        claude
            .iter()
            .filter(|m| m.skill_id == id)
            .map(|m| m.source)
            .collect::<Vec<_>>()
    };
    assert_eq!(source("brainstorming"), vec![SkillSource::User]);
    assert_eq!(source("systematic-debugging"), vec![SkillSource::Builtin]);
    assert!(source("executing-plans").is_empty());
    let codex = plan(&data, "codex", Some(&project), Some(&home));
    assert_eq!(
        codex
            .iter()
            .filter(|m| m.skill_id == "brainstorming")
            .map(|m| m.source)
            .collect::<Vec<_>>(),
        vec![SkillSource::Builtin]
    );
}

/// A learned skill of one project is listed with where it applies, and a
/// launch elsewhere is not given it.
#[test]
fn a_project_skill_applies_in_its_project_only() {
    let (data, home, project, other) = (temp(), temp(), temp(), temp());
    learned(&data, "deploy-steps", 1, Some(&project));
    let here = catalog(&data, Some(&project), Some(&home));
    let skill = here.iter().find(|s| s.id == "deploy-steps").unwrap();
    assert!(skill.active);
    assert!(skill.note.as_deref().unwrap().starts_with("只在项目 "));
    let there = catalog(&data, Some(&other), Some(&home));
    assert!(
        !there
            .iter()
            .find(|s| s.id == "deploy-steps")
            .unwrap()
            .active
    );
    assert!(plan(&data, "omp", Some(&project), Some(&home))
        .iter()
        .any(|m| m.skill_id == "deploy-steps"));
    assert!(!plan(&data, "omp", Some(&other), Some(&home))
        .iter()
        .any(|m| m.skill_id == "deploy-steps"));
}

/// Each launch keeps what it was given: a skill upgraded mid-session shows
/// the old version for the launch before; a turn reads the launch it ran
/// under; only the last 20 launches stay; no folder, nothing kept.
#[test]
fn each_launch_keeps_the_versions_it_had() {
    let (data, home, project) = (temp(), temp(), temp());
    learned(&data, "csv-bom", 1, None);
    let log = MountLog::default();
    log.note("task-1", "claude", Some(&project), Some(&home), 10);
    assert!(
        log.launches("task-1").is_empty(),
        "no folder set: nothing kept"
    );

    log.set_dir(data.clone());
    log.note("task-1", "claude", Some(&project), Some(&home), 10);
    learned(&data, "csv-bom", 2, None);
    log.note("task-1", "claude", Some(&project), Some(&home), 20);
    let launches = log.launches("task-1");
    let version = |i: usize| {
        launches[i]
            .skills
            .iter()
            .find(|m| m.skill_id == "csv-bom")
            .map(|m| m.version.clone())
    };
    assert_eq!(version(0).as_deref(), Some("v1"));
    assert_eq!(version(1).as_deref(), Some("v2"));
    assert_eq!(launches[0].harness, "claude");
    assert_eq!(log.at("task-1", 15).unwrap().at_ms, 10);
    assert_eq!(log.at("task-1", 25).unwrap().at_ms, 20);
    assert_eq!(
        log.at("task-1", 5).unwrap().at_ms,
        10,
        "none before: the first"
    );
    assert!(log.at("task-2", 5).is_none());
    assert_ne!(
        fingerprint(&launches[0].skills),
        fingerprint(&launches[1].skills)
    );
    assert!(fingerprint(&launches[0].skills)
        .starts_with(&format!("{} 个 · ", launches[0].skills.len())));

    for at in 0..25 {
        log.note("task-1", "codex", Some(&project), Some(&home), 100 + at);
    }
    assert_eq!(log.launches("task-1").len(), 20);
    // An id that is not a plain file name is kept under its hash.
    log.note("../evil", "codex", None, Some(&home), 1);
    assert_eq!(log.launches("../evil").len(), 1);
    assert!(!data.join("evil.json").exists());
    assert_eq!(harness_of("claude-code"), "claude");
    assert_eq!(harness_of("codex"), "codex");
}
