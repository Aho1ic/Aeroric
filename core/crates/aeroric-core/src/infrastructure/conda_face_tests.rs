use super::*;
use serde_json::json;

// These local policy tests moved with their bodies from the desktop.
#[test]
fn env_name_uses_last_path_segment() {
    assert_eq!(env_name_from_path(Path::new("/opt/miniconda3")), "base");
    assert_eq!(
        env_name_from_path(Path::new("/opt/miniconda3/envs/cv")),
        "cv"
    );
}

#[cfg(not(windows))]
#[test]
fn conda_candidates_include_default_miniconda_on_macos() {
    assert!(candidates_with_env(|_| None)
        .iter()
        .any(|candidate| candidate == Path::new("/opt/miniconda3/bin/conda")));
}

#[test]
fn parses_expected_miniconda_environment_names() {
    let raw = br#"{"envs":["/opt/miniconda3","/opt/miniconda3/envs/codex",
        "/opt/miniconda3/envs/detect","/opt/miniconda3/envs/kiro",
        "/opt/miniconda3/envs/labelimg","/opt/miniconda3/envs/labelme",
        "/opt/miniconda3/envs/mahjong"]}"#;
    let names: Vec<_> = parse_conda_envs_with_python_exists(raw, |_| true)
        .into_iter()
        .map(|env| env.name)
        .collect();
    assert_eq!(
        names,
        ["base", "codex", "detect", "kiro", "labelimg", "labelme", "mahjong"]
    );
}

#[cfg(windows)]
#[test]
fn windows_conda_environments_use_python_exe() {
    let raw =
        br#"{"envs":["C:\\Users\\test\\miniconda3","C:\\Users\\test\\miniconda3\\envs\\cv"]}"#;
    let envs = parse_conda_envs_with_python_exists(raw, |path| {
        path.file_name().and_then(|name| name.to_str()) == Some("python.exe")
    });
    assert_eq!(envs.len(), 2);
    assert!(envs
        .iter()
        .all(|env| env.python_path.ends_with("python.exe")));
}

#[test]
fn explicit_executable_including_empty_is_first_without_mutating_env() {
    for value in ["", "/owned path/Conda"] {
        let paths = candidates_with_env(|key| (key == "CONDA_EXE").then(|| value.into()));
        assert_eq!(paths[0], Path::new(value));
        assert!(paths.len() > 1);
    }
}

#[cfg(not(windows))]
#[test]
fn candidate_order_includes_all_three_home_installs_last() {
    let paths = candidates_with_env(|key| match key {
        "HOME" => Some("/owned home/中文".into()),
        "CONDA_EXE" => Some("/explicit".into()),
        _ => None,
    });
    assert_eq!(
        paths,
        [
            "/explicit",
            "/opt/miniconda3/bin/conda",
            "/opt/homebrew/bin/conda",
            "/usr/local/bin/conda",
            "conda",
            "/owned home/中文/miniconda3/bin/conda",
            "/owned home/中文/anaconda3/bin/conda",
            "/owned home/中文/mambaforge/bin/conda",
        ]
        .map(PathBuf::from)
    );
}

#[cfg(windows)]
#[test]
fn windows_candidate_order_includes_each_root_and_case() {
    let paths = candidates_with_env(|key| match key {
        "CONDA_EXE" => Some("explicit.exe".into()),
        "USERPROFILE" => Some(r"C:\owned-user".into()),
        "LOCALAPPDATA" => Some(r"C:\owned-local".into()),
        "ProgramData" => Some(r"C:\owned-data".into()),
        _ => None,
    });
    let mut expected = vec![PathBuf::from("explicit.exe")];
    for root in [r"C:\owned-user", r"C:\owned-local", r"C:\owned-data"] {
        for name in ["miniconda3", "anaconda3", "Miniconda3", "Anaconda3"] {
            expected.push(Path::new(root).join(name).join("Scripts/conda.exe"));
        }
    }
    expected.extend([
        PathBuf::from(r"C:\ProgramData\miniconda3\Scripts\conda.exe"),
        PathBuf::from(r"C:\ProgramData\anaconda3\Scripts\conda.exe"),
        PathBuf::from("conda.exe"),
    ]);
    assert_eq!(paths, expected);
}

#[test]
fn parser_preserves_order_deduplicates_and_checks_python_once() {
    let raw = br#"{"envs":["/base/envs/z","/base","/base/envs/z",false,
        "/base/envs/no-python",null,"/base/envs/a"]}"#;
    let checked = std::cell::RefCell::new(Vec::new());
    let envs = parse_conda_envs_with_python_exists(raw, |path| {
        checked.borrow_mut().push(path.to_path_buf());
        path != python_path_for_env(Path::new("/base/envs/no-python"))
    });
    assert_eq!(
        envs.iter().map(|env| env.name.as_str()).collect::<Vec<_>>(),
        ["z", "base", "a"]
    );
    assert_eq!(checked.borrow().len(), 4);
    let value = serde_json::to_value(&envs).unwrap();
    assert_eq!(
        value[0],
        json!({
            "name": "z", "path": "/base/envs/z",
            "pythonPath": python_path_for_env(Path::new("/base/envs/z")),
        })
    );
    assert_eq!(
        serde_json::from_value::<Vec<CondaEnvironment>>(value).unwrap(),
        envs
    );
}

#[test]
fn malformed_or_missing_env_lists_are_empty_without_file_checks() {
    for raw in [
        b"not json".as_slice(),
        b"{}",
        b"[]",
        b"null",
        b"{\"envs\":{}}",
        b"{\"envs\":[1,null,false]}",
        b"{\"envs\":[]}",
    ] {
        assert!(parse_conda_envs_with_python_exists(raw, |_| {
            panic!("invalid/empty listing must not inspect Python paths")
        })
        .is_empty());
    }
}
