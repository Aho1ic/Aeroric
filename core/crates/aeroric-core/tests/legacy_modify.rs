//! Existing-entry policy contrasts, without invoking any rename/Trash boundary.

use aeroric_core::files::{legacy_entry, legacy_modify, legacy_path, path_policy};
use std::fs;
use std::path::Path;

#[path = "common/scratch.rs"]
mod scratch;

fn error(target: &Path, project: &Path, expected: &str) {
    assert_eq!(
        legacy_modify::validate_existing_path_for_delete(
            target.to_str().unwrap(),
            project.to_str().unwrap()
        ),
        Err(expected.to_string())
    );
    assert_eq!(
        legacy_modify::validate_existing_path_for_modify(
            target.to_str().unwrap(),
            project.to_str().unwrap(),
            "move"
        ),
        Err(expected.to_string())
    );
}

fn allowed(target: &Path, project: &Path, expected: &Path) {
    assert_eq!(
        legacy_modify::validate_existing_path_for_delete(
            target.to_str().unwrap(),
            project.to_str().unwrap()
        ),
        Ok(expected.to_path_buf())
    );
    assert_eq!(
        legacy_modify::validate_existing_path_for_modify(
            target.to_str().unwrap(),
            project.to_str().unwrap(),
            "move"
        ),
        Ok((expected.to_path_buf(), project.canonicalize().unwrap()))
    );
}

#[test]
fn error_order_and_root_target_are_not_new_entry_or_native_policy() {
    let scratch = scratch::Scratch::new("legacy-modify-order");
    let project = scratch.path().join("project");
    fs::create_dir(&project).unwrap();
    let missing = scratch.path().join("missing");
    let file_root = scratch.path().join("root-file");
    fs::write(&file_root, b"unchanged").unwrap();
    let project_time = fs::metadata(&project).unwrap().modified().unwrap();
    error(Path::new("relative"), &missing, "Path must be absolute");
    error(
        &missing.join("leaf"),
        &missing,
        &format!(
            "Cannot resolve parent directory: {}",
            missing.canonicalize().unwrap_err()
        ),
    );
    error(
        &project.join("leaf"),
        &missing,
        &format!(
            "Cannot resolve root directory: {}",
            missing.canonicalize().unwrap_err()
        ),
    );
    error(&project, &project, "Path is outside the allowed directory");
    for name in ["absent", "nul\0leaf"] {
        error(&project.join(name), &project, "Path does not exist");
    }
    error(&file_root.join("leaf"), &file_root, "Path does not exist");
    assert_eq!(
        path_policy::validate_project_root(file_root.to_str().unwrap()),
        Err("Project path is not a directory".into())
    );
    assert_eq!(
        legacy_entry::validate_new_path_within(
            project.join("absent").to_str().unwrap(),
            project.to_str().unwrap()
        ),
        Ok((project.canonicalize().unwrap(), "absent".into()))
    );
    assert_eq!(fs::read(&file_root).unwrap(), b"unchanged");
    assert_eq!(
        fs::metadata(&project).unwrap().modified().unwrap(),
        project_time
    );
    assert_eq!(fs::read_dir(project).unwrap().count(), 0);
}

#[test]
fn protection_preserves_first_segment_action_and_metadata_precedence() {
    let scratch = scratch::Scratch::new("legacy-modify-protection");
    for (index, protected) in [".git", ".GiT", ".aeroric", ".AeRoRiC"]
        .into_iter()
        .enumerate()
    {
        let project = scratch.path().join(format!("project-{index}"));
        fs::create_dir(&project).unwrap();
        let target = project.join(protected);
        error(&target, &project, "Path does not exist");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("child"), b"not modified").unwrap();
        let nested = project.join("sub").join(protected);
        fs::create_dir_all(&nested).unwrap();
        let before = fs::metadata(&project).unwrap().modified().unwrap();
        for target in [&target, &target.join("child")] {
            assert_eq!(
                legacy_modify::validate_existing_path_for_delete(
                    target.to_str().unwrap(),
                    project.to_str().unwrap()
                ),
                Err(format!("Cannot delete protected directory: {protected}"))
            );
            assert_eq!(
                legacy_modify::validate_existing_path_for_modify(
                    target.to_str().unwrap(),
                    project.to_str().unwrap(),
                    " 原样 move "
                ),
                Err(format!(
                    "Cannot  原样 move  protected directory: {protected}"
                ))
            );
        }
        error(&target.join("absent"), &project, "Path does not exist");
        allowed(&nested, &project, &nested.canonicalize().unwrap());
        assert_eq!(fs::read(target.join("child")).unwrap(), b"not modified");
        assert_eq!(fs::metadata(&project).unwrap().modified().unwrap(), before);
        assert_eq!(fs::read_dir(&project).unwrap().count(), 2);
    }
}

#[test]
fn raw_existing_names_are_not_revalidated_or_trimmed() {
    let scratch = scratch::Scratch::new("legacy-modify-names");
    let project = scratch.path();
    let plain = project.join("ordinary");
    fs::write(&plain, b"plain").unwrap();
    let before = fs::metadata(&plain).unwrap().modified().unwrap();
    allowed(&plain, project, &plain.canonicalize().unwrap());
    #[cfg(unix)]
    {
        for name in [" 原名 ", "existing\\name"] {
            let target = project.join(name);
            fs::write(&target, name.as_bytes()).unwrap();
            let time = fs::metadata(&target).unwrap().modified().unwrap();
            allowed(
                &target,
                project,
                &project.canonicalize().unwrap().join(name),
            );
            assert_eq!(fs::read(&target).unwrap(), name.as_bytes());
            assert_eq!(fs::metadata(&target).unwrap().modified().unwrap(), time);
        }
        assert_eq!(
            legacy_entry::validate_entry_name("existing\\name"),
            Err("File name contains forbidden characters".into())
        );
    }
    assert_eq!(fs::read(&plain).unwrap(), b"plain");
    assert_eq!(fs::metadata(&plain).unwrap().modified().unwrap(), before);
}

#[test]
fn final_leaf_identity_is_distinct_from_strict_reads_and_parent_resolution() {
    let scratch = scratch::Scratch::new("legacy-modify-leaf");
    let project = scratch.path().join("project");
    fs::create_dir(&project).unwrap();
    let file = project.join("file");
    fs::write(&file, b"inside").unwrap();
    allowed(&file, &project, &file.canonicalize().unwrap());
    // This contrast is portable; symlink branches below run only on Unix.
    assert!(legacy_path::validate_path_within(
        project.join("absent").to_str().unwrap(),
        project.to_str().unwrap(),
        false
    )
    .is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let outside = scratch.path().join("outside");
        fs::write(&outside, b"outside").unwrap();
        symlink("file", project.join("inside-link")).unwrap();
        symlink(&outside, project.join("outside-link")).unwrap();
        symlink(scratch.path().join("missing"), project.join("dangling")).unwrap();
        symlink("loop", project.join("loop")).unwrap();
        symlink(&project, scratch.path().join("parent-alias")).unwrap();
        let before = fs::metadata(&project).unwrap().modified().unwrap();
        for name in ["inside-link", "outside-link", "dangling", "loop"] {
            let target = project.join(name);
            let link = fs::read_link(&target).unwrap();
            allowed(
                &target,
                &project,
                &project.canonicalize().unwrap().join(name),
            );
            assert_eq!(fs::read_link(&target).unwrap(), link);
        }
        allowed(
            &scratch.path().join("parent-alias/file"),
            &project,
            &file.canonicalize().unwrap(),
        );
        assert_eq!(
            legacy_path::validate_path_within(
                project.join("outside-link").to_str().unwrap(),
                project.to_str().unwrap(),
                false
            ),
            Err("Path is outside the allowed directory".into())
        );
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
        assert_eq!(fs::metadata(&project).unwrap().modified().unwrap(), before);
    }
    assert_eq!(fs::read(&file).unwrap(), b"inside");
}
