//! 发送端配置。所有敏感值只从环境变量/文件读,且**不**提供任何把私钥
//! 打印出来的路径(`Debug` / 日志都手写)。

use std::path::PathBuf;

/// APNs 目标环境。开发与生产是两台不同的主机(Apple 文档),不能混用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApnsEnvironment {
    Sandbox,
    Production,
}

impl ApnsEnvironment {
    /// Apple 文档给出的 provider 主机。注意域名是 `push.apple.com` 的子域,
    /// 不是 `courier`。
    pub fn host(self) -> &'static str {
        match self {
            Self::Sandbox => "api.sandbox.push.apple.com",
            Self::Production => "api.push.apple.com",
        }
    }

    pub fn from_env(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") | Some("sandbox") => Ok(Self::Sandbox),
            Some("production") | Some("prod") => Ok(Self::Production),
            Some(other) => Err(format!(
                "APNS_ENVIRONMENT must be 'sandbox' or 'production', got {other:?}"
            )),
        }
    }
}

/// 发送端配置。
///
/// 刻意**不派生 `Debug`**:配置里带着 APNs 私钥与 per-user 凭据摘要,一个
/// 随手 `{:?}` 就会把密钥写进日志。需要诊断的地方用 [`PushConfig::describe`]。
#[derive(Clone)]
pub struct PushConfig {
    pub port: u16,
    /// APNs 认证密钥的 PKCS#8 PEM 内容(`.p8` 文件正文)。
    pub apns_key_pem: String,
    /// 密钥 ID(`kid` header;Apple 后台的 Key ID)。
    pub apns_key_id: String,
    /// 团队 ID(JWT `iss`)。
    pub apns_team_id: String,
    /// APNs topic = app bundle id(例如 `com.aeroric.ios.companion`)。
    pub apns_topic: String,
    pub apns_environment: ApnsEnvironment,
    /// 覆盖 APNs 基址。只用于本地联调/测试的假 APNs;生产留空。
    pub apns_base_override: Option<String>,
    /// per-user 凭据摘要文件(见 `auth::CredentialStore::load`)。
    pub credentials_path: Option<PathBuf>,
    /// 设备绑定表落盘路径。
    pub bindings_path: Option<PathBuf>,
    /// 单凭据每分钟请求上限。
    pub rate_limit_per_minute: u32,
    /// 单凭据可绑定的设备数上限。
    pub max_devices_per_user: usize,
    /// 允许的签名时间戳偏移(毫秒)。
    pub signature_window_ms: i64,
}

impl PushConfig {
    /// 生产默认值。`credentials_path`/`bindings_path` 由部署方给绝对路径;
    /// 为 `None` 时用内存态(仅测试/联调,重启即丢,也不落盘)。
    pub fn new(
        apns_key_pem: String,
        apns_key_id: String,
        apns_team_id: String,
        apns_topic: String,
    ) -> Self {
        Self {
            port: 6792,
            apns_key_pem,
            apns_key_id,
            apns_team_id,
            apns_topic,
            apns_environment: ApnsEnvironment::Sandbox,
            apns_base_override: None,
            credentials_path: None,
            bindings_path: None,
            rate_limit_per_minute: 60,
            max_devices_per_user: 8,
            signature_window_ms: 60_000,
        }
    }

    pub fn with_environment(mut self, environment: ApnsEnvironment) -> Self {
        self.apns_environment = environment;
        self
    }

    pub fn with_base_override(mut self, base: impl Into<String>) -> Self {
        self.apns_base_override = Some(base.into());
        self
    }

    pub fn with_paths(mut self, credentials: PathBuf, bindings: PathBuf) -> Self {
        self.credentials_path = Some(credentials);
        self.bindings_path = Some(bindings);
        self
    }

    /// APNs 请求基址(含 scheme+host,不含路径)。
    pub fn apns_base(&self) -> String {
        match &self.apns_base_override {
            Some(base) => base.trim_end_matches('/').to_string(),
            None => format!("https://{}", self.apns_environment.host()),
        }
    }

    /// 面向日志的描述:**只**报告「有没有配好」,不回显任何密钥字节。
    pub fn describe(&self) -> String {
        format!(
            "port={} topic={} env={:?} key_id_present={} team_id_present={} key_pem_bytes={} credentials_file={} bindings_file={}",
            self.port,
            self.apns_topic,
            self.apns_environment,
            !self.apns_key_id.is_empty(),
            !self.apns_team_id.is_empty(),
            self.apns_key_pem.len(),
            self.credentials_path.is_some(),
            self.bindings_path.is_some(),
        )
    }

    /// 从环境变量构造。缺 APNs 凭据时返回 `Err`(发送端**不**降级为「假装能推」)。
    pub fn from_env() -> Result<Self, String> {
        let key_pem = std::env::var("APNS_KEY_PEM").or_else(|_| {
            std::env::var("APNS_KEY_P8")
                .map(|path| std::fs::read_to_string(path).unwrap_or_default())
        });
        let key_pem = key_pem.map_err(|_| {
            "APNS_KEY_PEM (PEM text) or APNS_KEY_P8 (path to the .p8 file) is required".to_string()
        })?;
        if key_pem.trim().is_empty() {
            return Err("APNS_KEY_PEM is empty".to_string());
        }
        let key_id =
            std::env::var("APNS_KEY_ID").map_err(|_| "APNS_KEY_ID is required".to_string())?;
        let team_id =
            std::env::var("APNS_TEAM_ID").map_err(|_| "APNS_TEAM_ID is required".to_string())?;
        let topic =
            std::env::var("APNS_TOPIC").map_err(|_| "APNS_TOPIC is required".to_string())?;

        let mut config = PushConfig::new(key_pem, key_id, team_id, topic).with_environment(
            ApnsEnvironment::from_env(std::env::var("APNS_ENVIRONMENT").ok().as_deref())?,
        );
        if let Ok(port) = std::env::var("PUSH_PORT") {
            config.port = port
                .trim()
                .parse::<u16>()
                .map_err(|_| format!("PUSH_PORT must be a port number, got {port:?}"))?;
        }
        if let Ok(base) = std::env::var("APNS_BASE_OVERRIDE") {
            if !base.trim().is_empty() {
                config.apns_base_override = Some(base.trim().to_string());
            }
        }
        if let Ok(path) = std::env::var("PUSH_CREDENTIALS_FILE") {
            if !path.trim().is_empty() {
                config.credentials_path = Some(PathBuf::from(path.trim()));
            }
        }
        if let Ok(path) = std::env::var("PUSH_BINDINGS_FILE") {
            if !path.trim().is_empty() {
                config.bindings_path = Some(PathBuf::from(path.trim()));
            }
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_hosts_match_apple_documentation() {
        assert_eq!(
            ApnsEnvironment::Sandbox.host(),
            "api.sandbox.push.apple.com"
        );
        assert_eq!(ApnsEnvironment::Production.host(), "api.push.apple.com");
        assert_eq!(
            ApnsEnvironment::from_env(None).unwrap(),
            ApnsEnvironment::Sandbox
        );
        assert_eq!(
            ApnsEnvironment::from_env(Some("production")).unwrap(),
            ApnsEnvironment::Production
        );
        assert!(ApnsEnvironment::from_env(Some("staging")).is_err());
    }

    #[test]
    fn describe_never_echoes_the_key_material() {
        let secret = "-----BEGIN PRIVATE KEY-----\nSUPERSECRET\n-----END PRIVATE KEY-----\n";
        let config = PushConfig::new(
            secret.to_string(),
            "KID123".to_string(),
            "TEAM456".to_string(),
            "com.aeroric.ios.companion".to_string(),
        );
        let described = config.describe();
        assert!(!described.contains("SUPERSECRET"), "{described}");
        assert!(!described.contains("KID123"), "{described}");
        assert!(!described.contains("TEAM456"), "{described}");
        assert!(described.contains("key_pem_bytes="), "{described}");
        assert_eq!(config.apns_base(), "https://api.sandbox.push.apple.com");
        assert_eq!(
            config
                .clone()
                .with_environment(ApnsEnvironment::Production)
                .apns_base(),
            "https://api.push.apple.com"
        );
        assert_eq!(
            config
                .clone()
                .with_base_override("http://127.0.0.1:9/")
                .apns_base(),
            "http://127.0.0.1:9"
        );
    }
}
