//! Filename enumeration/search parity on isolated folders and local Git indexes.
use aeroric_core::files::project_files::{
    list_project_files, search_project_files, ProjectFileSearchResult,
};
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "common/scratch.rs"]
mod scratch;

struct Project {
    scratch: scratch::Scratch,
    root: PathBuf,
    env: Vec<(String, String)>,
}

impl Project {
    fn new() -> Self {
        let scratch = scratch::Scratch::new("aeroric-project-files");
        let scratch_path = scratch
            .path()
            .canonicalize()
            .expect("resolve scratch directory");
        let root = scratch_path.join("project");
        let home = scratch_path.join("home");
        std::fs::create_dir(&root).expect("create project directory");
        std::fs::create_dir(&home).expect("create isolated Git home");
        let env = vec![
            ("HOME".into(), home.to_string_lossy().into_owned()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
            ("GIT_CONFIG_COUNT".into(), "0".into()),
            ("GIT_CONFIG_PARAMETERS".into(), String::new()),
            (
                "GIT_DIR".into(),
                root.join(".git").to_string_lossy().into_owned(),
            ),
            ("GIT_WORK_TREE".into(), root.to_string_lossy().into_owned()),
            (
                "GIT_INDEX_FILE".into(),
                root.join(".git/index").to_string_lossy().into_owned(),
            ),
        ];
        Self { scratch, root, env }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture parent"))
            .expect("create fixture parent");
        std::fs::write(path, contents).expect("write fixture");
    }

    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .envs(self.env.iter().cloned())
            .output()
            .expect("run Git in scratch project");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn track_files(&self) {
        self.git(&["init", "--quiet"]);
        self.git(&["add", "--", "."]);
    }

    fn search(
        &self,
        query: &str,
        extensions: &[String],
        limit: Option<usize>,
    ) -> Vec<ProjectFileSearchResult> {
        search_project_files(&self.root, query, extensions, limit, "git", &self.env)
            .expect("search scratch project")
    }

    fn relative_paths(&self, results: &[ProjectFileSearchResult]) -> Vec<String> {
        results
            .iter()
            .map(|result| {
                Path::new(&result.path)
                    .strip_prefix(&self.root)
                    .expect("result remains in the project")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }
}

#[test]
fn fallback_project_file_list_skips_common_dependency_directories() {
    let project = Project::new();
    project.write("README.md", "readme");
    project.write("src/main.ts", "main");
    project.write("node_modules/package.js", "dependency");
    project.write(".git/config", "git");
    project.write("BUILD/generated.js", "generated");

    for git_path in [None, Some("")] {
        let files = list_project_files(&project.root, git_path, &project.env)
            .expect("list project files without Git");
        assert_eq!(files, vec!["README.md", "src/main.ts"]);
    }
}

#[test]
fn list_falls_back_when_git_cannot_start_or_the_folder_is_not_a_repository() {
    let project = Project::new();
    project.write("README.md", "readme");
    let missing_git = project.scratch.path().join("missing-git");
    for git_path in [missing_git.to_str().expect("scratch path"), "git"] {
        let files = list_project_files(&project.root, Some(git_path), &project.env)
            .expect("fall back to directory enumeration");
        assert_eq!(files, vec!["README.md"]);
    }
}

#[test]
fn fallback_reports_directory_read_errors() {
    let project = Project::new();
    assert!(list_project_files(&project.root.join("missing"), None, &project.env).is_err());
}

#[test]
fn git_list_includes_untracked_files_respects_ignores_and_sorts_without_duplicates() {
    let project = Project::new();
    project.write(".gitignore", "ignored/\n");
    project.write("README.md", "readme");
    project.write("src/main.ts", "main");
    project.track_files();
    project.write("untracked.txt", "untracked");
    project.write("ignored/cache.txt", "ignored");

    let files = list_project_files(&project.root, Some("git"), &project.env)
        .expect("list tracked and untracked files");
    assert_eq!(
        files,
        vec![".gitignore", "README.md", "src/main.ts", "untracked.txt"]
    );
}

fn ranked_project() -> Project {
    let project = Project::new();
    for relative in [
        "report",
        "a/report.ts",
        "z/report.TS",
        "reporting.md",
        "z-report.txt",
        "report-directory/unrelated.txt",
        "report-missing.ts",
    ] {
        project.write(relative, "fixture");
    }
    project.track_files();
    std::fs::remove_file(project.root.join("report-missing.ts")).expect("remove tracked file");
    project.write("report-untracked.ts", "not in the index");
    project
}

#[test]
fn search_uses_tracked_existing_filenames_and_preserves_rank_and_tie_breaking() {
    let project = ranked_project();
    let results = project.search(" RePoRt ", &[], None);
    assert_eq!(
        project.relative_paths(&results),
        vec![
            "report",
            "a/report.ts",
            "z/report.TS",
            "reporting.md",
            "z-report.txt"
        ]
    );
    assert_eq!(results[0].dir, "");
    assert_eq!(results[0].extension, None);
    assert_eq!(
        project.relative_paths(&project.search("report", &[], Some(2))),
        vec!["report", "a/report.ts"]
    );
    assert!(project.search("not-present", &[], None).is_empty());
}

#[test]
fn search_normalizes_extension_filters_and_keeps_the_original_filename() {
    let project = ranked_project();
    let extensions = vec![" .tS ".into(), ".".into(), " ".into()];
    let results = project.search("report", &extensions, None);
    assert_eq!(
        project.relative_paths(&results),
        vec!["a/report.ts", "z/report.TS"]
    );
    assert!(results
        .iter()
        .all(|row| row.extension.as_deref() == Some("ts")));

    let extensions = vec![".TS".into(), " md ".into()];
    let results = project.search("report", &extensions, None);
    assert_eq!(
        project.relative_paths(&results),
        vec!["a/report.ts", "z/report.TS", "reporting.md"]
    );
}

#[test]
fn search_preserves_default_limit_and_clamps_zero_and_oversized_limits() {
    let project = Project::new();
    for index in 0..205 {
        project.write(&format!("item-{index:03}.ts"), "fixture");
    }
    project.track_files();

    let default = project.search("", &[], None);
    assert_eq!(default.len(), 80);
    assert_eq!(default.first().expect("first result").name, "item-000.ts");
    assert_eq!(default.last().expect("last result").name, "item-079.ts");
    let minimum = project.search("", &[], Some(0));
    assert_eq!(minimum.len(), 1);
    assert_eq!(minimum[0].name, "item-000.ts");
    let maximum = project.search("", &[], Some(usize::MAX));
    assert_eq!(maximum.len(), 200);
    assert_eq!(maximum.last().expect("last result").name, "item-199.ts");
}

#[test]
fn filename_search_and_wire_roundtrip_preserve_spaces_and_unicode() {
    let project = Project::new();
    let relative = "目录 空格/按钮 描述.TS";
    project.write(relative, "fixture");
    project.track_files();

    let files = list_project_files(&project.root, Some("git"), &project.env)
        .expect("list Unicode filename");
    assert_eq!(files, vec![relative]);
    let results = project.search("按钮", &[".ts".into()], None);
    assert_eq!(project.relative_paths(&results), vec![relative]);
    let result = results.into_iter().next().expect("Unicode result");
    let value = serde_json::to_value(result.clone()).expect("serialize search result");
    assert_eq!(
        value,
        serde_json::json!({
            "path": project.root.join(relative).to_string_lossy(),
            "name": "按钮 描述.TS",
            "dir": "目录 空格",
            "extension": "ts",
        })
    );
    let decoded: ProjectFileSearchResult =
        serde_json::from_value(value.clone()).expect("deserialize search result");
    assert_eq!(
        serde_json::to_value(decoded).expect("serialize roundtrip"),
        value
    );
}

#[test]
fn search_reports_git_failures_instead_of_falling_back() {
    let project = Project::new();
    project.write("README.md", "readme");
    let missing_git = project.scratch.path().join("missing-git");
    for git_path in [missing_git.to_str().expect("scratch path"), "git"] {
        assert!(
            search_project_files(&project.root, "", &[], None, git_path, &project.env).is_err()
        );
    }
    project.track_files();
    assert!(search_project_files(
        &project.root,
        "",
        &[],
        None,
        missing_git.to_str().expect("scratch path"),
        &project.env,
    )
    .is_err());
    assert_eq!(
        project.relative_paths(&project.search("", &[], None)),
        vec!["README.md"]
    );
}

#[cfg(unix)]
#[test]
fn fallback_does_not_follow_file_directory_or_cyclic_symlinks() {
    use std::os::unix::fs::symlink;

    let project = Project::new();
    project.write("src/inside.ts", "inside");
    let outside = project.scratch.path().join("outside");
    std::fs::create_dir(&outside).expect("create outside fixture directory");
    std::fs::write(outside.join("secret.ts"), "outside").expect("write outside fixture");
    symlink(&outside, project.root.join("external")).expect("link outside directory");
    symlink(outside.join("secret.ts"), project.root.join("escape.ts")).expect("link outside file");
    symlink(
        project.root.join("src/inside.ts"),
        project.root.join("alias.ts"),
    )
    .expect("link inside file");
    symlink(&project.root, project.root.join("cycle")).expect("link project cycle");

    let files = list_project_files(&project.root, None, &project.env)
        .expect("enumerate without following symlinks");
    assert_eq!(files, vec!["src/inside.ts"]);
}

#[cfg(unix)]
#[test]
fn search_skips_canonical_escapes_but_keeps_in_root_file_symlinks() {
    use std::os::unix::fs::symlink;

    let project = Project::new();
    project.write("safe.ts", "inside");
    project.write("nested/inside.ts", "initially inside");
    let outside = project.scratch.path().join("outside");
    std::fs::create_dir(&outside).expect("create outside fixture directory");
    std::fs::write(outside.join("inside.ts"), "outside").expect("write outside fixture");
    symlink(outside.join("inside.ts"), project.root.join("escape.ts")).expect("link outside file");
    symlink(project.root.join("safe.ts"), project.root.join("alias.ts")).expect("link inside file");
    project.track_files();
    std::fs::remove_dir_all(project.root.join("nested")).expect("remove indexed directory");
    symlink(&outside, project.root.join("nested")).expect("replace indexed directory with symlink");
    assert!(project.root.join("escape.ts").is_file());
    assert!(project.root.join("nested/inside.ts").is_file());

    let results = project.search("", &["ts".into()], None);
    assert_eq!(
        project.relative_paths(&results),
        vec!["alias.ts", "safe.ts"]
    );
}
