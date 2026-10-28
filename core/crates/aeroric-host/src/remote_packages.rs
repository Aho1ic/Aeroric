//! Offline remote installs (O1–O6, 2026-10-28): when the remote machine can't
//! download, this Mac fetches the agent's official standalone build
//! (following the macOS system proxy), checks its sha256 and caches it; the
//! install job then streams it to the remote machine over the connection's
//! own ssh, where it is checked again and put in place.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use aeroric_core::context::CoreContext;
use serde_json::Value;
use sha2::Digest;

const CLAUDE_RELEASES: &str = "https://downloads.claude.ai/claude-code-releases";
const CODEX_RELEASE: &str = "https://api.github.com/repos/openai/codex/releases/latest";
const OMP_BUILDS: &str = "https://build.stencil.so/api/products/omp/latest";

/// One agent's standalone build for one remote platform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub agent: String,
    pub version: String,
    /// The vendor's platform name (`remote::package_platform`).
    pub platform: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// A `.tar.gz` holding the executable (Codex) rather than the executable.
    pub archive: bool,
}

/// The HTTP(S) proxy in `scutil --proxy` output, as a URL.
pub fn parse_scutil_proxy(text: &str) -> Option<String> {
    let field = |key: &str| {
        text.lines().find_map(|line| {
            let (name, value) = line.split_once(" : ")?;
            (name.trim() == key).then(|| value.trim().to_string())
        })
    };
    for (scheme, prefix) in [("https", "HTTPS"), ("http", "HTTP"), ("socks5h", "SOCKS")] {
        if field(&format!("{prefix}Enable")).as_deref() == Some("1") {
            let host = field(&format!("{prefix}Proxy")).filter(|h| !h.is_empty())?;
            let port = field(&format!("{prefix}Port")).unwrap_or_else(|| "80".into());
            let scheme = if scheme == "https" { "http" } else { scheme };
            return Some(format!("{scheme}://{host}:{port}"));
        }
    }
    None
}

/// The proxy macOS is set to use (O5); `HTTPS_PROXY` and friends from the
/// environment are honoured by reqwest itself when this finds none.
fn system_proxy() -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    parse_scutil_proxy(&String::from_utf8_lossy(&output.stdout))
}

/// A client for downloads: no overall deadline (builds are hundreds of MB).
fn client() -> Result<reqwest::blocking::Client, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent("Aeroric")
        .connect_timeout(Duration::from_secs(15))
        .timeout(None::<Duration>);
    if let Some(proxy) = system_proxy() {
        builder = builder
            .proxy(reqwest::Proxy::all(&proxy).map_err(|e| format!("系统代理 {proxy} 无效：{e}"))?);
    }
    builder
        .build()
        .map_err(|e| format!("无法创建下载客户端：{e}"))
}

fn get(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<reqwest::blocking::Response, String> {
    let host = url.split('/').nth(2).unwrap_or(url).to_string();
    client
        .get(url)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|e| format!("本机连不上 {host}：{e}（检查本机网络或系统代理）"))
}

fn get_json(client: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    get(client, url)?
        .json::<Value>()
        .map_err(|e| format!("{url} 的回答无法解析：{e}"))
}

/// The latest standalone build of `agent` for `platform`.
pub fn resolve(agent: &str, platform: &str) -> Result<Package, String> {
    let client = client()?;
    let unexpected = |what: &str| format!("{what} 的回答缺少 {platform} 的安装包");
    let package = match agent {
        "claude" => {
            let version = get(&client, &format!("{CLAUDE_RELEASES}/latest"))?
                .text()
                .map_err(|e| e.to_string())?
                .trim()
                .to_string();
            if !version.starts_with(|c: char| c.is_ascii_digit()) {
                return Err("downloads.claude.ai 没有给出版本号".into());
            }
            let manifest = get_json(
                &client,
                &format!("{CLAUDE_RELEASES}/{version}/manifest.json"),
            )?;
            let entry = &manifest["platforms"][platform];
            Package {
                url: format!("{CLAUDE_RELEASES}/{version}/{platform}/claude"),
                sha256: entry["checksum"]
                    .as_str()
                    .ok_or_else(|| unexpected("Claude 清单"))?
                    .into(),
                size: entry["size"].as_u64().unwrap_or(0),
                archive: false,
                version,
                agent: agent.into(),
                platform: platform.into(),
            }
        }
        "codex" => {
            let release = get_json(&client, CODEX_RELEASE)?;
            let name = format!("codex-{platform}.tar.gz");
            let asset = release["assets"]
                .as_array()
                .and_then(|assets| assets.iter().find(|a| a["name"] == name.as_str()))
                .ok_or_else(|| unexpected("Codex 发布页"))?;
            Package {
                version: release["tag_name"]
                    .as_str()
                    .unwrap_or_default()
                    .trim_start_matches("rust-v")
                    .to_string(),
                url: asset["browser_download_url"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                sha256: asset["digest"]
                    .as_str()
                    .and_then(|d| d.strip_prefix("sha256:"))
                    .ok_or_else(|| unexpected("Codex 发布页的校验和"))?
                    .into(),
                size: asset["size"].as_u64().unwrap_or(0),
                archive: true,
                agent: agent.into(),
                platform: platform.into(),
            }
        }
        "omp" => {
            let build = get_json(&client, &format!("{OMP_BUILDS}/{platform}?channel=stable"))?;
            Package {
                version: build["build"]["version"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                url: build["download"]
                    .as_str()
                    .ok_or_else(|| unexpected("omp 构建服务"))?
                    .into(),
                sha256: build["file"]["sha256"]
                    .as_str()
                    .ok_or_else(|| unexpected("omp 构建服务"))?
                    .into(),
                size: build["file"]["size"].as_u64().unwrap_or(0),
                archive: false,
                agent: agent.into(),
                platform: platform.into(),
            }
        }
        _ => return Err(format!("{agent} 没有独立安装包")),
    };
    let sane = !package.version.is_empty()
        && package.url.starts_with("https://")
        && package.sha256.len() == 64
        && package.sha256.bytes().all(|b| b.is_ascii_hexdigit());
    if sane {
        Ok(package)
    } else {
        Err(format!("{agent} 的下载信息不完整"))
    }
}

/// Where downloaded builds are kept (O4): `<agent>/<platform>/<version>/`.
pub fn cache_root(context: &CoreContext) -> PathBuf {
    context.paths.data_dir().join("remote-packages")
}

fn package_file(root: &Path, package: &Package) -> PathBuf {
    root.join(&package.agent)
        .join(&package.platform)
        .join(&package.version)
        .join(if package.archive {
            "package.tar.gz"
        } else {
            "binary"
        })
}

/// The cached, verified file of `package`, if there is one.
pub fn cached(root: &Path, package: &Package) -> Option<PathBuf> {
    let file = package_file(root, package);
    let mark = std::fs::read_to_string(file.with_extension("sha256")).ok()?;
    (mark.trim() == package.sha256 && file.is_file()).then_some(file)
}

/// `package`'s file, downloaded and checked unless already cached; older
/// versions for the same agent and platform are removed (O4).
pub fn fetch(
    root: &Path,
    package: &Package,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let file = package_file(root, package);
    if cached(root, package).is_none() {
        let dir = file.parent().ok_or("缓存路径无效")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("无法创建缓存目录：{e}"))?;
        let part = file.with_extension("part");
        let mut response = get(&client()?, &package.url)?;
        let total = response.content_length().unwrap_or(package.size);
        let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut hasher = sha2::Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        loop {
            let read = response
                .read(&mut buffer)
                .map_err(|e| format!("下载中断：{e}"))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            out.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
            done += read as u64;
            progress(done, total);
        }
        drop(out);
        let sum: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if sum != package.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(format!("本机下载的安装包 sha256 不一致（{sum}），已丢弃"));
        }
        std::fs::rename(&part, &file).map_err(|e| e.to_string())?;
        std::fs::write(file.with_extension("sha256"), &package.sha256)
            .map_err(|e| e.to_string())?;
    }
    if let Some(versions) = file.parent().and_then(Path::parent) {
        for entry in std::fs::read_dir(versions).into_iter().flatten().flatten() {
            if entry.file_name() != package.version.as_str() {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    Ok(file)
}

/// Bytes the cache holds.
pub fn cache_size(root: &Path) -> u64 {
    fn walk(path: &Path) -> u64 {
        std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| match entry.metadata() {
                Ok(meta) if meta.is_dir() => walk(&entry.path()),
                Ok(meta) => meta.len(),
                Err(_) => 0,
            })
            .sum()
    }
    walk(root)
}

pub fn clear_cache(root: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(root) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_proxy_prefers_https_then_http_then_socks() {
        let both = "<dictionary> {\n  HTTPEnable : 1\n  HTTPPort : 8080\n  HTTPProxy : 10.0.0.1\n  HTTPSEnable : 1\n  HTTPSPort : 7890\n  HTTPSProxy : 127.0.0.1\n}";
        assert_eq!(
            parse_scutil_proxy(both).as_deref(),
            Some("http://127.0.0.1:7890")
        );
        let socks =
            "  HTTPSEnable : 0\n  SOCKSEnable : 1\n  SOCKSPort : 1080\n  SOCKSProxy : 127.0.0.1\n";
        assert_eq!(
            parse_scutil_proxy(socks).as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
        assert_eq!(
            parse_scutil_proxy("<dictionary> {\n  HTTPEnable : 0\n}"),
            None
        );
    }

    #[test]
    fn cached_builds_are_reused_only_when_their_sum_matches_and_old_versions_go() {
        let root = std::env::temp_dir().join(format!("aeroric-packages-{}", uuid::Uuid::new_v4()));
        let mut package = Package {
            agent: "omp".into(),
            version: "1.0.0".into(),
            platform: "macos-arm64".into(),
            url: "https://unused.invalid/omp".into(),
            sha256: "a".repeat(64),
            size: 3,
            archive: false,
        };
        let old = package_file(&root, &package);
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, b"old").unwrap();
        std::fs::write(old.with_extension("sha256"), "a".repeat(64)).unwrap();
        assert_eq!(cached(&root, &package), Some(old.clone()));

        package.version = "2.0.0".into();
        let new = package_file(&root, &package);
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::write(&new, b"new").unwrap();
        std::fs::write(new.with_extension("sha256"), "a".repeat(64)).unwrap();
        assert_eq!(fetch(&root, &package, |_, _| {}).unwrap(), new);
        assert!(!old.exists(), "the older version is pruned");
        assert_eq!(cache_size(&root), 3 + 64);

        package.sha256 = "b".repeat(64);
        assert_eq!(
            cached(&root, &package),
            None,
            "a different sum is not reused"
        );
        clear_cache(&root).unwrap();
        assert_eq!(cache_size(&root), 0);
        clear_cache(&root).unwrap();
    }
}
