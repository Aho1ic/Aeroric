//! 基于 Apache OpenDAL 的后端:对象存储、WebDAV 与四种 OAuth 网盘。
//!
//! OpenDAL 的 `Operator` 是异步 API,而 [`StorageBackend`] 是同步签名(调用方
//! 已在 `spawn_blocking` 里)。这里用一个专用的当前线程 runtime 桥接,避免
//! 在 blocking 线程上调用 `Handle::block_on`(会 panic)。

use opendal::{ErrorKind, Operator};

use crate::storage_backend::{
    join_storage_path, normalize_storage_path, validate_storage_mutation_path, Capability,
    StorageBackend, StorageEntry, StorageStat,
};
use crate::storage_conn::{StorageConnection, StorageProtocol};

/// 各云厂商 S3 兼容端点。用户不填 endpoint 时作为默认值。
const QINIU_DEFAULT_ENDPOINT: &str = "https://s3.cn-east-1.qiniucs.com";
const JD_CLOUD_DEFAULT_ENDPOINT: &str = "https://s3.cn-north-1.jdcloud-oss.com";

pub struct OpendalBackend {
    operator: Operator,
    runtime: tokio::runtime::Runtime,
    capability: Capability,
}

impl OpendalBackend {
    fn block<F, T>(&self, future: F) -> Result<T, String>
    where
        F: std::future::Future<Output = opendal::Result<T>>,
    {
        self.runtime.block_on(future).map_err(format_opendal_error)
    }
}

/// 把 OpenDAL 错误转成面向用户的信息,保留 kind 便于识别 404。
fn format_opendal_error(error: opendal::Error) -> String {
    match error.kind() {
        ErrorKind::NotFound => "No such file or directory".to_string(),
        ErrorKind::PermissionDenied => "Permission denied".to_string(),
        ErrorKind::AlreadyExists => "A file or folder with that name already exists".to_string(),
        ErrorKind::Unsupported => "This operation is not supported by the service".to_string(),
        _ => error.to_string(),
    }
}

/// OpenDAL 用不带前导 `/` 的相对路径;目录必须以 `/` 结尾。
fn to_opendal_path(path: &str, is_dir: bool) -> String {
    let normalized = normalize_storage_path(path);
    let relative = normalized.trim_start_matches('/');
    if relative.is_empty() {
        return "/".to_string();
    }
    if is_dir {
        format!("{relative}/")
    } else {
        relative.to_string()
    }
}

fn metadata_to_stat(metadata: &opendal::Metadata) -> StorageStat {
    StorageStat {
        is_dir: metadata.is_dir(),
        size: if metadata.is_dir() {
            None
        } else {
            Some(metadata.content_length())
        },
        modified_at_ms: metadata
            .last_modified()
            .map(|value| value.into_inner().as_millisecond())
            .and_then(|millis| u64::try_from(millis).ok()),
    }
}

impl StorageBackend for OpendalBackend {
    fn capability(&self) -> Capability {
        self.capability
    }

    fn read_dir(&self, path: &str) -> Result<Vec<StorageEntry>, String> {
        let parent = normalize_storage_path(path);
        let listed = self.block(self.operator.list(&to_opendal_path(&parent, true)))?;
        let mut entries = Vec::with_capacity(listed.len());
        for item in listed {
            let name = item.name().trim_end_matches('/').to_string();
            // OpenDAL 的 list 会把被列目录自身作为第一条返回,跳过。
            if name.is_empty() {
                continue;
            }
            let stat = metadata_to_stat(item.metadata());
            entries.push(StorageEntry {
                path: join_storage_path(&parent, &name),
                name,
                is_dir: stat.is_dir,
                size: stat.size,
                modified_at_ms: stat.modified_at_ms,
            });
        }
        Ok(entries)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let buffer = self.block(self.operator.read(&to_opendal_path(path, false)))?;
        Ok(buffer.to_vec())
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        self.block(
            self.operator
                .write(&to_opendal_path(path, false), bytes.to_vec()),
        )
        .map(|_| ())
    }

    fn create_dir(&self, path: &str) -> Result<(), String> {
        self.block(self.operator.create_dir(&to_opendal_path(path, true)))
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        let path = validate_storage_mutation_path(path)?;
        // 目录需要递归删除;先 stat 判断类型,stat 失败则按文件删。
        match self.stat(&path) {
            Ok(stat) if stat.is_dir => self.block(self.operator.delete_options(
                &to_opendal_path(&path, true),
                opendal::options::DeleteOptions {
                    recursive: true,
                    ..Default::default()
                },
            )),
            _ => self.block(self.operator.delete(&to_opendal_path(&path, false))),
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let from = validate_storage_mutation_path(from)?;
        let stat = self.stat(&from)?;
        if stat.is_dir {
            // 对象存储没有目录 rename;逐条 copy 后删除源目录。
            self.copy_dir_recursive(&from, to)?;
            return self.delete(&from);
        }
        let from_path = to_opendal_path(&from, false);
        let to_path = to_opendal_path(to, false);
        match self.block(self.operator.rename(&from_path, &to_path)) {
            Ok(()) => Ok(()),
            // 不支持原生 rename 的服务退回 copy + delete。
            Err(_) => {
                self.block(self.operator.copy(&from_path, &to_path))?;
                self.block(self.operator.delete(&from_path))
            }
        }
    }

    fn copy(&self, from: &str, to: &str) -> Result<(), String> {
        let stat = self.stat(from)?;
        if stat.is_dir {
            return self.copy_dir_recursive(from, to);
        }
        self.block(
            self.operator
                .copy(&to_opendal_path(from, false), &to_opendal_path(to, false)),
        )
        .map(|_| ())
    }

    fn stat(&self, path: &str) -> Result<StorageStat, String> {
        let normalized = normalize_storage_path(path);
        if normalized == "/" {
            return Ok(StorageStat {
                is_dir: true,
                size: None,
                modified_at_ms: None,
            });
        }
        // 先按文件 stat;404 时再按目录 stat(对象存储两者是不同的 key)。
        match self
            .runtime
            .block_on(self.operator.stat(&to_opendal_path(&normalized, false)))
        {
            Ok(metadata) => Ok(metadata_to_stat(&metadata)),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let metadata =
                    self.block(self.operator.stat(&to_opendal_path(&normalized, true)))?;
                Ok(metadata_to_stat(&metadata))
            }
            Err(error) => Err(format_opendal_error(error)),
        }
    }
}

impl OpendalBackend {
    /// 递归复制目录。对象存储没有服务端目录 copy,只能逐个对象搬。
    fn copy_dir_recursive(&self, from: &str, to: &str) -> Result<(), String> {
        self.create_dir(to)?;
        for entry in self.read_dir(from)? {
            let target = join_storage_path(to, &entry.name);
            if entry.is_dir {
                self.copy_dir_recursive(&entry.path, &target)?;
            } else {
                self.block(self.operator.copy(
                    &to_opendal_path(&entry.path, false),
                    &to_opendal_path(&target, false),
                ))?;
            }
        }
        Ok(())
    }
}

/// 取公开配置,缺失或空白返回 `None`。
fn config_value(connection: &StorageConnection, key: &str) -> Option<String> {
    connection
        .config
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 取凭据,缺失或空白返回 `None`。
fn secret_value(connection: &StorageConnection, key: &str) -> Option<String> {
    connection
        .secrets
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn require_config(connection: &StorageConnection, key: &str) -> Result<String, String> {
    config_value(connection, key).ok_or_else(|| format!("\"{key}\" is required"))
}

fn require_secret(connection: &StorageConnection, key: &str) -> Result<String, String> {
    secret_value(connection, key).ok_or_else(|| format!("\"{key}\" is required"))
}

/// `root` 是连接层面的路径前缀,所有操作都相对它解析。
fn connection_root(connection: &StorageConnection) -> String {
    config_value(connection, "root")
        .map(|value| normalize_storage_path(&value))
        .unwrap_or_else(|| "/".to_string())
}

/// 构造 S3 系(含各家兼容端点)的 Operator。
fn build_s3(
    connection: &StorageConnection,
    default_endpoint: Option<&str>,
) -> Result<Operator, String> {
    let mut builder = opendal::services::S3::default()
        .bucket(&require_config(connection, "bucket")?)
        .root(&connection_root(connection));

    let endpoint =
        config_value(connection, "endpoint").or_else(|| default_endpoint.map(str::to_string));
    if let Some(endpoint) = endpoint {
        builder = builder.endpoint(&endpoint);
    }
    // S3 兼容服务多数忽略 region,但签名需要一个非空值。
    builder = builder
        .region(&config_value(connection, "region").unwrap_or_else(|| "us-east-1".to_string()));
    if let Some(access_key) = secret_value(connection, "accessKeyId") {
        builder = builder.access_key_id(&access_key);
    }
    if let Some(secret_key) = secret_value(connection, "secretAccessKey") {
        builder = builder.secret_access_key(&secret_key);
    }
    if let Some(session_token) = secret_value(connection, "sessionToken") {
        builder = builder.session_token(&session_token);
    }
    Operator::new(builder).map_err(format_opendal_error)
}

fn build_oss(connection: &StorageConnection) -> Result<Operator, String> {
    let mut builder = opendal::services::Oss::default()
        .bucket(&require_config(connection, "bucket")?)
        .endpoint(&require_config(connection, "endpoint")?)
        .root(&connection_root(connection));
    if let Some(access_key) = secret_value(connection, "accessKeyId") {
        builder = builder.access_key_id(&access_key);
    }
    if let Some(secret_key) = secret_value(connection, "secretAccessKey") {
        builder = builder.access_key_secret(&secret_key);
    }
    Operator::new(builder).map_err(format_opendal_error)
}

fn build_cos(connection: &StorageConnection) -> Result<Operator, String> {
    let mut builder = opendal::services::Cos::default()
        .bucket(&require_config(connection, "bucket")?)
        .endpoint(&require_config(connection, "endpoint")?)
        .root(&connection_root(connection));
    if let Some(secret_id) = secret_value(connection, "accessKeyId") {
        builder = builder.secret_id(&secret_id);
    }
    if let Some(secret_key) = secret_value(connection, "secretAccessKey") {
        builder = builder.secret_key(&secret_key);
    }
    Operator::new(builder).map_err(format_opendal_error)
}

fn build_upyun(connection: &StorageConnection) -> Result<Operator, String> {
    let builder = opendal::services::Upyun::default()
        .bucket(&require_config(connection, "bucket")?)
        .operator(&require_secret(connection, "operator")?)
        .password(&require_secret(connection, "password")?)
        .root(&connection_root(connection));
    Operator::new(builder).map_err(format_opendal_error)
}

fn build_webdav(connection: &StorageConnection, require_https: bool) -> Result<Operator, String> {
    let endpoint = require_config(connection, "endpoint")?;
    let parsed = url::Url::parse(&endpoint).map_err(|_| format!("Invalid endpoint: {endpoint}"))?;
    match parsed.scheme() {
        "https" if require_https => {}
        "http" if !require_https => {}
        "http" if require_https => {
            return Err("This connection requires an https:// endpoint".to_string())
        }
        "https" if !require_https => {
            return Err("This connection requires an http:// endpoint".to_string())
        }
        other => return Err(format!("Unsupported endpoint scheme: {other}")),
    }
    let mut builder = opendal::services::Webdav::default()
        .endpoint(&endpoint)
        .root(&connection_root(connection));
    if let Some(username) = secret_value(connection, "username") {
        builder = builder.username(&username);
    }
    if let Some(password) = secret_value(connection, "password") {
        builder = builder.password(&password);
    }
    Operator::new(builder).map_err(format_opendal_error)
}

fn build_dropbox(connection: &StorageConnection) -> Result<Operator, String> {
    let mut builder = opendal::services::Dropbox::default().root(&connection_root(connection));
    // refresh token 优先:access token 两小时过期,OpenDAL 会自动续期。
    match (
        secret_value(connection, "refreshToken"),
        secret_value(connection, "clientId"),
        secret_value(connection, "clientSecret"),
    ) {
        (Some(refresh_token), Some(client_id), Some(client_secret)) => {
            builder = builder
                .refresh_token(&refresh_token)
                .client_id(&client_id)
                .client_secret(&client_secret);
        }
        (Some(_), _, _) => {
            return Err(
                "Dropbox refresh credentials are incomplete; reconnect with your app client ID and client secret"
                    .to_string(),
            )
        }
        _ => {
            builder = builder.access_token(&require_secret(connection, "accessToken")?);
        }
    }
    Operator::new(builder).map_err(format_opendal_error)
}

fn build_gdrive(connection: &StorageConnection) -> Result<Operator, String> {
    let mut builder = opendal::services::Gdrive::default().root(&connection_root(connection));
    match (
        secret_value(connection, "refreshToken"),
        secret_value(connection, "clientId"),
        secret_value(connection, "clientSecret"),
    ) {
        (Some(refresh_token), Some(client_id), Some(client_secret)) => {
            builder = builder
                .refresh_token(&refresh_token)
                .client_id(&client_id)
                .client_secret(&client_secret);
        }
        (Some(_), _, _) => {
            return Err(
                "Google Drive refresh credentials are incomplete; reconnect with your app client ID and client secret"
                    .to_string(),
            )
        }
        _ => {
            builder = builder.access_token(&require_secret(connection, "accessToken")?);
        }
    }
    Operator::new(builder).map_err(format_opendal_error)
}

fn build_onedrive(connection: &StorageConnection) -> Result<Operator, String> {
    let mut builder = opendal::services::Onedrive::default().root(&connection_root(connection));
    match (
        secret_value(connection, "refreshToken"),
        secret_value(connection, "clientId"),
    ) {
        (Some(refresh_token), Some(client_id)) => {
            builder = builder.refresh_token(&refresh_token).client_id(&client_id);
            if let Some(client_secret) = secret_value(connection, "clientSecret") {
                builder = builder.client_secret(&client_secret);
            }
        }
        _ => {
            builder = builder.access_token(&require_secret(connection, "accessToken")?);
        }
    }
    Operator::new(builder).map_err(format_opendal_error)
}

fn build_aliyun_drive(connection: &StorageConnection) -> Result<Operator, String> {
    let mut builder = opendal::services::AliyunDrive::default().root(&connection_root(connection));
    if let Some(drive_type) = config_value(connection, "driveType") {
        builder = builder.drive_type(&drive_type);
    }
    match (
        secret_value(connection, "refreshToken"),
        secret_value(connection, "clientId"),
        secret_value(connection, "clientSecret"),
    ) {
        (Some(refresh_token), Some(client_id), Some(client_secret)) => {
            builder = builder
                .refresh_token(&refresh_token)
                .client_id(&client_id)
                .client_secret(&client_secret);
        }
        (Some(refresh_token), _, _) => {
            builder = builder.refresh_token(&refresh_token);
        }
        _ => {
            builder = builder.access_token(&require_secret(connection, "accessToken")?);
        }
    }
    Operator::new(builder).map_err(format_opendal_error)
}

/// 按连接协议构造 OpenDAL 后端。
pub fn build(connection: &StorageConnection) -> Result<Box<dyn StorageBackend>, String> {
    let operator = match connection.protocol {
        StorageProtocol::S3 => build_s3(connection, None)?,
        StorageProtocol::S3Compatible => build_s3(connection, None)?,
        StorageProtocol::QiniuKodo => build_s3(connection, Some(QINIU_DEFAULT_ENDPOINT))?,
        StorageProtocol::JdCloudOss => build_s3(connection, Some(JD_CLOUD_DEFAULT_ENDPOINT))?,
        StorageProtocol::AliyunOss => build_oss(connection)?,
        StorageProtocol::TencentCos => build_cos(connection)?,
        StorageProtocol::Upyun => build_upyun(connection)?,
        StorageProtocol::WebdavHttps => build_webdav(connection, true)?,
        StorageProtocol::WebdavHttp => build_webdav(connection, false)?,
        StorageProtocol::Dropbox => build_dropbox(connection)?,
        StorageProtocol::GoogleDrive => build_gdrive(connection)?,
        StorageProtocol::OneDrive => build_onedrive(connection)?,
        StorageProtocol::AliyunDrive => build_aliyun_drive(connection)?,
        other => {
            return Err(format!(
                "{} is not handled by the OpenDAL backend",
                other.as_str()
            ))
        }
    };
    // 每个后端实例自带一个当前线程 runtime:调用方已在 spawn_blocking 上,
    // 复用主 runtime 的 handle 会在 block_on 时 panic。
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(Box::new(OpendalBackend {
        operator,
        runtime,
        capability: crate::storage_backend::capability_for(connection.protocol),
    }))
}

/// 前端展示用:某协议默认端点(表单占位符)。
pub fn default_endpoint(protocol: StorageProtocol) -> Option<&'static str> {
    match protocol {
        StorageProtocol::QiniuKodo => Some(QINIU_DEFAULT_ENDPOINT),
        StorageProtocol::JdCloudOss => Some(JD_CLOUD_DEFAULT_ENDPOINT),
        _ => None,
    }
}

/// 连接摘要(不含任何凭据值)。用于回归测试:断言摘要永不泄露密钥。
#[cfg(test)]
fn describe(connection: &StorageConnection) -> std::collections::BTreeMap<String, String> {
    let mut summary = std::collections::BTreeMap::new();
    summary.insert(
        "protocol".to_string(),
        connection.protocol.as_str().to_string(),
    );
    summary.insert("root".to_string(), connection_root(connection));
    if let Some(bucket) = config_value(connection, "bucket") {
        summary.insert("bucket".to_string(), bucket);
    }
    if let Some(endpoint) = config_value(connection, "endpoint") {
        summary.insert("endpoint".to_string(), endpoint);
    }
    summary.insert(
        "credentials".to_string(),
        connection
            .secrets
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(","),
    );
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn connection(protocol: StorageProtocol) -> StorageConnection {
        StorageConnection {
            id: "conn-1".to_string(),
            name: "Test".to_string(),
            group: None,
            protocol,
            config: BTreeMap::new(),
            secrets: BTreeMap::new(),
            created_at: 1,
            last_connected_at: None,
        }
    }

    #[test]
    fn to_opendal_path_drops_leading_slash_and_marks_dirs() {
        assert_eq!(to_opendal_path("/a/b.txt", false), "a/b.txt");
        assert_eq!(to_opendal_path("/a/b", true), "a/b/");
        assert_eq!(to_opendal_path("/", true), "/");
        assert_eq!(to_opendal_path("", false), "/");
    }

    #[test]
    fn to_opendal_path_resolves_traversal_before_stripping() {
        assert_eq!(to_opendal_path("/a/../b", false), "b");
        assert_eq!(to_opendal_path("/../../etc", false), "etc");
    }

    #[test]
    fn connection_root_defaults_to_slash() {
        let mut input = connection(StorageProtocol::S3);
        assert_eq!(connection_root(&input), "/");
        input
            .config
            .insert("root".to_string(), "media/".to_string());
        assert_eq!(connection_root(&input), "/media");
    }

    #[test]
    fn webdav_rejects_scheme_mismatch() {
        let mut input = connection(StorageProtocol::WebdavHttps);
        input
            .config
            .insert("endpoint".to_string(), "http://dav.example.com".to_string());
        let error = build_webdav(&input, true).unwrap_err();
        assert!(error.contains("https://"), "unexpected error: {error}");

        input.config.insert(
            "endpoint".to_string(),
            "https://dav.example.com".to_string(),
        );
        let error = build_webdav(&input, false).unwrap_err();
        assert!(error.contains("http://"), "unexpected error: {error}");
    }

    #[test]
    fn webdav_accepts_matching_scheme() {
        let mut input = connection(StorageProtocol::WebdavHttps);
        input.config.insert(
            "endpoint".to_string(),
            "https://dav.example.com".to_string(),
        );
        assert!(build_webdav(&input, true).is_ok());
    }

    #[test]
    fn s3_requires_a_bucket() {
        let input = connection(StorageProtocol::S3);
        let error = build_s3(&input, None).unwrap_err();
        assert!(error.contains("bucket"), "unexpected error: {error}");
    }

    #[test]
    fn s3_compatible_variants_get_vendor_default_endpoints() {
        assert_eq!(
            default_endpoint(StorageProtocol::QiniuKodo),
            Some(QINIU_DEFAULT_ENDPOINT)
        );
        assert_eq!(
            default_endpoint(StorageProtocol::JdCloudOss),
            Some(JD_CLOUD_DEFAULT_ENDPOINT)
        );
        assert_eq!(default_endpoint(StorageProtocol::S3), None);
    }

    #[test]
    fn qiniu_builds_without_an_explicit_endpoint() {
        let mut input = connection(StorageProtocol::QiniuKodo);
        input
            .config
            .insert("bucket".to_string(), "media".to_string());
        input
            .secrets
            .insert("accessKeyId".to_string(), "ak".to_string());
        input
            .secrets
            .insert("secretAccessKey".to_string(), "sk".to_string());
        assert!(build(&input).is_ok());
    }

    #[test]
    fn describe_never_leaks_secret_values() {
        let mut input = connection(StorageProtocol::S3);
        input
            .config
            .insert("bucket".to_string(), "media".to_string());
        input
            .secrets
            .insert("secretAccessKey".to_string(), "top-secret".to_string());
        let summary = describe(&input);
        let joined = summary.values().cloned().collect::<Vec<_>>().join("|");
        assert!(!joined.contains("top-secret"));
        assert_eq!(summary["credentials"], "secretAccessKey");
    }

    #[test]
    fn dropbox_requires_a_token() {
        let input = connection(StorageProtocol::Dropbox);
        assert!(build_dropbox(&input).is_err());
    }

    #[test]
    fn dropbox_accepts_a_bare_access_token() {
        let mut input = connection(StorageProtocol::Dropbox);
        input
            .secrets
            .insert("accessToken".to_string(), "token".to_string());
        assert!(build_dropbox(&input).is_ok());
    }

    #[test]
    fn unhandled_protocols_are_rejected() {
        let error = build(&connection(StorageProtocol::Smb))
            .err()
            .expect("smb must not be handled here");
        assert!(error.contains("smb"), "unexpected error: {error}");
    }
}
