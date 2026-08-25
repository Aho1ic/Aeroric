//! 让 SSH 连接走设置里配置的全局代理。
//!
//! ssh 自己不会说 HTTP CONNECT 或 SOCKS5,标准做法是给它一个 `ProxyCommand`:
//! 一个把 stdin/stdout 接到目标主机的程序。通常用 `nc -X 5 -x ...`,但那条路有
//! 两个硬伤 —— Windows 没有 `nc`,而且 `nc` 没法非交互地传代理密码。
//!
//! 所以这里让 Aeroric 自己充当那个程序:`ProxyCommand "<自身可执行文件>"
//! --ssh-proxy-bridge %h %p`。好处是三个平台同一套代码、支持代理认证、进程生命
//! 周期由 ssh 自己管(它负责拉起和回收),不需要我们维护常驻监听。
//!
//! **host key 校验不受影响**:代理只改传输层,ssh 仍然用真实主机名查 known_hosts,
//! 也仍然校验真实主机的 key。见 [`crate::ssh_hostkey`]。

use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream};
use std::time::Duration;

use base64::Engine as _;

/// 桥模式的命令行标记。`main` 在进 Tauri 之前就要认出它。
pub const SSH_PROXY_BRIDGE_FLAG: &str = "--ssh-proxy-bridge";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Eq)]
enum ProxyKind {
    Http,
    Socks5,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProxyEndpoint {
    kind: ProxyKind,
    host: String,
    port: u16,
    username: String,
    password: String,
}

/// 解析设置里的代理 URL。
///
/// `socks5h` 与 `socks5` 都按"域名交给代理解析"处理 —— 这正是 SSH 经代理时想要的
/// 行为(本地可能根本解析不了目标域名)。URL 里带的凭据优先于设置里单独填的那对,
/// 因为它更具体。
fn parse_proxy_url(url: &str, username: &str, password: &str) -> Option<ProxyEndpoint> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    // 与 app_settings::normalize_proxy_url 一致:没写 scheme 当 http。
    let (scheme, rest) = match trimmed.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("http".to_string(), trimmed),
    };
    let kind = match scheme.as_str() {
        "http" | "https" => ProxyKind::Http,
        "socks5" | "socks5h" | "socks" => ProxyKind::Socks5,
        _ => return None,
    };
    // 去掉路径部分,只留 authority。
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }
    let (credentials, host_port) = match authority.rsplit_once('@') {
        Some((credentials, host_port)) => (Some(credentials), host_port),
        None => (None, authority),
    };
    let (mut user, mut pass) = (username.trim().to_string(), password.trim().to_string());
    if let Some(credentials) = credentials {
        let (url_user, url_pass) = match credentials.split_once(':') {
            Some((user, pass)) => (user, pass),
            None => (credentials, ""),
        };
        if !url_user.is_empty() {
            user = percent_decode(url_user);
            pass = percent_decode(url_pass);
        }
    }
    // IPv6 字面量写成 [::1]:1080。
    let (host, port) = if let Some(rest) = host_port.strip_prefix('[') {
        let (host, tail) = rest.split_once(']')?;
        let port = tail.strip_prefix(':').and_then(|p| p.parse().ok());
        (host.to_string(), port)
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), port.parse().ok()),
            None => (host_port.to_string(), None),
        }
    };
    if host.is_empty() {
        return None;
    }
    let port = port.unwrap_or(match kind {
        ProxyKind::Http => 8080,
        ProxyKind::Socks5 => 1080,
    });
    Some(ProxyEndpoint {
        kind,
        host,
        port,
        username: user,
        password: pass,
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `no_proxy` 命中判断。支持 `*`、`.suffix`、`*.suffix` 与精确主机名。
fn bypasses_proxy(no_proxy: &str, host: &str) -> bool {
    let host = host.trim().trim_matches('.').to_ascii_lowercase();
    no_proxy.split(',').any(|pattern| {
        let pattern = pattern.trim().to_ascii_lowercase();
        if pattern.is_empty() {
            return false;
        }
        if pattern == "*" {
            return true;
        }
        let suffix = pattern
            .strip_prefix("*.")
            .or_else(|| pattern.strip_prefix('.'));
        match suffix {
            Some(suffix) => host == suffix || host.ends_with(&format!(".{suffix}")),
            None => host == pattern,
        }
    })
}

/// 只读出 `proxy_settings` 这一段。
///
/// 不能用 `app_settings::load_settings_internal()`:那条路径在归一化时会**回写**
/// settings.json、还会刷新 agent 脚本。桥是 ssh 每次连接都拉起的短命进程,
/// 绝不能带这种副作用。
fn read_proxy_settings() -> Result<(String, String, String, String), String> {
    let path = crate::storage::aeroric_dir()?.join("settings.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let proxy = parsed.get("proxy_settings");
    let field = |name: &str| {
        proxy
            .and_then(|proxy| proxy.get(name))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string()
    };
    Ok((
        field("url"),
        field("no_proxy"),
        field("username"),
        field("password"),
    ))
}

fn connect_timeout(host: &str, port: u16) -> Result<TcpStream, String> {
    use std::net::ToSocketAddrs;
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {host}:{port}: {e}"))?;
    let mut last_error = format!("cannot resolve {host}:{port}");
    for address in addresses {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(e) => last_error = format!("{address}: {e}"),
        }
    }
    Err(last_error)
}

fn http_connect(
    stream: &mut TcpStream,
    proxy: &ProxyEndpoint,
    host: &str,
    port: u16,
) -> Result<(), String> {
    // 目标是 IPv6 字面量时请求行里必须带方括号。
    let authority = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if !proxy.username.is_empty() {
        let token = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", proxy.username, proxy.password));
        request.push_str(&format!("Proxy-Authorization: Basic {token}\r\n"));
    }
    request.push_str("Proxy-Connection: keep-alive\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("proxy write failed: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;

    // 响应头可能分多次到达,读到空行为止。
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    while !response.windows(4).any(|window| window == b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) => return Err("proxy closed the connection during CONNECT".to_string()),
            Ok(_) => response.push(byte[0]),
            Err(e) => return Err(format!("proxy read failed: {e}")),
        }
        if response.len() > 8192 {
            return Err("proxy sent an oversized CONNECT response".to_string());
        }
    }
    let head = String::from_utf8_lossy(&response);
    let status_line = head.lines().next().unwrap_or_default();
    let code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| format!("proxy returned an unparsable status: {status_line}"))?;
    if !(200..300).contains(&code) {
        if code == 407 {
            return Err(
                "proxy requires authentication (407). Set the proxy username and password in \
Settings > Proxy."
                    .to_string(),
            );
        }
        return Err(format!("proxy refused CONNECT: {status_line}"));
    }
    Ok(())
}

fn socks5_connect(
    stream: &mut TcpStream,
    proxy: &ProxyEndpoint,
    host: &str,
    port: u16,
) -> Result<(), String> {
    let use_auth = !proxy.username.is_empty();
    // 问候:声明支持的认证方式。
    let greeting: Vec<u8> = if use_auth {
        vec![0x05, 0x02, 0x00, 0x02]
    } else {
        vec![0x05, 0x01, 0x00]
    };
    stream
        .write_all(&greeting)
        .map_err(|e| format!("socks write failed: {e}"))?;
    let mut choice = [0u8; 2];
    stream
        .read_exact(&mut choice)
        .map_err(|e| format!("socks handshake failed: {e}"))?;
    if choice[0] != 0x05 {
        return Err("proxy is not a SOCKS5 server".to_string());
    }
    match choice[1] {
        0x00 => {}
        0x02 => {
            if !use_auth {
                return Err(
                    "proxy requires authentication. Set the proxy username and password in \
Settings > Proxy."
                        .to_string(),
                );
            }
            let user = proxy.username.as_bytes();
            let pass = proxy.password.as_bytes();
            if user.len() > 255 || pass.len() > 255 {
                return Err("proxy credentials are too long for SOCKS5".to_string());
            }
            let mut auth = vec![0x01, user.len() as u8];
            auth.extend_from_slice(user);
            auth.push(pass.len() as u8);
            auth.extend_from_slice(pass);
            stream
                .write_all(&auth)
                .map_err(|e| format!("socks auth write failed: {e}"))?;
            let mut result = [0u8; 2];
            stream
                .read_exact(&mut result)
                .map_err(|e| format!("socks auth failed: {e}"))?;
            if result[1] != 0x00 {
                return Err("proxy rejected the credentials".to_string());
            }
        }
        0xFF => return Err("proxy rejected every offered authentication method".to_string()),
        other => {
            return Err(format!(
                "proxy chose an unsupported auth method: {other:#04x}"
            ))
        }
    }

    // CONNECT 请求。域名交给代理解析(ATYP=0x03),这正是经代理连内网主机时想要的。
    let mut request = vec![0x05, 0x01, 0x00];
    match host
        .trim_matches(|c| c == '[' || c == ']')
        .parse::<IpAddr>()
    {
        Ok(IpAddr::V4(address)) => {
            request.push(0x01);
            request.extend_from_slice(&address.octets());
        }
        Ok(IpAddr::V6(address)) => {
            request.push(0x04);
            request.extend_from_slice(&address.octets());
        }
        Err(_) => {
            let bytes = host.as_bytes();
            if bytes.len() > 255 {
                return Err("target hostname is too long for SOCKS5".to_string());
            }
            request.push(0x03);
            request.push(bytes.len() as u8);
            request.extend_from_slice(bytes);
        }
    }
    request.extend_from_slice(&port.to_be_bytes());
    stream
        .write_all(&request)
        .map_err(|e| format!("socks connect write failed: {e}"))?;

    let mut reply = [0u8; 4];
    stream
        .read_exact(&mut reply)
        .map_err(|e| format!("socks connect failed: {e}"))?;
    if reply[1] != 0x00 {
        let reason = match reply[1] {
            0x01 => "general SOCKS server failure",
            0x02 => "connection not allowed by ruleset",
            0x03 => "network unreachable",
            0x04 => "host unreachable",
            0x05 => "connection refused",
            0x06 => "TTL expired",
            0x07 => "command not supported",
            0x08 => "address type not supported",
            _ => "unknown SOCKS error",
        };
        return Err(format!("proxy could not reach {host}:{port}: {reason}"));
    }
    // 把绑定地址读掉,长度随地址类型变化。
    let skip = match reply[3] {
        0x01 => 4 + 2,
        0x04 => 16 + 2,
        0x03 => {
            let mut length = [0u8; 1];
            stream
                .read_exact(&mut length)
                .map_err(|e| format!("socks reply failed: {e}"))?;
            length[0] as usize + 2
        }
        other => {
            return Err(format!(
                "proxy replied with an unknown address type {other:#04x}"
            ))
        }
    };
    let mut discard = vec![0u8; skip];
    stream
        .read_exact(&mut discard)
        .map_err(|e| format!("socks reply failed: {e}"))?;
    Ok(())
}

/// 双向搬字节:stdin → socket、socket → stdout。
///
/// stdout 是 ssh 的传输通道,除了隧道数据不能往里写任何东西 —— 诊断信息全部走 stderr。
fn pump(stream: TcpStream) -> Result<(), String> {
    let mut upstream = stream
        .try_clone()
        .map_err(|e| format!("cannot split the proxy socket: {e}"))?;
    // stdin 侧放到后台线程,主线程负责 socket → stdout。
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buffer = [0u8; 32 * 1024];
        loop {
            match stdin.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if upstream.write_all(&buffer[..count]).is_err() {
                        break;
                    }
                    if upstream.flush().is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        // 本端不再发送,让远端知道 —— 否则对方可能一直等下一个请求。
        let _ = upstream.shutdown(std::net::Shutdown::Write);
    });

    let mut downstream = stream;
    let mut stdout = std::io::stdout().lock();
    let mut buffer = [0u8; 32 * 1024];
    loop {
        match downstream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                stdout
                    .write_all(&buffer[..count])
                    .map_err(|e| format!("stdout write failed: {e}"))?;
                stdout
                    .flush()
                    .map_err(|e| format!("stdout flush failed: {e}"))?;
            }
            Err(e) => return Err(format!("proxy read failed: {e}")),
        }
    }
    Ok(())
}

fn run_bridge(host: &str, port: u16) -> Result<(), String> {
    let (url, no_proxy, username, password) = read_proxy_settings()?;
    let endpoint = parse_proxy_url(&url, &username, &password);

    // 代理没配、或目标在 no_proxy 里 → 直连。宁可连上,也不要因为代理没填就失败。
    let Some(proxy) = endpoint.filter(|_| !bypasses_proxy(&no_proxy, host)) else {
        if url.trim().is_empty() {
            eprintln!("aeroric: no proxy configured in Settings > Proxy, connecting directly");
        }
        let stream = connect_timeout(host, port)?;
        let _ = stream.set_nodelay(true);
        return pump(stream);
    };

    let mut stream = connect_timeout(&proxy.host, proxy.port)
        .map_err(|e| format!("cannot reach the proxy {}:{}: {e}", proxy.host, proxy.port))?;
    let _ = stream.set_nodelay(true);
    match proxy.kind {
        ProxyKind::Http => http_connect(&mut stream, &proxy, host, port)?,
        ProxyKind::Socks5 => socks5_connect(&mut stream, &proxy, host, port)?,
    }
    pump(stream)
}

/// `main` 的前置分支:命令行带 `--ssh-proxy-bridge <host> <port>` 时进入桥模式,
/// 干完就退出,不启动 Tauri。返回 `true` 表示已处理。
pub fn try_run_ssh_proxy_bridge() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some(SSH_PROXY_BRIDGE_FLAG) {
        return false;
    }
    let host = args.get(1).cloned().unwrap_or_default();
    let port = args.get(2).and_then(|value| value.parse::<u16>().ok());
    let result = match (host.is_empty(), port) {
        (false, Some(port)) => run_bridge(&host, port),
        _ => Err(format!("usage: {SSH_PROXY_BRIDGE_FLAG} <host> <port>")),
    };
    if let Err(error) = result {
        eprintln!("aeroric ssh proxy: {error}");
        std::process::exit(1);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_proxy_with_default_port() {
        let proxy = parse_proxy_url("http://127.0.0.1:7890", "", "").expect("parses");

        assert_eq!(proxy.kind, ProxyKind::Http);
        assert_eq!(proxy.host, "127.0.0.1");
        assert_eq!(proxy.port, 7890);
        assert!(proxy.username.is_empty());
    }

    /// 与 `normalize_proxy_url` 保持一致:缺 scheme 当 http。
    fn assert_http(url: &str, host: &str, port: u16) {
        let proxy = parse_proxy_url(url, "", "").expect("parses");
        assert_eq!(proxy.kind, ProxyKind::Http);
        assert_eq!(proxy.host, host);
        assert_eq!(proxy.port, port);
    }

    #[test]
    fn scheme_defaults_and_socks_variants_are_recognised() {
        assert_http("127.0.0.1:7890", "127.0.0.1", 7890);
        assert_http("http://proxy.example.com", "proxy.example.com", 8080);

        for url in ["socks5://127.0.0.1:1080", "socks5h://127.0.0.1:1080"] {
            let proxy = parse_proxy_url(url, "", "").expect("parses");
            assert_eq!(proxy.kind, ProxyKind::Socks5);
            assert_eq!(proxy.port, 1080);
        }
        assert_eq!(
            parse_proxy_url("socks5://127.0.0.1", "", "")
                .expect("parses")
                .port,
            1080
        );
    }

    #[test]
    fn empty_and_unsupported_schemes_are_rejected() {
        assert!(parse_proxy_url("", "", "").is_none());
        assert!(parse_proxy_url("   ", "", "").is_none());
        assert!(parse_proxy_url("ftp://proxy.example.com", "", "").is_none());
        assert!(parse_proxy_url("http://", "", "").is_none());
    }

    /// URL 里的凭据比设置里单独填的那对更具体,必须优先。
    #[test]
    fn url_credentials_win_over_separate_fields() {
        let proxy = parse_proxy_url("http://alice:s%40cret@127.0.0.1:7890", "bob", "other")
            .expect("parses");

        assert_eq!(proxy.username, "alice");
        assert_eq!(proxy.password, "s@cret");
        assert_eq!(proxy.host, "127.0.0.1");
    }

    #[test]
    fn separate_credentials_are_used_when_the_url_has_none() {
        let proxy = parse_proxy_url("http://127.0.0.1:7890", " bob ", " pw ").expect("parses");

        assert_eq!(proxy.username, "bob");
        assert_eq!(proxy.password, "pw");
    }

    #[test]
    fn ipv6_proxy_literals_keep_their_port() {
        let proxy = parse_proxy_url("http://[::1]:7890", "", "").expect("parses");

        assert_eq!(proxy.host, "::1");
        assert_eq!(proxy.port, 7890);
    }

    #[test]
    fn no_proxy_matches_exact_hosts_suffixes_and_wildcards() {
        assert!(bypasses_proxy("example.com", "example.com"));
        assert!(!bypasses_proxy("example.com", "other.com"));
        // 后缀写法的两种形式都要认。
        assert!(bypasses_proxy(".example.com", "api.example.com"));
        assert!(bypasses_proxy("*.example.com", "api.example.com"));
        // 后缀也应覆盖裸域名本身。
        assert!(bypasses_proxy(".example.com", "example.com"));
        // 不能把 notexample.com 误判成 example.com 的子域。
        assert!(!bypasses_proxy(".example.com", "notexample.com"));
        assert!(bypasses_proxy("*", "anything.internal"));
        assert!(bypasses_proxy("a.com, b.com ", "b.com"));
        assert!(!bypasses_proxy("", "example.com"));
    }
}
