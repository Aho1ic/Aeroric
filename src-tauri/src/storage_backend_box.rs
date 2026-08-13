//! Box 后端(官方 REST API,`https://api.box.com/2.0`)。
//!
//! Box 的 WebDAV 网关已于 2023-04-28 停止服务,所以必须走官方 API。
//! Box 是 **id 寻址** 的:每个文件/文件夹有数字 id,没有"按路径取对象"的
//! 接口。因此这里维护一个 path → id 缓存,逐级解析路径。

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;

use crate::storage_backend::{
    join_storage_path, normalize_storage_path, path_basename, path_parent, Capability,
    StorageBackend, StorageEntry, StorageStat,
};
use crate::storage_conn::StorageConnection;

const BOX_API_BASE: &str = "https://api.box.com/2.0";
const BOX_UPLOAD_BASE: &str = "https://upload.box.com/api/2.0";
const ROOT_FOLDER_ID: &str = "0";
const PAGE_LIMIT: usize = 1000;

#[derive(Debug, Deserialize)]
struct BoxItem {
    id: String,
    #[serde(rename = "type")]
    item_type: String,
    name: String,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    modified_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BoxItemPage {
    #[serde(default)]
    entries: Vec<BoxItem>,
    #[serde(default)]
    total_count: Option<u64>,
}

/// 缓存条目:id 与是否为目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoxNode {
    pub id: String,
    pub is_dir: bool,
}

pub struct BoxBackend {
    client: reqwest::Client,
    access_token: String,
    runtime: tokio::runtime::Runtime,
    root: String,
    /// path → node 缓存。写操作后要按父目录失效。
    cache: Mutex<HashMap<String, BoxNode>>,
}

/// 解析 Box 的 RFC 3339 时间戳为毫秒。
pub(crate) fn parse_box_timestamp(value: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.timestamp_millis())
        .and_then(|millis| u64::try_from(millis).ok())
}

impl BoxBackend {
    /// 内部路径 → Box 上的绝对路径(叠加连接 root)。
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

    async fn list_folder(&self, folder_id: &str) -> Result<Vec<BoxItem>, String> {
        let mut items = Vec::new();
        let mut offset = 0usize;
        loop {
            let response = self
                .client
                .get(format!("{BOX_API_BASE}/folders/{folder_id}/items"))
                .bearer_auth(&self.access_token)
                .query(&[
                    ("limit", PAGE_LIMIT.to_string()),
                    ("offset", offset.to_string()),
                    ("fields", "id,type,name,size,modified_at".to_string()),
                ])
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let page: BoxItemPage = parse_json(response).await?;
            let count = page.entries.len();
            items.extend(page.entries);
            offset += count;
            // 没有下一页,或服务端已给出总数且已取满。
            if count == 0
                || count < PAGE_LIMIT
                || page
                    .total_count
                    .map(|total| items.len() as u64 >= total)
                    .unwrap_or(false)
            {
                break;
            }
        }
        Ok(items)
    }

    /// 逐级解析绝对路径为 Box node。
    async fn resolve(&self, absolute: &str) -> Result<BoxNode, String> {
        let normalized = normalize_storage_path(absolute);
        if normalized == "/" {
            return Ok(BoxNode {
                id: ROOT_FOLDER_ID.to_string(),
                is_dir: true,
            });
        }
        if let Some(cached) = self
            .cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&normalized).cloned())
        {
            return Ok(cached);
        }

        let parent = path_parent(&normalized);
        let name = path_basename(&normalized);
        // 递归解析父目录(根会立即命中上面的分支)。
        let parent_node = Box::pin(self.resolve(&parent)).await?;
        if !parent_node.is_dir {
            return Err("No such file or directory".to_string());
        }
        let items = self.list_folder(&parent_node.id).await?;
        let mut found = None;
        if let Ok(mut cache) = self.cache.lock() {
            for item in &items {
                let node = BoxNode {
                    id: item.id.clone(),
                    is_dir: item.item_type == "folder",
                };
                let item_path = join_storage_path(&parent, &item.name);
                if item.name == name {
                    found = Some(node.clone());
                }
                cache.insert(item_path, node);
            }
        }
        found.ok_or_else(|| "No such file or directory".to_string())
    }

    /// 使某路径及其子树的缓存失效。
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
}

/// 解析 JSON 响应,把 HTTP 错误转成用户可读信息。
async fn parse_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, String> {
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(http_error_message(status, &body));
    }
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

/// 统一的 HTTP 错误文案。不回显完整响应体,避免把 token 之类的内容带出去。
pub(crate) fn http_error_message(status: reqwest::StatusCode, body: &str) -> String {
    match status.as_u16() {
        401 => "Authorization expired, please reconnect".to_string(),
        403 => "Permission denied".to_string(),
        404 => "No such file or directory".to_string(),
        409 => "A file or folder with that name already exists".to_string(),
        429 => "Rate limited by the service, try again shortly".to_string(),
        _ => {
            // 取服务端 message 字段(若有),否则只报状态码。
            serde_json::from_str::<serde_json::Value>(body)
                .ok()
                .and_then(|value| {
                    value
                        .get("message")
                        .or_else(|| value.get("error_description"))
                        .or_else(|| value.get("errmsg"))
                        .and_then(|message| message.as_str().map(str::to_string))
                })
                .unwrap_or_else(|| format!("Request failed ({status})"))
        }
    }
}

impl StorageBackend for BoxBackend {
    fn capability(&self) -> Capability {
        Capability::FULL
    }

    fn read_dir(&self, path: &str) -> Result<Vec<StorageEntry>, String> {
        let display_parent = normalize_storage_path(path);
        let absolute = self.absolute(&display_parent);
        self.block(async {
            let node = self.resolve(&absolute).await?;
            if !node.is_dir {
                return Err("Not a directory".to_string());
            }
            let items = self.list_folder(&node.id).await?;
            if let Ok(mut cache) = self.cache.lock() {
                for item in &items {
                    cache.insert(
                        join_storage_path(&absolute, &item.name),
                        BoxNode {
                            id: item.id.clone(),
                            is_dir: item.item_type == "folder",
                        },
                    );
                }
            }
            Ok(items
                .into_iter()
                .map(|item| {
                    let is_dir = item.item_type == "folder";
                    StorageEntry {
                        path: join_storage_path(&display_parent, &item.name),
                        name: item.name,
                        is_dir,
                        size: if is_dir { None } else { item.size },
                        modified_at_ms: item.modified_at.as_deref().and_then(parse_box_timestamp),
                    }
                })
                .collect())
        })
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let absolute = self.absolute(path);
        self.block(async {
            let node = self.resolve(&absolute).await?;
            if node.is_dir {
                return Err("Cannot read a directory".to_string());
            }
            let response = self
                .client
                .get(format!("{BOX_API_BASE}/files/{}/content", node.id))
                .bearer_auth(&self.access_token)
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
        let absolute = self.absolute(path);
        let parent = path_parent(&absolute);
        let name = path_basename(&absolute);
        let data = bytes.to_vec();
        let result = self.block(async {
            let parent_node = self.resolve(&parent).await?;
            // 已存在则走 new version 端点,否则新建。
            let existing = self
                .resolve(&absolute)
                .await
                .ok()
                .filter(|node| !node.is_dir);
            let url = match &existing {
                Some(node) => format!("{BOX_UPLOAD_BASE}/files/{}/content", node.id),
                None => format!("{BOX_UPLOAD_BASE}/files/content"),
            };
            let attributes = match &existing {
                Some(_) => serde_json::json!({ "name": name }),
                None => serde_json::json!({
                    "name": name,
                    "parent": { "id": parent_node.id },
                }),
            };
            let form = reqwest::multipart::Form::new()
                .text("attributes", attributes.to_string())
                .part(
                    "file",
                    reqwest::multipart::Part::bytes(data).file_name(name.clone()),
                );
            let response = self
                .client
                .post(url)
                .bearer_auth(&self.access_token)
                .multipart(form)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(http_error_message(status, &body));
            }
            Ok(())
        });
        self.invalidate(&parent);
        result
    }

    fn create_dir(&self, path: &str) -> Result<(), String> {
        let absolute = self.absolute(path);
        let parent = path_parent(&absolute);
        let name = path_basename(&absolute);
        let result = self.block(async {
            let parent_node = self.resolve(&parent).await?;
            let response = self
                .client
                .post(format!("{BOX_API_BASE}/folders"))
                .bearer_auth(&self.access_token)
                .json(&serde_json::json!({
                    "name": name,
                    "parent": { "id": parent_node.id },
                }))
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(http_error_message(status, &body));
            }
            Ok(())
        });
        self.invalidate(&parent);
        result
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        let absolute = self.absolute(path);
        let result = self.block(async {
            let node = self.resolve(&absolute).await?;
            let url = if node.is_dir {
                format!("{BOX_API_BASE}/folders/{}?recursive=true", node.id)
            } else {
                format!("{BOX_API_BASE}/files/{}", node.id)
            };
            let response = self
                .client
                .delete(url)
                .bearer_auth(&self.access_token)
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(http_error_message(status, &body));
            }
            Ok(())
        });
        self.invalidate(&path_parent(&absolute));
        result
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let from_absolute = self.absolute(from);
        let to_absolute = self.absolute(to);
        let result = self.block(async {
            let node = self.resolve(&from_absolute).await?;
            let target_parent = self.resolve(&path_parent(&to_absolute)).await?;
            let url = if node.is_dir {
                format!("{BOX_API_BASE}/folders/{}", node.id)
            } else {
                format!("{BOX_API_BASE}/files/{}", node.id)
            };
            let response = self
                .client
                .put(url)
                .bearer_auth(&self.access_token)
                .json(&serde_json::json!({
                    "name": path_basename(&to_absolute),
                    "parent": { "id": target_parent.id },
                }))
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(http_error_message(status, &body));
            }
            Ok(())
        });
        self.invalidate(&path_parent(&from_absolute));
        self.invalidate(&path_parent(&to_absolute));
        result
    }

    fn copy(&self, from: &str, to: &str) -> Result<(), String> {
        let from_absolute = self.absolute(from);
        let to_absolute = self.absolute(to);
        let result = self.block(async {
            let node = self.resolve(&from_absolute).await?;
            let target_parent = self.resolve(&path_parent(&to_absolute)).await?;
            let url = if node.is_dir {
                format!("{BOX_API_BASE}/folders/{}/copy", node.id)
            } else {
                format!("{BOX_API_BASE}/files/{}/copy", node.id)
            };
            let response = self
                .client
                .post(url)
                .bearer_auth(&self.access_token)
                .json(&serde_json::json!({
                    "parent": { "id": target_parent.id },
                    "name": path_basename(&to_absolute),
                }))
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let status = response.status();
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                return Err(http_error_message(status, &body));
            }
            Ok(())
        });
        self.invalidate(&path_parent(&to_absolute));
        result
    }

    fn stat(&self, path: &str) -> Result<StorageStat, String> {
        let absolute = self.absolute(path);
        if normalize_storage_path(&absolute) == "/" {
            return Ok(StorageStat {
                is_dir: true,
                size: None,
                modified_at_ms: None,
            });
        }
        self.block(async {
            let node = self.resolve(&absolute).await?;
            if node.is_dir {
                return Ok(StorageStat {
                    is_dir: true,
                    size: None,
                    modified_at_ms: None,
                });
            }
            let response = self
                .client
                .get(format!("{BOX_API_BASE}/files/{}", node.id))
                .bearer_auth(&self.access_token)
                .query(&[("fields", "id,type,name,size,modified_at")])
                .send()
                .await
                .map_err(|error| error.to_string())?;
            let item: BoxItem = parse_json(response).await?;
            Ok(StorageStat {
                is_dir: false,
                size: item.size,
                modified_at_ms: item.modified_at.as_deref().and_then(parse_box_timestamp),
            })
        })
    }
}

/// 构造具体类型(测试可直接检查内部状态)。
pub(crate) fn build_typed(connection: &StorageConnection) -> Result<BoxBackend, String> {
    let access_token = connection
        .secrets
        .get("accessToken")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Box requires authorization; connect the account first".to_string())?
        .to_string();
    let root = connection
        .config
        .get("root")
        .map(|value| normalize_storage_path(value))
        .unwrap_or_else(|| "/".to_string());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    Ok(BoxBackend {
        client: reqwest::Client::new(),
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
            name: "Box".to_string(),
            group: None,
            protocol: crate::storage_conn::StorageProtocol::Box,
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
            .expect("box must require a token");
        assert!(error.contains("authorization"), "unexpected: {error}");
    }

    #[test]
    fn build_succeeds_with_a_token() {
        let mut input = connection();
        input
            .secrets
            .insert("accessToken".to_string(), "token".to_string());
        assert!(build(&input).is_ok());
    }

    #[test]
    fn box_timestamps_parse_to_millis() {
        assert_eq!(parse_box_timestamp("1970-01-01T00:00:01Z"), Some(1_000));
        assert!(parse_box_timestamp("2024-05-01T12:00:00-07:00").is_some());
        assert_eq!(parse_box_timestamp("not-a-date"), None);
    }

    #[test]
    fn http_errors_map_to_actionable_messages() {
        assert_eq!(
            http_error_message(reqwest::StatusCode::UNAUTHORIZED, ""),
            "Authorization expired, please reconnect"
        );
        assert_eq!(
            http_error_message(reqwest::StatusCode::NOT_FOUND, ""),
            "No such file or directory"
        );
        assert_eq!(
            http_error_message(reqwest::StatusCode::CONFLICT, ""),
            "A file or folder with that name already exists"
        );
        assert_eq!(
            http_error_message(reqwest::StatusCode::TOO_MANY_REQUESTS, ""),
            "Rate limited by the service, try again shortly"
        );
    }

    #[test]
    fn http_errors_prefer_the_service_message() {
        let message = http_error_message(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"message":"Invalid folder name"}"#,
        );
        assert_eq!(message, "Invalid folder name");
    }

    #[test]
    fn http_errors_fall_back_to_the_status_code() {
        let message = http_error_message(reqwest::StatusCode::BAD_GATEWAY, "<html>oops</html>");
        assert!(message.contains("502"));
        // 不能把整个响应体倒给用户。
        assert!(!message.contains("<html>"));
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
            for path in ["/a", "/a/b", "/a/b/c", "/other"] {
                cache.insert(
                    path.to_string(),
                    BoxNode {
                        id: path.to_string(),
                        is_dir: true,
                    },
                );
            }
        }
        backend.invalidate("/a");
        let cache = backend.cache.lock().unwrap();
        assert!(!cache.contains_key("/a"));
        assert!(!cache.contains_key("/a/b"));
        assert!(!cache.contains_key("/a/b/c"));
        assert!(cache.contains_key("/other"));
    }

    #[test]
    fn root_config_prefixes_all_paths() {
        let mut input = connection();
        input
            .secrets
            .insert("accessToken".to_string(), "token".to_string());
        input
            .config
            .insert("root".to_string(), "/Projects".to_string());
        let backend = build_typed(&input).unwrap();
        assert_eq!(backend.absolute("/a.txt"), "/Projects/a.txt");
        // 逃逸尝试被夹回 root 之下。
        assert_eq!(backend.absolute("/../../a.txt"), "/Projects/a.txt");
    }
}
