//! `aeroric-remote-push` —— 自托管 APNs 发送端二进制入口。
//!
//! 子命令:
//! * `serve`   —— 起 HTTP 服务(默认);启动前**必须**能解析 APNs 私钥,否则拒绝启动。
//! * `enroll`  —— 本地铸造一份 per-user 凭据并打印(id/secret)。只在服务器本机跑,
//!   避免把「谁能注册推送凭据」变成网络入口。
//! * `check`   —— 自检:解析配置、验证私钥、报告绑定/凭据数量,不起监听。
//!
//! 所有失败都走「打印一行原因 + exit 1」:这是部署方要读的一行诊断,不是 backtrace。

use std::sync::Arc;

use aeroric_remote_push::apns::{ApnsClient, ProviderKey};
use aeroric_remote_push::auth::{now_ms, validate_key_pem, CredentialStore};
use aeroric_remote_push::bindings::BindingStore;
use aeroric_remote_push::config::PushConfig;
use aeroric_remote_push::server::{self, AppState};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("serve");
    let exit = match command {
        "serve" => serve(),
        "enroll" => enroll(args.get(1).map(String::as_str)),
        "check" => check(),
        "-h" | "--help" | "help" => {
            println!(
                "aeroric-remote-push [serve|enroll <user>|check]\n\
                 \n\
                 Environment:\n\
                 \x20 APNS_KEY_PEM         PKCS#8 PEM text of the APNs .p8 auth key\n\
                 \x20 APNS_KEY_P8          path to the .p8 file (alternative to APNS_KEY_PEM)\n\
                 \x20 APNS_KEY_ID          Apple Key ID (JWT kid)\n\
                 \x20 APNS_TEAM_ID         Apple Team ID (JWT iss)\n\
                 \x20 APNS_TOPIC           app bundle id, e.g. com.aeroric.ios.companion\n\
                 \x20 APNS_ENVIRONMENT      sandbox (default) | production\n\
                 \x20 APNS_BASE_OVERRIDE    testing only: fake APNs base URL\n\
                 \x20 PUSH_PORT             listen port, default 6792\n\
                 \x20 PUSH_CREDENTIALS_FILE credentials.json path (default: in-memory)\n\
                 \x20 PUSH_BINDINGS_FILE    bindings.json path (default: in-memory)\n\
                 \x20 PUSH_ENROLL_SECRET    if set, POST /v1/enroll is open with this secret"
            );
            0
        }
        other => {
            eprintln!("[push] unknown command {other:?}; try --help");
            1
        }
    };
    std::process::exit(exit);
}

/// 读取配置,并**强制**私钥可解析。返回 `Err` 时调用方打印并退出 1。
fn load_config() -> Result<PushConfig, String> {
    let config = PushConfig::from_env()?;
    validate_key_pem(&config.apns_key_pem)
        .map_err(|error| format!("APNs key rejected: {error}"))?;
    Ok(config)
}

fn build_state(config: PushConfig) -> Result<Arc<AppState>, String> {
    let credentials = CredentialStore::load(config.credentials_path.clone())?;
    let bindings = BindingStore::load(config.bindings_path.clone())?;
    let key = ProviderKey::from_pem(&config.apns_key_pem)
        .map_err(|e| format!("APNs key rejected: {e}"))?
        .with_ids(config.apns_key_id.clone(), config.apns_team_id.clone());
    let apns = ApnsClient::new(&config.apns_base(), &config.apns_topic, Arc::new(key))?;
    let enroll_secret = std::env::var("PUSH_ENROLL_SECRET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Ok(Arc::new(AppState::new(
        config,
        credentials,
        bindings,
        Some(apns),
        enroll_secret,
    )))
}

fn serve() -> i32 {
    let config = match load_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[push] {error}");
            return 1;
        }
    };
    let port = config.port;
    // `describe` 只报告「配好了没」,不回显密钥字节。
    eprintln!("[push] {}", config.describe());
    let state = match build_state(config) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("[push] {error}");
            return 1;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("[push] cannot start tokio runtime: {error}");
            return 1;
        }
    };

    runtime.block_on(async move {
        let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("[push] failed to bind 0.0.0.0:{port}: {error}");
                return 1;
            }
        };
        eprintln!(
            "[push] listening on 0.0.0.0:{port} (per-user signatures required; APNs key stays here)"
        );
        match server::serve(state, listener).await {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("[push] server stopped: {error}");
                1
            }
        }
    })
}

fn enroll(user: Option<&str>) -> i32 {
    let config = match PushConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[push] {error}");
            return 1;
        }
    };
    let mut store = match CredentialStore::load(config.credentials_path.clone()) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("[push] {error}");
            return 1;
        }
    };
    match store.enroll(user) {
        Ok((id, secret)) => {
            // 明文只在这里出现一次:部署方要把它粘进 Mac 客户端的钥匙串。
            println!("user: {id}\nsecret: {secret}");
            println!(
                "store this secret on the Aeroric Mac host; it is not recoverable from {}",
                config
                    .credentials_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "the in-memory store".to_string())
            );
            0
        }
        Err(error) => {
            eprintln!("[push] {error}");
            1
        }
    }
}

fn check() -> i32 {
    let config = match load_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("[push] {error}");
            return 1;
        }
    };
    let credentials = match CredentialStore::load(config.credentials_path.clone()) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("[push] {error}");
            return 1;
        }
    };
    let bindings = match BindingStore::load(config.bindings_path.clone()) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("[push] {error}");
            return 1;
        }
    };
    println!("{}", config.describe());
    println!(
        "users={} bindings={} apns_base={} now_ms={}",
        credentials.user_count(),
        bindings.len(),
        config.apns_base(),
        now_ms()
    );
    0
}
