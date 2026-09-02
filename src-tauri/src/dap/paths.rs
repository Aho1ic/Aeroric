//! 调试配置的路径解析、校验与读写。
//!
//! 从 `dap.rs` 整块搬出来,内容一行没改。单独成文件是因为这里**全是安全边界**:
//! 用户在调试配置里填的 program / cwd / 断点文件路径都要先钉死在项目根目录内,
//! 否则调试器会被用来读写项目外的文件。本地一套(`ensure_path_inside_root` /
//! `candidate_path` 走 `normalize_path_lexically`,不跟随符号链接地做词法归一)、
//! 远程一套(`remote_debug_path_*`,纯字符串判断,因为拿不到远端文件系统)。
//!
//! `validate_*` 几个是对外的入口,`read/write_debug_configs_from_root` 负责
//! `.aeroric/debug.json` 的实际读写。

use super::*;

pub(super) fn debug_configs_path(root: &Path) -> PathBuf {
    root.join(".aeroric").join("debug-configs.json")
}

pub(super) fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub(super) fn ensure_path_inside_root(root: &Path, path: &Path) -> Result<(), String> {
    let normalized_root = normalize_path_lexically(root);
    let normalized_path = normalize_path_lexically(path);
    if normalized_path.starts_with(&normalized_root) {
        Ok(())
    } else {
        Err("Path is outside project root".to_string())
    }
}

pub(super) fn candidate_path(root: &Path, value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Path cannot be empty".to_string());
    }
    let path = Path::new(trimmed);
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    })
}

pub(super) fn remote_debug_path_has_relative_components(path: &str) -> bool {
    path.split('/')
        .any(|component| component == "." || component == "..")
}

pub(super) fn normalize_remote_debug_root(remote_project_path: &str) -> Result<String, String> {
    let trimmed = remote_project_path.trim();
    if !trimmed.starts_with('/') {
        return Err("Remote project path must be absolute".to_string());
    }
    if trimmed.contains('\0') || remote_debug_path_has_relative_components(trimmed) {
        return Err("Remote project path cannot contain . or .. components".to_string());
    }
    Ok(if trimmed == "/" {
        "/".to_string()
    } else {
        trimmed.trim_end_matches('/').to_string()
    })
}

pub(super) fn remote_debug_path_is_inside_root(root: &str, path: &str) -> bool {
    root == "/" || path == root || path.starts_with(&format!("{root}/"))
}

pub(super) fn join_remote_debug_path(
    root: &str,
    value: &str,
    label: &str,
) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }
    if trimmed == "." && label == "Debug cwd" {
        return Ok(root.to_string());
    }
    if trimmed.contains('\0') || remote_debug_path_has_relative_components(trimmed) {
        return Err(format!("{label} cannot contain . or .. components"));
    }
    let path = if trimmed.starts_with('/') {
        if trimmed == "/" {
            "/".to_string()
        } else {
            trimmed.trim_end_matches('/').to_string()
        }
    } else if root == "/" {
        format!("/{}", trimmed.trim_matches('/'))
    } else {
        format!(
            "{}/{}",
            root.trim_end_matches('/'),
            trimmed.trim_matches('/')
        )
    };
    if !remote_debug_path_is_inside_root(root, &path) {
        return Err(format!("{label} is outside project root"));
    }
    Ok(path)
}

pub(super) fn remote_debug_config_path(root: &str) -> String {
    if root == "/" {
        "/.aeroric/debug-configs.json".to_string()
    } else {
        format!("{}/.aeroric/debug-configs.json", root.trim_end_matches('/'))
    }
}

pub(super) fn validate_remote_breakpoint(
    root: &str,
    breakpoint: &DebugBreakpoint,
) -> Result<(), String> {
    if breakpoint.line == 0 {
        return Err("Breakpoint line must be at least 1".to_string());
    }
    if breakpoint.column == 0 {
        return Err("Breakpoint column must be at least 1".to_string());
    }
    if breakpoint
        .condition
        .as_deref()
        .is_some_and(|condition| condition.trim().is_empty())
    {
        return Err("Breakpoint condition cannot be empty".to_string());
    }
    if breakpoint
        .log_message
        .as_deref()
        .is_some_and(|message| message.trim().is_empty())
    {
        return Err("Breakpoint log message cannot be empty".to_string());
    }
    join_remote_debug_path(root, &breakpoint.file, "Breakpoint file").map(|_| ())
}

pub(super) fn validate_remote_debug_config(root: &str, config: &DebugConfig) -> Result<(), String> {
    if config.id.trim().is_empty() {
        return Err("Debug config id cannot be empty".to_string());
    }
    if config.name.trim().is_empty() {
        return Err("Debug config name cannot be empty".to_string());
    }
    join_remote_debug_path(root, &config.cwd, "Debug cwd")?;
    match config.request {
        DebugRequestType::Launch => {
            if config.program.trim().is_empty() {
                return Err("Debug program cannot be empty".to_string());
            }
            join_remote_debug_path(root, &config.program, "Debug program")?;
        }
        DebugRequestType::Attach => {
            validate_attach_endpoint(&config.attach_host, config.attach_port)?;
            if !config.program.trim().is_empty() {
                join_remote_debug_path(root, &config.program, "Debug program")?;
            }
        }
    }
    for breakpoint in &config.breakpoints {
        validate_remote_breakpoint(root, breakpoint)?;
    }
    Ok(())
}

pub(super) fn validate_remote_debug_configs(
    root: &str,
    document: &DebugConfigDocument,
) -> Result<(), String> {
    for config in &document.configs {
        validate_remote_debug_config(root, config)?;
    }
    Ok(())
}

pub(super) fn validate_breakpoint(root: &Path, breakpoint: &DebugBreakpoint) -> Result<(), String> {
    if breakpoint.line == 0 {
        return Err("Breakpoint line must be at least 1".to_string());
    }
    if breakpoint.column == 0 {
        return Err("Breakpoint column must be at least 1".to_string());
    }
    if breakpoint
        .condition
        .as_deref()
        .is_some_and(|condition| condition.trim().is_empty())
    {
        return Err("Breakpoint condition cannot be empty".to_string());
    }
    if breakpoint
        .log_message
        .as_deref()
        .is_some_and(|message| message.trim().is_empty())
    {
        return Err("Breakpoint log message cannot be empty".to_string());
    }
    let candidate = candidate_path(root, &breakpoint.file)?;
    ensure_path_inside_root(root, &candidate)
}

pub(super) fn validate_debug_config(root: &Path, config: &DebugConfig) -> Result<(), String> {
    if config.id.trim().is_empty() {
        return Err("Debug config id cannot be empty".to_string());
    }
    if config.name.trim().is_empty() {
        return Err("Debug config name cannot be empty".to_string());
    }
    let cwd = candidate_path(root, &config.cwd)?;
    ensure_path_inside_root(root, &cwd)?;
    match config.request {
        DebugRequestType::Launch => {
            if config.program.trim().is_empty() {
                return Err("Debug program cannot be empty".to_string());
            }
            let program = candidate_path(root, &config.program)?;
            ensure_path_inside_root(root, &program)?;
        }
        DebugRequestType::Attach => {
            validate_attach_endpoint(&config.attach_host, config.attach_port)?;
            if !config.program.trim().is_empty() {
                let program = candidate_path(root, &config.program)?;
                ensure_path_inside_root(root, &program)?;
            }
        }
    }
    for breakpoint in &config.breakpoints {
        validate_breakpoint(root, breakpoint)?;
    }
    Ok(())
}

pub(super) fn validate_attach_endpoint(host: &str, port: Option<u16>) -> Result<(), String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("Attach host cannot be empty".to_string());
    }
    if host
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '/' | '\\'))
    {
        return Err("Attach host cannot contain whitespace or URL separators".to_string());
    }
    match port {
        Some(port) if port > 0 => Ok(()),
        _ => Err("Attach port must be between 1 and 65535".to_string()),
    }
}

pub(super) fn resolve_debug_cwd(root: &Path, cwd: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    let candidate = candidate_path(&root, cwd)?;
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Cannot resolve debug cwd: {e}"))?;
    if !canonical.is_dir() {
        return Err("Debug cwd is not a directory".to_string());
    }
    if !canonical.starts_with(root) {
        return Err("Debug cwd is outside project root".to_string());
    }
    Ok(canonical)
}

pub(super) fn resolve_debug_program(root: &Path, program: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    let candidate = candidate_path(&root, program)?;
    let canonical = candidate
        .canonicalize()
        .map_err(|e| format!("Cannot resolve debug program: {e}"))?;
    if !canonical.is_file() {
        return Err("Debug program is not a file".to_string());
    }
    if !canonical.starts_with(root) {
        return Err("Debug program is outside project root".to_string());
    }
    Ok(canonical)
}

pub(super) fn read_debug_configs_from_root(root: &Path) -> Result<DebugConfigDocument, String> {
    let path = debug_configs_path(root);
    if !path.exists() {
        return Ok(DebugConfigDocument::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut document: DebugConfigDocument =
        serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if document.version == 0 {
        document.version = DEBUG_CONFIG_VERSION;
    }
    for config in &document.configs {
        validate_debug_config(root, config)?;
    }
    Ok(document)
}

pub(super) fn write_debug_configs_from_root(
    root: &Path,
    mut document: DebugConfigDocument,
) -> Result<DebugConfigDocument, String> {
    document.version = DEBUG_CONFIG_VERSION;
    for config in &document.configs {
        validate_debug_config(root, config)?;
    }
    let path = debug_configs_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(&document).map_err(|e| e.to_string())?;
    crate::storage::atomic_write(&path, &raw)?;
    Ok(document)
}

/// 测试用的一次性临时目录。**放在模块层而不是 `tests` 里**:父模块测试块
/// 还有 2 个断点解析用例在用,提上来让两边共用一份定义。
#[cfg(test)]
pub(super) fn unique_test_dir(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("aeroric-debug-config-test-{name}-{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn reads_missing_debug_configs_as_empty_document() {
        let root = unique_test_dir("missing");
        fs::create_dir_all(&root).unwrap();

        let document = read_debug_configs_from_root(&root).unwrap();

        assert_eq!(document.version, 1);
        assert!(document.configs.is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_python_debug_configs_from_aeroric_directory() {
        let root = unique_test_dir("read-python");
        fs::create_dir_all(root.join(".aeroric")).unwrap();
        fs::write(
            root.join(".aeroric").join("debug-configs.json"),
            r#"{
              "version": 1,
              "configs": [
                {
                  "id": "py",
                  "name": "Python",
                  "type": "python",
                  "program": "app/main.py",
                  "cwd": ".",
                  "args": ["--port", "8000"],
                  "env": { "PYTHONPATH": "." },
                  "breakpoints": [
                    { "file": "app/main.py", "line": 3, "column": 1 }
                  ]
                }
              ]
            }"#,
        )
        .unwrap();

        let document = read_debug_configs_from_root(&root).unwrap();

        assert_eq!(document.configs.len(), 1);
        assert_eq!(document.configs[0].config_type, DebugConfigType::Python);
        assert_eq!(document.configs[0].request, DebugRequestType::Launch);
        assert_eq!(document.configs[0].program, "app/main.py");
        assert_eq!(document.configs[0].attach_host, "127.0.0.1");
        assert_eq!(document.configs[0].attach_port, None);
        assert_eq!(document.configs[0].args, vec!["--port", "8000"]);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_debug_program_outside_project_root() {
        let root = Path::new("/repo");
        let config = DebugConfig {
            id: "debug".to_string(),
            name: "Debug".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Launch,
            program: "../outside.js".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec![],
            env: Default::default(),
            breakpoints: vec![],
        };

        let error = validate_debug_config(root, &config).unwrap_err();

        assert!(error.contains("outside project root"));
    }

    #[test]
    fn rejects_breakpoint_outside_project_root() {
        let root = Path::new("/repo");
        let config = DebugConfig {
            id: "debug".to_string(),
            name: "Debug".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Launch,
            program: "src/index.js".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec![],
            env: Default::default(),
            breakpoints: vec![DebugBreakpoint {
                file: "../outside.js".to_string(),
                line: 3,
                column: 1,
                condition: None,
                log_message: None,
            }],
        };

        let error = validate_debug_config(root, &config).unwrap_err();

        assert!(error.contains("outside project root"));
    }

    #[test]
    fn validates_node_attach_config_without_program() {
        let root = Path::new("/repo");
        let config = DebugConfig {
            id: "attach".to_string(),
            name: "Attach".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(9229),
            args: vec![],
            env: Default::default(),
            breakpoints: vec![],
        };

        validate_debug_config(root, &config).unwrap();
    }

    #[test]
    fn validates_python_attach_config_without_program() {
        let root = Path::new("/repo");
        let config = DebugConfig {
            id: "attach".to_string(),
            name: "Attach".to_string(),
            config_type: DebugConfigType::Python,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(5678),
            args: vec![],
            env: Default::default(),
            breakpoints: vec![],
        };

        validate_debug_config(root, &config).unwrap();
    }

    #[test]
    fn validates_remote_python_attach_config_paths() {
        let config = DebugConfig {
            id: "remote-py".to_string(),
            name: "Remote Python".to_string(),
            config_type: DebugConfigType::Python,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: "app".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(5678),
            args: Vec::new(),
            env: BTreeMap::new(),
            breakpoints: vec![DebugBreakpoint {
                file: "app/main.py".to_string(),
                line: 12,
                column: 1,
                condition: None,
                log_message: None,
            }],
        };

        validate_remote_debug_config("/srv/project", &config).unwrap();
        assert_eq!(
            resolve_remote_python_breakpoint_targets("/srv/project", &config.breakpoints).unwrap()
                [0]
            .file,
            PathBuf::from("/srv/project/app/main.py")
        );
    }

    #[test]
    fn validates_remote_node_attach_config_paths() {
        let config = DebugConfig {
            id: "remote-node".to_string(),
            name: "Remote Node".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: "app".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(9229),
            args: Vec::new(),
            env: BTreeMap::new(),
            breakpoints: vec![DebugBreakpoint {
                file: "app/main.ts".to_string(),
                line: 12,
                column: 1,
                condition: Some("count > 0".to_string()),
                log_message: None,
            }],
        };

        validate_remote_debug_config("/srv/project space", &config).unwrap();
        let targets =
            resolve_remote_node_breakpoint_targets("/srv/project space", &config.breakpoints)
                .unwrap();
        assert_eq!(
            targets[0].file_url,
            "file:///srv/project%20space/app/main.ts"
        );
        assert_eq!(targets[0].condition.as_deref(), Some("count > 0"));
    }

    #[test]
    fn rejects_remote_debug_paths_outside_root() {
        let mut config = DebugConfig {
            id: "remote-py".to_string(),
            name: "Remote Python".to_string(),
            config_type: DebugConfigType::Python,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: "/srv/project".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: Some(5678),
            args: Vec::new(),
            env: BTreeMap::new(),
            breakpoints: Vec::new(),
        };

        config.cwd = "/srv/project2".to_string();
        assert!(validate_remote_debug_config("/srv/project", &config).is_err());
        config.cwd = "../project".to_string();
        assert!(validate_remote_debug_config("/srv/project", &config).is_err());
    }

    #[test]
    fn rejects_attach_config_with_missing_port_or_bad_host() {
        let root = Path::new("/repo");
        let mut config = DebugConfig {
            id: "attach".to_string(),
            name: "Attach".to_string(),
            config_type: DebugConfigType::Node,
            request: DebugRequestType::Attach,
            program: "".to_string(),
            cwd: ".".to_string(),
            attach_host: "127.0.0.1".to_string(),
            attach_port: None,
            args: vec![],
            env: Default::default(),
            breakpoints: vec![],
        };

        let missing_port = validate_debug_config(root, &config).unwrap_err();
        assert!(missing_port.contains("Attach port"));

        config.attach_port = Some(9229);
        config.attach_host = "http://127.0.0.1".to_string();
        let bad_host = validate_debug_config(root, &config).unwrap_err();
        assert!(bad_host.contains("Attach host"));
    }

    #[test]
    fn resolves_debug_cwd_inside_project_root() {
        let root = unique_test_dir("cwd");
        fs::create_dir_all(root.join("app")).unwrap();

        let cwd = resolve_debug_cwd(&root, "app").unwrap();

        assert_eq!(cwd, root.join("app").canonicalize().unwrap());

        fs::remove_dir_all(root).unwrap();
    }
}
