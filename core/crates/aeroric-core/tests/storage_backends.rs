//! P7-06 验收：存储后端契约（`tests/storage_backends.rs`）。
//!
//! fake backend 覆盖断线 / 冲突 / quota / 取消（fail_with 注入的远端已在
//! `notebook_sync.rs` 由引擎侧消费；本文件盯 **conn 仓库**与 **路径守卫**：
//! 公开文件永不含凭据值、0600 权限、根目录拒绝改写、mount 挂载点不出
//! mounts 目录、协议元信息完整性。真实 provider 的 HTTP 语义由 opendal/box/
//! baidu 契约测试（同 crate 单测）覆盖——本套件不接真实云。

use std::collections::BTreeMap;
use std::path::Path;

use aeroric_core::storage_backends::{
    capability_for,
    conn::{
        required_config_keys, validate_connection, StorageConnRepo, StorageConnection,
        StorageProtocol,
    },
    join_storage_path, normalize_storage_path, path_basename, path_parent, protocol_descriptors,
    validate_storage_mutation_path,
};

fn connection(protocol: StorageProtocol) -> StorageConnection {
    StorageConnection {
        id: "conn-1".to_string(),
        name: "Bucket".to_string(),
        group: None,
        protocol,
        config: BTreeMap::from([
            ("bucket".to_string(), "media".to_string()),
            ("region".to_string(), "us-east-1".to_string()),
        ]),
        secrets: BTreeMap::from([
            ("accessKeyId".to_string(), "AKIAEXAMPLE".to_string()),
            ("secretAccessKey".to_string(), "s3cr3t".to_string()),
        ]),
        created_at: 1,
        last_connected_at: None,
    }
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "aeroric-storage-contract-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create dir");
    std::fs::canonicalize(&dir).expect("canonicalize")
}

// ── 凭据边界：公开文件永不含凭据值；secrets 文件 0600 ─────────────────────

#[test]
fn the_public_file_never_contains_secret_values_and_stays_private() {
    let dir = temp_dir("secrets");
    let repo = StorageConnRepo::new(&dir);
    repo.save_connection(connection(StorageProtocol::S3))
        .expect("save");

    let public = std::fs::read_to_string(dir.join("storage-connections.json")).expect("public");
    assert!(!public.contains("AKIAEXAMPLE"), "公开文件不含 accessKey");
    assert!(!public.contains("s3cr3t"), "公开文件不含 secret");
    assert!(!public.contains("secrets"), "公开文件不含 secrets 键");

    let secrets = std::fs::read_to_string(dir.join("storage-secrets.json")).expect("secrets");
    assert!(secrets.contains("AKIAEXAMPLE"), "凭据单独落盘");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for file in ["storage-connections.json", "storage-secrets.json"] {
            let mode = std::fs::metadata(dir.join(file))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "{file} 必须是 0600（group/other 无权）");
        }
    }

    // 读回合并：凭据回到连接里。
    let found = aeroric_core::storage_backends::conn::find_connection(&dir, "conn-1")
        .expect("find with secrets");
    assert_eq!(
        found.secrets.get("accessKeyId").map(String::as_str),
        Some("AKIAEXAMPLE")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_merge_and_delete_round_trip_through_the_repo() {
    let dir = temp_dir("roundtrip");
    let repo = StorageConnRepo::new(&dir);

    // 先存完整凭据；再次保存时空值键表示「保留原值」。
    repo.save_connection(connection(StorageProtocol::S3))
        .expect("initial save");
    let mut next = connection(StorageProtocol::S3);
    next.secrets
        .insert("accessKeyId".to_string(), String::new());
    let listed = repo.save_connection(next).expect("save");
    assert!(listed[0].secrets.is_empty(), "返回给前端的一侧永远无凭据");
    let merged =
        aeroric_core::storage_backends::conn::find_connection(&dir, "conn-1").expect("find");
    assert_eq!(
        merged.secrets.get("accessKeyId").map(String::as_str),
        Some("AKIAEXAMPLE"),
        "空值键从既有凭据补回"
    );

    // 删除。
    repo.delete_connection("conn-1").expect("delete");
    assert!(repo.list_public().expect("list").is_empty());
    assert!(
        repo.touch_connection("conn-1", 42).is_ok(),
        "touch 不存在的连接不是错误"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn validation_requires_config_and_rejects_blank_identity() {
    let mut input = connection(StorageProtocol::S3);
    input.config.remove("region");
    assert!(validate_connection(&input).is_err(), "缺 region 拒绝");
    assert!(required_config_keys(StorageProtocol::WebdavHttps).contains(&"endpoint"));

    let mut blank = connection(StorageProtocol::S3);
    blank.id = String::new();
    assert!(validate_connection(&blank).is_err());
}

// ── 路径守卫：根目录永不作为改写目标 ────────────────────────────────────────

#[test]
fn mutation_paths_reject_the_root_and_normalize_the_rest() {
    for path in ["/", "/.", "/a/..", "../.."] {
        assert!(validate_storage_mutation_path(path).is_err(), "{path}");
    }
    assert_eq!(validate_storage_mutation_path("/a/../b").unwrap(), "/b");
    assert_eq!(normalize_storage_path("/../../etc/passwd"), "/etc/passwd");
    assert_eq!(path_basename("/a/b.txt"), "b.txt");
    assert_eq!(path_parent("/a/b.txt"), "/a");
    assert_eq!(join_storage_path("/a/", "b"), "/a/b");
}

#[test]
fn every_protocol_has_capabilities_and_a_unique_display_name() {
    for protocol in StorageProtocol::ALL {
        let capability = capability_for(protocol);
        assert!(capability.read_dir, "{} cannot read_dir", protocol.as_str());
    }
    let descriptors = protocol_descriptors();
    assert_eq!(descriptors.len(), StorageProtocol::ALL.len());
    let mut names: Vec<&str> = descriptors
        .iter()
        .map(|d| d.display_name.as_str())
        .collect();
    names.sort();
    let before = names.len();
    names.dedup();
    assert_eq!(names.len(), before, "展示名两两不同");
}

// ── mount 挂载点白名单（id 校验防 `../` 逃逸）───────────────────────────────

#[test]
fn mount_connection_ids_cannot_escape_the_mounts_directory() {
    use aeroric_core::storage_backends::mount;
    assert!(mount::validate_connection_id("conn-1").is_ok());
    assert!(mount::validate_connection_id("../escape").is_err());
    assert!(mount::validate_connection_id("").is_err());
    let point = mount::mount_point_for(Path::new("/data"), "conn-1").expect("point");
    assert!(
        point.starts_with("/data/mounts/"),
        "挂载点固定在 mounts 目录内"
    );
}
