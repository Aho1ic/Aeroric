//! 百度网盘后端(xpan 开放平台 REST API)。
//!
//! 与 Box 一样是 id 寻址(`fs_id`),但列目录接口接受路径,所以只在需要
//! `fs_id` 的操作(删除/移动/下载直链)前解析一次。
//!
//! 百度开放平台强制要求 client_secret 才能换取 token,因此本协议**只能**
//! 使用用户自建应用(见 `storage_oauth::provider_for`)。
//!
//! 路径约定:百度网盘的用户根目录是 `/`;第三方应用通常被限制在
//! `/apps/<应用名>` 下,这由用户在连接配置的 `root` 里填写。

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;

use crate::storage_backend::{
    join_storage_path, normalize_storage_path, path_basename, path_parent, Capability,
    StorageBackend, StorageEntry, StorageStat,
};
use crate::storage_backend_box::http_error_message;
use crate::storage_conn::StorageConnection;

const XPAN_FILE_BASE: &str = "https://pan.baidu.com/rest/2.0/xpan/file";
const XPAN_MULTIMEDIA_BASE: &str = "https://pan.baidu.com/rest/2.0/xpan/multimedia";
const PCS_UPLOAD_BASE: &str = "https://d.pcs.baidu.com/rest/2.0/pcs/superfile2";
/// 单次上传分片上限(普通用户 4MB)。超过则拒绝,避免静默截断。
const MAX_SINGLE_UPLOAD_BYTES: usize = 4 * 1024 * 1024;
const PAGE_LIMIT: usize = 1000;
/// User-Agent 必须是 pan.baidu.com,否则下载接口返回 403。
const REQUIRED_USER_AGENT: &str = "pan.baidu.com";

#[derive(Debug, Deserialize)]
struct XpanEntry {
    fs_id: u64,
    path: String,
    server_filename: String,
    #[serde(default)]
    isdir: i64,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    server_mtime: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct XpanListResponse {
    #[serde(default)]
    errno: i64,
    #[serde(default)]
    errmsg: Option<String>,
    #[serde(default)]
    list: Vec<XpanEntry>,
}

#[derive(Debug, Deserialize)]
struct XpanSimpleResponse {
    #[serde(default)]
    errno: i64,
    #[serde(default)]
    errmsg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct XpanDownloadResponse {
    #[serde(default)]
    errno: i64,
    #[serde(default)]
    errmsg: Option<String>,
    #[serde(default)]
    list: Vec<XpanDownloadItem>,
}

#[derive(Debug, Deserialize)]
struct XpanDownloadItem {
    #[serde(default)]
    dlink: Option<String>,
}

/// 把百度的 errno 映射为用户可读信息。
pub(crate) fn errno_message(errno: i64, errmsg: Option<&str>) -> String {
    match errno {
        0 => String::new(),
        -6 => "Authorization expired, please reconnect".to_string(),
        -7 | 111 => "Permission denied".to_string(),
        -8 => "A file or folder with that name already exists".to_string(),
        -9 => "No such file or directory".to_string(),
        2 | 31034 => "Rate limited by the service, try again shortly".to_string(),
        31061 => "A file or folder with that name already exists".to_string(),
        31064 => "Permission denied".to_string(),
        other => match errmsg.filter(|value| !value.is_empty()) {
            Some(message) => format!("Baidu Netdisk error {other}: {message}"),
            None => format!("Baidu Netdisk error {other}"),
        },
    }
}

fn check_errno(errno: i64, errmsg: Option<&str>) -> Result<(), String> {
    if errno == 0 {
        return Ok(());
    }
    Err(errno_message(errno, errmsg))
}

pub struct BaiduBackend {
    client: reqwest::Client,
    access_token: String,
    runtime: tokio::runtime::Runtime,
    root: String,
    /// path → fs_id 缓存。
    cache: Mutex<HashMap<String, u64>>,
}

impl BaiduBackend {
    fn absolute(&self, path: &str) -> String {
        if self.root == "/" {
            normalize_storage_path(path)
        } else {
            join_storage_path(
                &self.root,
                normalize_storage_path(path).trim_start_matches('/'),
            )
        }
    }

    fn block<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        self.runtime.block_on(future)
    }

    async fn list_path(&self, absolute: &str) -> Result<Vec<XpanEntry>, String> {
        let mut all = Vec::new();
        let mut start = 0usize;
        loop {
            let response = self
                .client
                .get(XPAN_FILE_BASE)
                .query(&[
                    ("method", "list".to_string()),
                    ("access_token", self.access_token.clone()),
                    ("dir", absolute.to_string()),
                    ("start", start.to_string()),
                    ("limit", PAGE_LIMIT.to_string()),
                ])
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            let body = response.text().await.map_err(|error| error.to_string())?;
            if !status.is_success() {
                return Err(http_error_message(status, &body));
            }
            let page: XpanListResponse =
                serde_json::from_str(&body).map_err(|error| error.to_string())?;
            check_errno(page.errno, page.errmsg.as_deref())?;
            let count = page.list.len();
            all.extend(page.list);
            if count < PAGE_LIMIT {
                break;
            }
            start += count;
        }
        Ok(all)
    }

    async fn resolve_fs_id(&self, absolute: &str) -> Result<u64, String> {
        let normalized = normalize_storage_path(absolute);
        if let Some(cached) = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&normalized).copied())
        {
            return Ok(cached);
        }
        let parent = path_parent(&normalized);
        let name = path_basename(&normalized);
        let entries = self.list_path(&parent).await?;
        let mut found = None;
        if let Ok(mut cache) = self.cache.lock() {
            for entry in &entries {
                cache.insert(normalize_storage_path(&entry.path), entry.fs_id);
                if entry.server_filename == name {
                    found = Some(entry.fs_id);
                }
            }
        }
        found.ok_or_else(|| "No such file or directory".to_string())
    }

    fn invalidate(&self, path: &str) {
        let normalized = normalize_storage_path(path);
        let prefix = if normalized == "/" {
            "/".to_string()
        } else {
            format!("{normalized}/")
        };
        if let Ok(mut cache) = self.cache.lock() {
            cache.retain(|key, _| key != &normalized && !key.starts_with(&prefix));
        }
    }

    /// filemanager 接口(copy / move / rename / delete)统一入口。
    async fn file_manager(
        &self,
        operation: &str,
        filelist: serde_json::Value,
    ) -> Result<(), String> {
        let response = self
            .client
            .post(XPAN_FILE_BASE)
            .query(&[
                ("method", "filemanager"),
                ("access_token", self.access_token.as_str()),
                ("opera", operation),
            ])
            .form(&[
                ("async", "0".to_string()),
                ("filelist", filelist.to_string()),
                ("ondup", "fail".to_string()),
            ])
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let body = response.text().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(http_error_message(status, &body));
        }
        let parsed: XpanSimpleResponse =
            serde_json::from_str(&body).map_err(|error| error.to_string())?;
        check_errno(parsed.errno, parsed.errmsg.as_deref())
    }
}

impl StorageBackend for BaiduBackend {
    fn capability(&self) -> Capability {
        Capability::FULL
    }

    fn read_dir(&self, path: &str) -> Result<Vec<StorageEntry>, String> {
        let display_parent = normalize_storage_path(path);
        let absolute = self.absolute(&display_parent);
        self.block(async {
            let entries = self.list_path(&absolute).await?;
            if let Ok(mut cache) = self.cache.lock() {
                for entry in &entries {
                    cache.insert(normalize_storage_path(&entry.path), entry.fs_id);
                }
            }
            Ok(entries
                .into_iter()
                .map(|entry| {
                    let is_dir = entry.isdir != 0;
                    StorageEntry {
                        path: join_storage_path(&display_parent, &entry.server_filename),
                        name: entry.server_filename,
                        is_dir,
                        size: if is_dir { None } else { entry.size },
                        // server_mtime 是秒级 Unix 时间戳。
                        modified_at_ms: entry
                            .server_mtime
                            .filter(|value| *value > 0)
                            .map(|value| value as u64 * 1000),
                    }
                })
                .collect())
        })
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let absolute = self.absolute(path);
        self.block(async {
            let fs_id = self.resolve_fs_id(&absolute).await?;
            // 先取下载直链,再按直链拉内容。两步都必须带 pan.baidu.com UA。
            let response = self
                .client
                .get(XPAN_MULTIMEDIA_BASE)
                .query(&[
                    ("method", "filemetas".to_string()),
                    ("access_token", self.access_token.clone()),
                    ("fsids", format!("[{fs_id}]")),
                    ("dlink", "1".to_string()),
                ])
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            let body = response.text().await.map_err(|error| error.to_string())?;
            if !status.is_success() {
                return Err(http_error_message(status, &body));
            }
            let parsed: XpanDownloadResponse =
                serde_json::from_str(&body).map_err(|error| error.to_string())?;
            check_errno(parsed.errno, parsed.errmsg.as_deref())?;
            let dlink = parsed
                .list
                .into_iter()
                .find_map(|item| item.dlink)
                .ok_or_else(|| "Baidu Netdisk did not return a download link".to_string())?;
            let response = self
                .client
                .get(&dlink)
                .query(&[("access_token", self.access_token.as_str())])
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(http_error_message(status, &body));
            }
            response
                .bytes()
                .await
                .map(|bytes| bytes.to_vec())
                .map_err(|error| error.to_string())
        })
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MAX_SINGLE_UPLOAD_BYTES {
            return Err(format!(
                "File too large for a single upload ({:.1} MB); Baidu Netdisk requires \
                 chunked upload above 4 MB",
                bytes.len() as f64 / 1024.0 / 1024.0
            ));
        }
        let absolute = self.absolute(path);
        let data = bytes.to_vec();
        let result = self.block(async {
            // superfile2 单分片上传:先 precreate,再上传分片,最后 create。
            let response = self
                .client
                .post(XPAN_FILE_BASE)
                .query(&[
                    ("method", "precreate"),
                    ("access_token", self.access_token.as_str()),
                ])
                .form(&[
                    ("path", absolute.clone()),
                    ("size", data.len().to_string()),
                    ("isdir", "0".to_string()),
                    ("autoinit", "1".to_string()),
                    ("block_list", format!("[\"{}\"]", md5_hex(&data))),
                ])
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            let body = response.text().await.map_err(|error| error.to_string())?;
            if !status.is_success() {
                return Err(http_error_message(status, &body));
            }
            let precreate: serde_json::Value =
                serde_json::from_str(&body).map_err(|error| error.to_string())?;
            check_errno(
                precreate.get("errno").and_then(|v| v.as_i64()).unwrap_or(0),
                precreate.get("errmsg").and_then(|v| v.as_str()),
            )?;
            let uploadid = precreate
                .get("uploadid")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "Baidu Netdisk did not return an upload id".to_string())?
                .to_string();

            let form = reqwest::multipart::Form::new().part(
                "file",
                reqwest::multipart::Part::bytes(data.clone()).file_name(path_basename(&absolute)),
            );
            let response = self
                .client
                .post(PCS_UPLOAD_BASE)
                .query(&[
                    ("method", "upload"),
                    ("access_token", self.access_token.as_str()),
                    ("type", "tmpfile"),
                    ("path", absolute.as_str()),
                    ("uploadid", uploadid.as_str()),
                    ("partseq", "0"),
                ])
                .multipart(form)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(http_error_message(status, &body));
            }

            let response = self
                .client
                .post(XPAN_FILE_BASE)
                .query(&[
                    ("method", "create"),
                    ("access_token", self.access_token.as_str()),
                ])
                .form(&[
                    ("path", absolute.clone()),
                    ("size", data.len().to_string()),
                    ("isdir", "0".to_string()),
                    ("uploadid", uploadid),
                    ("block_list", format!("[\"{}\"]", md5_hex(&data))),
                ])
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            let body = response.text().await.map_err(|error| error.to_string())?;
            if !status.is_success() {
                return Err(http_error_message(status, &body));
            }
            let parsed: XpanSimpleResponse =
                serde_json::from_str(&body).map_err(|error| error.to_string())?;
            check_errno(parsed.errno, parsed.errmsg.as_deref())
        });
        self.invalidate(&path_parent(&absolute));
        result
    }

    fn create_dir(&self, path: &str) -> Result<(), String> {
        let absolute = self.absolute(path);
        let result = self.block(async {
            let response = self
                .client
                .post(XPAN_FILE_BASE)
                .query(&[
                    ("method", "create"),
                    ("access_token", self.access_token.as_str()),
                ])
                .form(&[
                    ("path", absolute.clone()),
                    ("isdir", "1".to_string()),
                    ("size", "0".to_string()),
                ])
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            let body = response.text().await.map_err(|error| error.to_string())?;
            if !status.is_success() {
                return Err(http_error_message(status, &body));
            }
            let parsed: XpanSimpleResponse =
                serde_json::from_str(&body).map_err(|error| error.to_string())?;
            check_errno(parsed.errno, parsed.errmsg.as_deref())
        });
        self.invalidate(&path_parent(&absolute));
        result
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        let path = crate::storage_backend::validate_storage_mutation_path(path)?;
        let absolute = self.absolute(&path);
        let result = self.block(async {
            self.file_manager("delete", serde_json::json!([absolute.clone()]))
                .await
        });
        self.invalidate(&path_parent(&absolute));
        result
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let from = crate::storage_backend::validate_storage_mutation_path(from)?;
        let from_absolute = self.absolute(&from);
        let to_absolute = self.absolute(to);
        let from_parent = path_parent(&from_absolute);
        let to_parent = path_parent(&to_absolute);
        let result = self.block(async {
            if from_parent == to_parent {
                // 同目录:rename 只需新名字。
                self.file_manager(
                    "rename",
                    serde_json::json!([{
                        "path": from_absolute.clone(),
                        "newname": path_basename(&to_absolute),
                    }]),
                )
                .await
            } else {
                self.file_manager(
                    "move",
                    serde_json::json!([{
                        "path": from_absolute.clone(),
                        "dest": to_parent.clone(),
                        "newname": path_basename(&to_absolute),
                    }]),
                )
                .await
            }
        });
        self.invalidate(&from_parent);
        self.invalidate(&to_parent);
        result
    }

    fn copy(&self, from: &str, to: &str) -> Result<(), String> {
        let from_absolute = self.absolute(from);
        let to_absolute = self.absolute(to);
        let to_parent = path_parent(&to_absolute);
        let result = self.block(async {
            self.file_manager(
                "copy",
                serde_json::json!([{
                    "path": from_absolute.clone(),
                    "dest": to_parent.clone(),
                    "newname": path_basename(&to_absolute),
                }]),
            )
            .await
        });
        self.invalidate(&to_parent);
        result
    }

    fn stat(&self, path: &str) -> Result<StorageStat, String> {
        let absolute = self.absolute(path);
        let normalized = normalize_storage_path(&absolute);
        if normalized == "/" {
            return Ok(StorageStat {
                is_dir: true,
                size: None,
                modified_at_ms: None,
            });
        }
        // 列父目录后按名字取,避免 filemetas 需要先解析 fs_id 的额外往返。
        let name = path_basename(&normalized);
        let parent = path_parent(&normalized);
        self.block(async {
            let entries = self.list_path(&parent).await?;
            let entry = entries
                .into_iter()
                .find(|entry| entry.server_filename == name)
                .ok_or_else(|| "No such file or directory".to_string())?;
            let is_dir = entry.isdir != 0;
            Ok(StorageStat {
                is_dir,
                size: if is_dir { None } else { entry.size },
                modified_at_ms: entry
                    .server_mtime
                    .filter(|value| *value > 0)
                    .map(|value| value as u64 * 1000),
            })
        })
    }
}

/// 计算 MD5 十六进制串。百度的 block_list 要求分片 MD5。
fn md5_hex(bytes: &[u8]) -> String {
    // 复用项目已有的 sha2 不行(必须是 MD5),这里用最小实现。
    md5_digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// RFC 1321 MD5。仅用于百度网盘要求的分片校验值,不用于任何安全用途。
fn md5_digest(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: [u32; 64] =
        std::array::from_fn(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32);

    let mut a0: u32 = 0x6745_2301;
    let mut b0: u32 = 0xefcd_ab89;
    let mut c0: u32 = 0x98ba_dcfe;
    let mut d0: u32 = 0x1032_5476;

    let mut message = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in message.chunks(64) {
        let m: [u32; 16] = std::array::from_fn(|i| {
            u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ])
        });
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

pub(crate) fn build_typed(connection: &StorageConnection) -> Result<BaiduBackend, String> {
    let access_token = connection
        .secrets
        .get("accessToken")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "Baidu Netdisk requires authorization; connect the account first".to_string()
        })?
        .to_string();
    let root = connection
        .config
        .get("root")
        .map(|value| normalize_storage_path(value))
        .unwrap_or_else(|| "/".to_string());
    // 下载接口强制校验 UA;不设置会稳定 403。
    let client = reqwest::Client::builder()
        .user_agent(REQUIRED_USER_AGENT)
        .build()
        .map_err(|error| error.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(BaiduBackend {
        client,
        access_token,
        runtime,
        root,
        cache: Mutex::new(HashMap::new()),
    })
}

pub fn build(connection: &StorageConnection) -> Result<Box<dyn StorageBackend>, String> {
    Ok(Box::new(build_typed(connection)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn connection() -> StorageConnection {
        StorageConnection {
            id: "conn-1".to_string(),
            name: "Baidu".to_string(),
            group: None,
            protocol: crate::storage_conn::StorageProtocol::BaiduNetdisk,
            config: BTreeMap::new(),
            secrets: BTreeMap::new(),
            created_at: 1,
            last_connected_at: None,
        }
    }

    #[test]
    fn build_requires_an_access_token() {
        let error = build(&connection())
            .err()
            .expect("baidu must require a token");
        assert!(error.contains("authorization"), "unexpected: {error}");
    }

    #[test]
    fn md5_matches_known_rfc1321_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            md5_hex(b"message digest"),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
        assert_eq!(
            md5_hex(b"abcdefghijklmnopqrstuvwxyz"),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
        assert_eq!(
            md5_hex(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    #[test]
    fn md5_handles_a_block_boundary_length() {
        // 56 字节正好触发额外填充块。
        let input = vec![b'x'; 56];
        assert_eq!(md5_hex(&input).len(), 32);
        let input = vec![b'x'; 64];
        assert_eq!(md5_hex(&input).len(), 32);
    }

    #[test]
    fn errno_zero_is_not_an_error() {
        assert!(check_errno(0, None).is_ok());
        assert_eq!(errno_message(0, None), "");
    }

    #[test]
    fn errno_maps_known_failures() {
        assert_eq!(
            errno_message(-6, None),
            "Authorization expired, please reconnect"
        );
        assert_eq!(errno_message(-9, None), "No such file or directory");
        assert_eq!(
            errno_message(-8, None),
            "A file or folder with that name already exists"
        );
        assert_eq!(
            errno_message(31061, None),
            "A file or folder with that name already exists"
        );
        assert_eq!(errno_message(111, None), "Permission denied");
    }

    #[test]
    fn unknown_errno_includes_the_code() {
        let message = errno_message(42, Some("weird"));
        assert!(message.contains("42"));
        assert!(message.contains("weird"));
        assert!(errno_message(42, None).contains("42"));
    }

    #[test]
    fn oversized_writes_are_refused_before_any_request() {
        let mut input = connection();
        input
            .secrets
            .insert("accessToken".to_string(), "token".to_string());
        let backend = build_typed(&input).unwrap();
        let oversized = vec![0u8; MAX_SINGLE_UPLOAD_BYTES + 1];
        let error = backend.write("/a.bin", &oversized).unwrap_err();
        assert!(error.contains("chunked upload"), "unexpected: {error}");
    }

    #[test]
    fn root_confines_paths() {
        let mut input = connection();
        input
            .secrets
            .insert("accessToken".to_string(), "token".to_string());
        input
            .config
            .insert("root".to_string(), "/apps/aeroric".to_string());
        let backend = build_typed(&input).unwrap();
        assert_eq!(backend.absolute("/a.txt"), "/apps/aeroric/a.txt");
        assert_eq!(backend.absolute("/../../a.txt"), "/apps/aeroric/a.txt");
    }

    #[test]
    fn cache_invalidation_clears_the_subtree_only() {
        let mut input = connection();
        input
            .secrets
            .insert("accessToken".to_string(), "token".to_string());
        let backend = build_typed(&input).unwrap();
        {
            let mut cache = backend.cache.lock().unwrap();
            cache.insert("/a".to_string(), 1);
            cache.insert("/a/b".to_string(), 2);
            cache.insert("/other".to_string(), 3);
        }
        backend.invalidate("/a");
        let cache = backend.cache.lock().unwrap();
        assert!(!cache.contains_key("/a"));
        assert!(!cache.contains_key("/a/b"));
        assert!(cache.contains_key("/other"));
    }
}
