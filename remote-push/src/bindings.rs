//! 设备绑定表:APNs device token ↔ (user, device_id)。
//!
//! 绑定发生的时机是**手机在已认证 E2EE 会话里**上报 token(见 core
//! `remote.push.register`):宿主把该 token 连同一个 per-user 凭据 id 转给发送端。
//! 因此发送端从来不需要知道 device token 属于哪台 Mac、哪个项目 —— 只有
//! `(user, device_id)` 这层路由关系。
//!
//! 失效路径有三条,都必须真的解绑:
//! * APNs 回 410/`BadDeviceToken`(设备卸载/重装);
//! * 用户在 Mac 上撤销设备(`remote.revokeDevice` → 宿主 `push.unregister`);
//! * 同一 device_id 换 token(重装后 token 变了),旧 token 直接覆盖。
//!
//! 落盘只存必要字段,0600 权限,与 `credentials.json` 同口径。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 一条绑定:`key = format!("{user}\u{1}{device_id}")`,避免二维 map 的嵌套锁。
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Binding {
    user: String,
    device_id: String,
    platform: String,
    token: String,
    updated_at_ms: i64,
}

#[derive(Serialize, Deserialize, Default)]
struct BindingFile {
    version: u32,
    bindings: Vec<Binding>,
}

/// 绑定表。`path = None` 时纯内存(测试/联调)。
pub struct BindingStore {
    bindings: BTreeMap<String, Binding>,
    path: Option<PathBuf>,
}

fn key(user: &str, device_id: &str) -> String {
    format!("{user}\u{1}{device_id}")
}

impl BindingStore {
    pub fn in_memory() -> Self {
        Self {
            bindings: BTreeMap::new(),
            path: None,
        }
    }

    pub fn load(path: Option<PathBuf>) -> Result<Self, String> {
        let mut store = Self {
            bindings: BTreeMap::new(),
            path,
        };
        if let Some(file) = &store.path {
            if file.exists() {
                let raw = std::fs::read_to_string(file)
                    .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
                let parsed: BindingFile = serde_json::from_str(&raw)
                    .map_err(|e| format!("cannot parse {}: {e}", file.display()))?;
                for binding in parsed.bindings {
                    store
                        .bindings
                        .insert(key(&binding.user, &binding.device_id), binding);
                }
            }
        }
        Ok(store)
    }

    /// 绑定(或按空 token 解绑)。返回是否改变了绑定表 —— 宿主据此判断
    /// 「这条 register 是否值得写审计」。
    pub fn put(
        &mut self,
        user: &str,
        device_id: &str,
        platform: &str,
        token: &str,
        now_ms: i64,
        max_devices_per_user: usize,
    ) -> Result<bool, String> {
        if token.trim().is_empty() {
            let removed = self.bindings.remove(&key(user, device_id)).is_some();
            if removed {
                self.persist()?;
            }
            return Ok(removed);
        }
        if !self.bindings.contains_key(&key(user, device_id)) {
            // 设备数上限:挡住「一个凭据绑无限设备」的滥用面。
            let existing = self
                .bindings
                .values()
                .filter(|binding| binding.user == user)
                .count();
            if existing >= max_devices_per_user {
                return Err(format!(
                    "user already has {existing} bound devices (limit {max_devices_per_user})"
                ));
            }
        }
        self.bindings.insert(
            key(user, device_id),
            Binding {
                user: user.to_string(),
                device_id: device_id.to_string(),
                platform: platform.to_string(),
                token: token.to_string(),
                updated_at_ms: now_ms,
            },
        );
        self.persist()?;
        Ok(true)
    }

    /// 按 device id 解绑(设备撤销路径)。返回被解绑的条数。
    pub fn unbind_device(&mut self, device_id: &str) -> Result<usize, String> {
        let before = self.bindings.len();
        self.bindings
            .retain(|_, binding| binding.device_id != device_id);
        let removed = before - self.bindings.len();
        if removed > 0 {
            self.persist()?;
        }
        Ok(removed)
    }

    /// APNs 判定 token 失效时调用。**按 token 精确解绑**,避免误伤同一设备的新 token。
    pub fn invalidate_token(&mut self, token: &str) -> Result<Vec<String>, String> {
        let victims: Vec<String> = self
            .bindings
            .iter()
            .filter(|(_, binding)| binding.token == token)
            .map(|(k, _)| k.clone())
            .collect();
        for victim in &victims {
            self.bindings.remove(victim);
        }
        if !victims.is_empty() {
            self.persist()?;
        }
        Ok(victims)
    }

    /// 一次推送的目标 token 列表。返回 `device_id -> token`。
    pub fn tokens_for(&self, user: &str, device_ids: &[String]) -> Vec<(String, String)> {
        device_ids
            .iter()
            .filter_map(|device_id| {
                self.bindings
                    .get(&key(user, device_id))
                    .map(|binding| (binding.device_id.clone(), binding.token.clone()))
            })
            .collect()
    }

    /// 该用户名下全部设备(诊断/撤销用),按 device_id 排序保证输出稳定。
    pub fn devices_of(&self, user: &str) -> Vec<(String, String, i64)> {
        self.bindings
            .values()
            .filter(|binding| binding.user == user)
            .map(|binding| {
                (
                    binding.device_id.clone(),
                    binding.platform.clone(),
                    binding.updated_at_ms,
                )
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    fn persist(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let file = BindingFile {
            version: 1,
            bindings: self.bindings.values().cloned().collect(),
        };
        let raw = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        crate::auth::write_private(path, &raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_unbind_and_rebind_semantics() {
        let mut store = BindingStore::in_memory();
        assert!(store.put("u1", "d1", "ios", "tok-a", 1, 8).unwrap());
        assert_eq!(store.len(), 1);
        // 同 device 重装换 token:覆盖,不新增条目。
        assert!(store.put("u1", "d1", "ios", "tok-b", 2, 8).unwrap());
        assert_eq!(store.len(), 1);
        assert_eq!(
            store.tokens_for("u1", &["d1".to_string()]),
            vec![("d1".to_string(), "tok-b".to_string())]
        );

        // 空 token = 解绑;再解绑一次不改动任何东西。
        assert!(store.put("u1", "d1", "ios", "", 3, 8).unwrap());
        assert!(!store.put("u1", "d1", "ios", "", 4, 8).unwrap());
        assert!(store.is_empty());
    }

    #[test]
    fn device_limit_is_enforced_per_user() {
        let mut store = BindingStore::in_memory();
        assert!(store.put("u1", "d1", "ios", "t1", 0, 2).unwrap());
        assert!(store.put("u1", "d2", "ios", "t2", 0, 2).unwrap());
        let error = store.put("u1", "d3", "ios", "t3", 0, 2).unwrap_err();
        assert!(error.contains("limit 2"), "{error}");
        // 另一个凭据不受影响。
        assert!(store.put("u2", "d3", "ios", "t3", 0, 2).unwrap());
        // 已有设备换 token 不占新名额。
        assert!(store.put("u1", "d1", "ios", "t1b", 0, 2).unwrap());
    }

    #[test]
    fn only_the_named_device_is_unbound_and_only_the_exact_token_invalidated() {
        let mut store = BindingStore::in_memory();
        store.put("u1", "d1", "ios", "tok-1", 0, 8).unwrap();
        store.put("u1", "d2", "ipados", "tok-2", 0, 8).unwrap();

        assert_eq!(store.invalidate_token("tok-1").unwrap().len(), 1);
        assert_eq!(store.len(), 1);
        assert_eq!(store.tokens_for("u1", &["d2".into()]).len(), 1);

        // 撤销设备按 device_id,与 token 无关。
        assert_eq!(store.unbind_device("d2").unwrap(), 1);
        assert_eq!(store.unbind_device("d2").unwrap(), 0);
        assert!(store.is_empty());
    }

    #[test]
    fn bindings_persist_privately_and_reload() {
        let dir = std::env::temp_dir().join(format!("aeroric-push-bind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("bindings.json");
        {
            let mut store = BindingStore::load(Some(path.clone())).unwrap();
            store.put("u1", "d1", "ios", "tok", 7, 8).unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        let reloaded = BindingStore::load(Some(path.clone())).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded.devices_of("u1").len(), 1);
        assert_eq!(reloaded.devices_of("u1")[0].0, "d1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tokens_for_ignores_unknown_device_ids() {
        let mut store = BindingStore::in_memory();
        store.put("u1", "d1", "ios", "tok", 0, 8).unwrap();
        let tokens = store.tokens_for("u1", &["d1".into(), "ghost".into()]);
        assert_eq!(tokens.len(), 1);
        // 别人的 device id 也取不到 token。
        assert!(store.tokens_for("u2", &["d1".into()]).is_empty());
    }
}
