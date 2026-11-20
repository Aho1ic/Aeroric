use super::*;
use crate::sessions::live::delegation::roles::builtin;

fn temp() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aeroric-native-roles-{}", uuid::Uuid::new_v4()))
}

/// Of the built-ins only the researcher fits a native subagent: the
/// reviewer and ideator want the other family, the implementer and tester
/// an isolated copy, the expert a stronger model.
#[test]
fn only_roles_that_fit_a_native_subagent_are_projected() {
    let projected: Vec<String> = builtin()
        .into_iter()
        .filter(|r| not_projected(r).is_none())
        .map(|r| r.id)
        .collect();
    assert_eq!(projected, ["researcher"]);
    let why =
        |id: &str| not_projected(&builtin().into_iter().find(|r| r.id == id).unwrap()).unwrap();
    assert!(why("reviewer").contains("换一种 Harness"));
    assert!(why("implementer").contains("隔离的可写副本"));
    assert!(why("expert").contains("更强的模型"));

    let mut budgeted = builtin()
        .into_iter()
        .find(|r| r.id == "researcher")
        .unwrap();
    budgeted.budget.minutes = Some(5);
    assert!(not_projected(&budgeted).unwrap().contains("预算"));
    let mut shared = budgeted.clone();
    shared.budget = Default::default();
    shared.id = "fixer".into();
    shared.workspace = Workspace::Shared;
    shared.permission = "auto_edit".into();
    assert_eq!(
        not_projected(&shared),
        None,
        "a writable role in place is fine"
    );
}

/// A read-only role is held read-only by a tool list in each harness's
/// own words; a writable one keeps the parent's tools.
#[test]
fn a_definition_holds_its_permission_where_the_harness_can() {
    let researcher = builtin()
        .into_iter()
        .find(|r| r.id == "researcher")
        .unwrap();
    let claude = definition(Target::ClaudePlugin, &researcher);
    assert!(claude.starts_with("---\nname: researcher\n"), "{claude}");
    assert!(claude.contains("description: \"调研员（Aeroric 角色库）：只读调研"));
    let tools = claude.lines().find(|l| l.starts_with("tools: ")).unwrap();
    assert!(tools.contains("Read, Grep, Glob"));
    assert!(tools.contains("mcp__aeroric_context__memory_recall"));
    assert!(!tools.contains("Edit") && !tools.contains("Bash") && !tools.contains("delegate"));
    assert!(
        tools.contains("mcp__aeroric_context__board_write"),
        "it reports on the board"
    );
    assert!(!tools.contains("board_decide"));
    assert!(claude.contains(MARKER));
    assert!(claude.contains(researcher.instruction.trim()));
    assert!(
        claude
            .trim_end()
            .ends_with("subagent 填“aeroric:researcher”，黑板上会标明是你写的。"),
        "{claude}"
    );

    let omp = definition(Target::OmpHome, &researcher);
    assert!(
        omp.starts_with("---\nname: aeroric-researcher\n"),
        "a bundled name stays OMP's"
    );
    assert!(omp.contains("tools: read, grep, glob, ast_grep, think\n"));
    assert!(omp.contains("subagent 填“aeroric-researcher”"));
    assert_eq!(name(Target::ClaudeHome, &researcher), "aeroric-researcher");

    let mut writer = researcher.clone();
    writer.permission = "auto_edit".into();
    writer.workspace = Workspace::Shared;
    assert!(!definition(Target::ClaudePlugin, &writer).contains("\ntools:"));
    let mut quoted = researcher;
    quoted.description = "带 \"引号\" 和\n换行".into();
    assert!(definition(Target::OmpHome, &quoted).contains("带 \\\"引号\\\" 和 换行"));
}

/// The folder holds exactly the projectable roles as Aeroric's files; a
/// role that stops fitting is removed, someone else's file stays.
#[test]
fn the_folder_keeps_only_aerorics_current_roles() {
    let dir = temp();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mine.md"), "---\nname: mine\n---\nthe user's").unwrap();
    std::fs::write(
        dir.join("aeroric-old.md"),
        format!("---\nname: aeroric-old\n---\n{MARKER}\nx"),
    )
    .unwrap();
    let written = write(&dir, Target::OmpHome, &builtin()).unwrap();
    assert_eq!(written, ["aeroric-researcher"]);
    assert!(dir.join("aeroric-researcher.md").is_file());
    assert!(
        !dir.join("aeroric-old.md").exists(),
        "a stale one of ours goes"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("mine.md")).unwrap(),
        "---\nname: mine\n---\nthe user's"
    );
    assert!(!dir.join("aeroric-reviewer.md").exists());

    // Nothing projectable: our files go, the folder is not made anew.
    write(&dir, Target::OmpHome, &[]).unwrap();
    assert!(!dir.join("aeroric-researcher.md").exists());
    assert!(dir.join("mine.md").exists());
    let none = temp();
    assert!(write(&none, Target::OmpHome, &[]).unwrap().is_empty());
    assert!(!none.exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// Mounting the skills for Claude (plugin) and OMP (managed home) puts the
/// projected roles beside them; Codex and DSH get none.
#[test]
fn mounting_gives_claude_and_omp_the_roles() {
    use crate::extensions::builtin_skills::{mount, Mount};
    let data = temp();
    let plugin = mount(&data, "claude", None, Mount::ClaudePlugin)
        .unwrap()
        .unwrap();
    assert!(plugin.join("agents/researcher.md").is_file());
    let omp_home = temp();
    mount(&data, "omp", None, Mount::Home(&omp_home)).unwrap();
    assert!(omp_home.join("agents/aeroric-researcher.md").is_file());
    let codex_home = temp();
    mount(&data, "codex", None, Mount::Home(&codex_home)).unwrap();
    assert!(!codex_home.join("agents").exists());
    for dir in [&data, &omp_home, &codex_home] {
        std::fs::remove_dir_all(dir).ok();
    }
}
