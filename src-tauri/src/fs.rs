use crate::path_guard::validate_project_root;
#[cfg(all(test, unix))]
use aeroric_core::files::legacy_copy::copy_path_recursive;
#[cfg(test)]
use aeroric_core::files::legacy_copy::ensure_copy_target_not_inside_source;
#[cfg(all(test, unix))]
use aeroric_core::files::legacy_entry::validate_entry_name;
use aeroric_core::files::legacy_entry::validate_new_path_within;
use aeroric_core::files::legacy_modify::validate_existing_path_for_delete;
#[cfg(all(test, unix))]
use aeroric_core::files::legacy_modify::validate_existing_path_for_modify;
use aeroric_core::files::legacy_path::validate_path_within;
#[cfg(test)]
use std::path::PathBuf;
use tauri::Manager;

#[cfg(test)]
#[cfg(unix)]
mod legacy_copy_tests;

#[cfg(test)]
#[cfg(unix)]
mod legacy_directory_tests;

#[cfg(test)]
#[cfg(unix)]
mod legacy_entry_tests;

#[cfg(test)]
#[cfg(unix)]
mod legacy_modify_tests;

#[cfg(test)]
#[cfg(unix)]
mod legacy_path_tests;

#[cfg(test)]
#[cfg(unix)]
mod legacy_read_tests;

#[cfg(test)]
#[cfg(unix)]
mod legacy_write_tests;

pub(crate) use aeroric_core::files::legacy_directory::FsEntry;

pub(crate) use aeroric_core::files::project_files::ProjectFileSearchResult;

pub(crate) use aeroric_core::files::legacy_read::ImagePreviewData;

#[tauri::command]
pub async fn open_in_system_file_manager(
    app: tauri::AppHandle,
    path: String,
    project_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 路径校验是 webview 请求的安全边界,留在命令体;OS 调用走平台服务端口。
        let target = validate_path_within(&path, &project_path, true)?;
        let services = app.state::<std::sync::Arc<dyn aeroric_core::runtime::PlatformServices>>();
        services
            .reveal_in_file_manager(&target)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_dir_entries(path: String, project_path: String) -> Result<Vec<FsEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = validate_path_within(&path, &project_path, false)?;
        aeroric_core::files::legacy_directory::read_dir_entries(&directory, &project_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_file_content(path: String, project_path: String) -> Result<String, String> {
    read_file_content_with_policy(path, project_path, true).await
}

/// Read a project file without following a final symlink outside the project.
/// Remote callers use this stricter policy; local settings retain the legacy
/// allowance for symlinked instruction files.
pub async fn read_project_file_content(
    path: String,
    project_path: String,
) -> Result<String, String> {
    read_file_content_with_policy(path, project_path, false).await
}

async fn read_file_content_with_policy(
    path: String,
    project_path: String,
    allow_symlink_escape: bool,
) -> Result<String, String> {
    let validated_path = validate_path_within(&path, &project_path, allow_symlink_escape)?;

    tauri::async_runtime::spawn_blocking(move || {
        aeroric_core::files::legacy_read::read_file_content(&validated_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_image_preview(
    path: String,
    project_path: String,
) -> Result<ImagePreviewData, String> {
    let validated_path = validate_path_within(&path, &project_path, true)?;

    tauri::async_runtime::spawn_blocking(move || {
        aeroric_core::files::legacy_read::read_image_preview(&validated_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn write_file_content(
    path: String,
    content: String,
    project_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        aeroric_core::files::legacy_write::write_file_content(&project_path, &path, &content)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_file(path: String, project_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (parent, file_name) = validate_new_path_within(&path, &project_path)?;
        let target = parent.join(&file_name);
        aeroric_core::files::legacy_entry::create_file(&target)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn create_directory(path: String, project_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (parent, file_name) = validate_new_path_within(&path, &project_path)?;
        let target = parent.join(&file_name);
        aeroric_core::files::legacy_entry::create_directory(&target)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_path(path: String, project_path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = validate_existing_path_for_delete(&path, &project_path)?;
        trash::delete(&resolved).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn rename_path(
    path: String,
    new_name: String,
    project_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        aeroric_core::files::legacy_modify::rename_path(path, new_name, project_path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn copy_paths_to_directory(
    source_paths: Vec<String>,
    target_directory: String,
    project_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if source_paths.is_empty() {
            return Ok(());
        }
        let target = validate_path_within(&target_directory, &project_path, false)?;
        aeroric_core::files::legacy_copy::copy_paths_to_directory(source_paths, target)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn read_clipboard_file_paths(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let services = app.state::<std::sync::Arc<dyn aeroric_core::runtime::PlatformServices>>();
        services
            .read_clipboard_file_paths()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("aeroric-fs-test-{}-{}", name, suffix))
    }

    #[test]
    fn rejects_copying_directory_into_its_own_descendant() {
        let root = unique_test_dir("copy-descendant");
        let source = root.join("source");
        let target = source.join("child");
        std::fs::create_dir_all(&target).expect("create test directories");

        let result = ensure_copy_target_not_inside_source(&source, &target);

        let _ = std::fs::remove_dir_all(&root);
        assert!(result.is_err());
    }

    #[test]
    fn allows_copying_file_into_child_directory() {
        let root = unique_test_dir("copy-file");
        let source = root.join("source.txt");
        let target = root.join("target");
        std::fs::create_dir_all(&target).expect("create target directory");
        std::fs::write(&source, "data").expect("create source file");

        let result = ensure_copy_target_not_inside_source(&source, &target);

        let _ = std::fs::remove_dir_all(&root);
        assert!(result.is_ok());
    }

    #[test]
    fn fallback_project_file_list_skips_common_dependency_directories() {
        let root = unique_test_dir("project-files-fallback");
        std::fs::create_dir_all(root.join("src")).expect("create source directory");
        std::fs::create_dir_all(root.join("node_modules")).expect("create dependency directory");
        std::fs::create_dir_all(root.join(".git")).expect("create git directory");
        std::fs::write(root.join("README.md"), "readme").expect("create readme");
        std::fs::write(root.join("src").join("main.ts"), "main").expect("create source");
        std::fs::write(root.join("node_modules").join("package.js"), "dependency")
            .expect("create dependency file");
        std::fs::write(root.join(".git").join("config"), "git").expect("create git metadata");

        let files = aeroric_core::files::project_files::list_project_files(&root, None, &[])
            .expect("list project files");

        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            files,
            vec!["README.md".to_string(), "src/main.ts".to_string()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn strict_project_reads_reject_symlink_escape() {
        let root = unique_test_dir("symlink-policy");
        let outside = unique_test_dir("symlink-outside");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside");
        let outside_file = outside.join("secret.txt");
        let link = root.join("linked.txt");
        std::fs::write(&outside_file, "secret").expect("create outside file");
        std::os::unix::fs::symlink(&outside_file, &link).expect("create symlink");

        assert!(
            validate_path_within(&link.to_string_lossy(), &root.to_string_lossy(), true).is_ok()
        );
        assert!(
            validate_path_within(&link.to_string_lossy(), &root.to_string_lossy(), false).is_err()
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn directory_listing_rejects_symlink_escape() {
        let root = unique_test_dir("directory-symlink-policy");
        let outside = unique_test_dir("directory-symlink-outside");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::create_dir_all(&outside).expect("create outside directory");
        std::fs::write(outside.join("secret.txt"), "secret").expect("create outside file");
        let link = root.join("linked-directory");
        std::os::unix::fs::symlink(&outside, &link).expect("create directory symlink");

        let result = read_dir_entries(
            link.to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
        )
        .await;

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        assert!(matches!(
            result,
            Err(error) if error == "Path is outside the allowed directory"
        ));
    }

    /// `is_dir` 跟随链接,`is_symlink` 不跟随 —— 指向目录的链接必须两个都为真,
    /// 否则前端无法把「链接到目录」和「真目录」画成不同的图标。
    #[cfg(unix)]
    #[tokio::test]
    async fn directory_listing_flags_symlinks_without_losing_the_followed_kind() {
        let root = unique_test_dir("directory-symlink-flags");
        std::fs::create_dir_all(root.join("real-dir")).expect("create real directory");
        std::fs::write(root.join("real.txt"), "data").expect("create real file");
        std::os::unix::fs::symlink(root.join("real-dir"), root.join("link-dir"))
            .expect("create directory symlink");
        std::os::unix::fs::symlink(root.join("real.txt"), root.join("link.txt"))
            .expect("create file symlink");

        let entries = read_dir_entries(
            root.to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
        )
        .await
        .expect("list directory");

        let by_name = |name: &str| {
            entries
                .iter()
                .find(|entry| entry.name == name)
                .unwrap_or_else(|| panic!("missing entry {name}"))
        };
        let real_dir = (by_name("real-dir").is_dir, by_name("real-dir").is_symlink);
        let link_dir = (by_name("link-dir").is_dir, by_name("link-dir").is_symlink);
        let real_file = (by_name("real.txt").is_dir, by_name("real.txt").is_symlink);
        let link_file = (by_name("link.txt").is_dir, by_name("link.txt").is_symlink);

        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(real_dir, (true, false));
        assert_eq!(link_dir, (true, true));
        assert_eq!(real_file, (false, false));
        assert_eq!(link_file, (false, true));
    }

    /// 前端 `src/components/file-explorer/types.ts` 的 `FsEntry` 逐字段消费这份 JSON。
    /// 这里锁的是**线上键名**:结构体没有 `rename_all`,改名/加字段而不同步 TS 侧,
    /// 文件树会静默读到 `undefined`(`modified_at_ms` / `modifiedAtMs` 就这么错过一次,
    /// 按修改时间排序恒等于按名字排序)。
    #[tokio::test]
    async fn fs_entry_wire_field_names_match_the_frontend_type() {
        let root = unique_test_dir("fs-entry-wire-names");
        std::fs::create_dir_all(&root).expect("create root");
        std::fs::write(root.join("main.rs"), "fn main() {}").expect("create file");

        let entries = read_dir_entries(
            root.to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
        )
        .await
        .expect("list directory");
        let json = serde_json::to_value(&entries[0]).expect("serialize entry");
        let mut keys: Vec<&str> = json
            .as_object()
            .expect("entry serializes to an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();

        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            keys,
            [
                "extension",
                "is_dir",
                "is_gitignored",
                "is_symlink",
                "modified_at_ms",
                "name",
                "path",
            ]
        );
    }
}

#[tauri::command]
pub async fn list_project_files(
    app: tauri::AppHandle,
    project_path: String,
) -> Result<Vec<String>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "files.list_project_files",
        serde_json::json!({"projectPath": project_path}),
        async move {
            let result = tauri::async_runtime::spawn_blocking(move || {
                let root = validate_project_root(&project_path)?;
                let git = crate::platform::detect_path("git");
                aeroric_core::files::project_files::list_project_files(
                    &root,
                    Some(&git),
                    crate::app_settings::get_login_shell_env(),
                )
            })
            .await
            .map_err(|e| e.to_string())??;
            serde_json::to_value(result).map_err(|e| e.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_project_files(
    app: tauri::AppHandle,
    project_path: String,
    query: String,
    extensions: Vec<String>,
    limit: Option<usize>,
) -> Result<Vec<ProjectFileSearchResult>, String> {
    let value = crate::host_dispatch::dispatch(
        &app, "files.search_project_files",
        serde_json::json!({"projectPath": project_path, "query": query, "extensions": extensions, "limit": limit}),
        async move {
            let result = tauri::async_runtime::spawn_blocking(move || {
                let root = validate_project_root(&project_path)?;
                aeroric_core::files::project_files::search_project_files(
                    &root, &query, &extensions, limit, "git", crate::app_settings::get_login_shell_env(),
                )
            }).await.map_err(|e| e.to_string())??;
            serde_json::to_value(result).map_err(|e| e.to_string())
        },
    ).await?;
    serde_json::from_value(value).map_err(|e| e.to_string())
}
