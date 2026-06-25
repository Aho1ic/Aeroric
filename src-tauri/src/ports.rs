use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PortProjectContext {
    Project,
    Other,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningPort {
    pub port: u16,
    pub address: String,
    pub protocol: String,
    pub pid: u32,
    pub process_name: String,
    pub url: String,
    pub project_context: PortProjectContext,
}

fn validate_project_root(project_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(project_path);
    if !path.is_absolute() {
        return Err("Project path must be absolute".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    if !canonical.is_dir() {
        return Err("Project path is not a directory".to_string());
    }
    Ok(canonical)
}

fn parse_endpoint(endpoint: &str) -> Option<(String, u16)> {
    let trimmed = endpoint.trim();
    let separator = trimmed.rfind(':')?;
    let (address, port) = trimmed.split_at(separator);
    let port = port.strip_prefix(':')?.parse::<u16>().ok()?;
    if port == 0 {
        return None;
    }
    let address = if address.is_empty() { "*" } else { address };
    Some((address.to_string(), port))
}

fn normalize_url_host(address: &str) -> String {
    let address = address.trim();
    match address {
        "" | "*" | "0.0.0.0" | "::" | "[::]" => "localhost".to_string(),
        "::1" | "[::1]" => "localhost".to_string(),
        value if value.starts_with('[') && value.ends_with(']') => value.to_string(),
        value if value.contains(':') => format!("[{value}]"),
        value => value.to_string(),
    }
}

fn make_port_url(address: &str, port: u16) -> String {
    format!("http://{}:{port}", normalize_url_host(address))
}

#[cfg(any(not(windows), test))]
fn classify_process_cwd(root: &Path, cwd: Option<&Path>) -> PortProjectContext {
    let Some(cwd) = cwd else {
        return PortProjectContext::Unknown;
    };
    if !cwd.is_absolute() {
        return PortProjectContext::Unknown;
    }
    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    if canonical_cwd == root || canonical_cwd.starts_with(root) {
        PortProjectContext::Project
    } else {
        PortProjectContext::Other
    }
}

#[cfg(any(not(windows), test))]
fn annotate_project_context<F>(ports: &mut [ListeningPort], root: &Path, mut read_cwd: F)
where
    F: FnMut(u32) -> Option<PathBuf>,
{
    let mut contexts = HashMap::new();
    for pid in ports.iter().map(|port| port.pid).collect::<HashSet<_>>() {
        let cwd = read_cwd(pid);
        contexts.insert(pid, classify_process_cwd(root, cwd.as_deref()));
    }
    for port in ports {
        port.project_context = contexts
            .get(&port.pid)
            .cloned()
            .unwrap_or(PortProjectContext::Unknown);
    }
}

#[cfg(not(windows))]
fn parse_lsof_cwd_output(raw: &str) -> Option<PathBuf> {
    let mut saw_cwd = false;
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        let (field, value) = line.split_at(1);
        match field {
            "f" => saw_cwd = value == "cwd",
            "n" if saw_cwd => return Some(PathBuf::from(value)),
            _ => {}
        }
    }
    None
}

#[cfg(not(windows))]
fn read_process_cwd(pid: u32, root: &Path) -> Option<PathBuf> {
    let proc_cwd = PathBuf::from(format!("/proc/{pid}/cwd"));
    if let Ok(cwd) = std::fs::read_link(proc_cwd) {
        return Some(cwd);
    }

    let pid_arg = pid.to_string();
    let mut command = Command::new("lsof");
    crate::subprocess::configure_background_command(&mut command);
    let output = command
        .args(["-nP", "-a", "-p", &pid_arg, "-d", "cwd", "-Fn"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_lsof_cwd_output(&String::from_utf8_lossy(&output.stdout))
}

fn parse_lsof_listen_output(raw: &str) -> Vec<ListeningPort> {
    let mut ports = Vec::new();
    let mut seen = HashSet::new();
    let mut pid = 0u32;
    let mut process_name = String::new();
    let mut protocol = "tcp".to_string();

    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        let (field, value) = line.split_at(1);
        match field {
            "p" => {
                pid = value.parse().unwrap_or(0);
                process_name.clear();
                protocol = "tcp".to_string();
            }
            "c" => {
                process_name = value.to_string();
            }
            "P" => {
                protocol = value.to_ascii_lowercase();
            }
            "n" => {
                let Some((address, port)) = parse_endpoint(value) else {
                    continue;
                };
                if pid == 0 {
                    continue;
                }
                let dedupe_key = (pid, protocol.clone(), address.clone(), port);
                if !seen.insert(dedupe_key) {
                    continue;
                }
                ports.push(ListeningPort {
                    port,
                    address: normalize_url_host(&address),
                    protocol: protocol.clone(),
                    pid,
                    process_name: if process_name.is_empty() {
                        "unknown".to_string()
                    } else {
                        process_name.clone()
                    },
                    url: make_port_url(&address, port),
                    project_context: PortProjectContext::Unknown,
                });
            }
            _ => {}
        }
    }

    ports.sort_by(|a, b| {
        a.port
            .cmp(&b.port)
            .then_with(|| a.process_name.cmp(&b.process_name))
            .then_with(|| a.pid.cmp(&b.pid))
    });
    ports.truncate(256);
    ports
}

#[cfg(any(windows, test))]
fn parse_tasklist_csv(raw: &str) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '"' if in_quotes && chars.peek() == Some(&'"') => {
                    current.push('"');
                    chars.next();
                }
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => {
                    fields.push(current.trim().to_string());
                    current.clear();
                }
                _ => current.push(ch),
            }
        }
        fields.push(current.trim().to_string());
        if fields.len() < 2 {
            continue;
        }
        if let Ok(pid) = fields[1].parse::<u32>() {
            names.insert(pid, fields[0].clone());
        }
    }
    names
}

#[cfg(any(windows, test))]
fn parse_netstat_listen_output(
    raw: &str,
    process_names: &HashMap<u32, String>,
) -> Vec<ListeningPort> {
    let mut ports = Vec::new();
    let mut seen = HashSet::new();
    for line in raw.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 5 || !parts[0].eq_ignore_ascii_case("tcp") {
            continue;
        }
        let state = parts[3];
        if !state.eq_ignore_ascii_case("listening") {
            continue;
        }
        let Some((address, port)) = parse_endpoint(parts[1]) else {
            continue;
        };
        let Ok(pid) = parts[4].parse::<u32>() else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        let protocol = "tcp".to_string();
        let dedupe_key = (pid, protocol.clone(), address.clone(), port);
        if !seen.insert(dedupe_key) {
            continue;
        }
        ports.push(ListeningPort {
            port,
            address: normalize_url_host(&address),
            protocol,
            pid,
            process_name: process_names
                .get(&pid)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            url: make_port_url(&address, port),
            project_context: PortProjectContext::Unknown,
        });
    }

    ports.sort_by(|a, b| {
        a.port
            .cmp(&b.port)
            .then_with(|| a.process_name.cmp(&b.process_name))
            .then_with(|| a.pid.cmp(&b.pid))
    });
    ports.truncate(256);
    ports
}

fn list_ports_sync(project_path: &str) -> Result<Vec<ListeningPort>, String> {
    let root = validate_project_root(project_path)?;
    #[cfg(windows)]
    {
        let mut netstat = Command::new("netstat");
        crate::subprocess::configure_background_command(&mut netstat);
        let netstat_output = netstat
            .args(["-ano", "-p", "tcp"])
            .current_dir(&root)
            .output()
            .map_err(|e| format!("Failed to run netstat: {e}"))?;
        if !netstat_output.status.success() {
            let stderr = String::from_utf8_lossy(&netstat_output.stderr)
                .trim()
                .to_string();
            return Err(if stderr.is_empty() {
                "Failed to list listening ports".to_string()
            } else {
                stderr
            });
        }

        let mut tasklist = Command::new("tasklist");
        crate::subprocess::configure_background_command(&mut tasklist);
        let process_names = tasklist
            .args(["/FO", "CSV", "/NH"])
            .current_dir(&root)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| parse_tasklist_csv(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or_default();
        return Ok(parse_netstat_listen_output(
            &String::from_utf8_lossy(&netstat_output.stdout),
            &process_names,
        ));
    }

    #[cfg(not(windows))]
    {
        let mut command = Command::new("lsof");
        crate::subprocess::configure_background_command(&mut command);
        let output = command
            .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcPn"])
            .current_dir(&root)
            .output()
            .map_err(|e| format!("Failed to run lsof: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                "Failed to list listening ports".to_string()
            } else {
                stderr
            });
        }
        let mut ports = parse_lsof_listen_output(&String::from_utf8_lossy(&output.stdout));
        annotate_project_context(&mut ports, &root, |pid| read_process_cwd(pid, &root));
        Ok(ports)
    }
}

#[tauri::command]
pub async fn list_listening_ports(project_path: String) -> Result<Vec<ListeningPort>, String> {
    tauri::async_runtime::spawn_blocking(move || list_ports_sync(&project_path))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{
        annotate_project_context, make_port_url, normalize_url_host, parse_endpoint,
        parse_lsof_listen_output, parse_netstat_listen_output, parse_tasklist_csv, ListeningPort,
        PortProjectContext,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn parses_lsof_field_output_and_deduplicates_ports() {
        let raw = "p3623\ncnode\nf31\nPTCP\nn*:5173\nf32\nPTCP\nn*:5173\np10364\ncnode\nf15\nPTCP\nn[::1]:1420\n";

        assert_eq!(
            parse_lsof_listen_output(raw),
            vec![
                ListeningPort {
                    port: 1420,
                    address: "localhost".to_string(),
                    protocol: "tcp".to_string(),
                    pid: 10364,
                    process_name: "node".to_string(),
                    url: "http://localhost:1420".to_string(),
                    project_context: PortProjectContext::Unknown,
                },
                ListeningPort {
                    port: 5173,
                    address: "localhost".to_string(),
                    protocol: "tcp".to_string(),
                    pid: 3623,
                    process_name: "node".to_string(),
                    url: "http://localhost:5173".to_string(),
                    project_context: PortProjectContext::Unknown,
                },
            ]
        );
    }

    #[test]
    fn parses_endpoint_host_and_port() {
        assert_eq!(
            parse_endpoint("127.0.0.1:3000"),
            Some(("127.0.0.1".to_string(), 3000))
        );
        assert_eq!(
            parse_endpoint("[::1]:1420"),
            Some(("[::1]".to_string(), 1420))
        );
        assert_eq!(parse_endpoint("*:5173"), Some(("*".to_string(), 5173)));
        assert_eq!(parse_endpoint("missing-port"), None);
    }

    #[test]
    fn parses_windows_netstat_output_with_task_names() {
        let process_names = parse_tasklist_csv(
            "\"node.exe\",\"1234\",\"Console\",\"1\",\"100,000 K\"\n\"python.exe\",\"4321\",\"Console\",\"1\",\"20,000 K\"\n",
        );
        let raw = "\
Proto  Local Address          Foreign Address        State           PID
TCP    0.0.0.0:5173           0.0.0.0:0              LISTENING       1234
TCP    [::1]:8000             [::]:0                 LISTENING       4321
TCP    127.0.0.1:9000         127.0.0.1:52300        ESTABLISHED     7777
";

        assert_eq!(
            parse_netstat_listen_output(raw, &process_names),
            vec![
                ListeningPort {
                    port: 5173,
                    address: "localhost".to_string(),
                    protocol: "tcp".to_string(),
                    pid: 1234,
                    process_name: "node.exe".to_string(),
                    url: "http://localhost:5173".to_string(),
                    project_context: PortProjectContext::Unknown,
                },
                ListeningPort {
                    port: 8000,
                    address: "localhost".to_string(),
                    protocol: "tcp".to_string(),
                    pid: 4321,
                    process_name: "python.exe".to_string(),
                    url: "http://localhost:8000".to_string(),
                    project_context: PortProjectContext::Unknown,
                },
            ]
        );
    }

    #[test]
    fn annotates_ports_with_project_context_from_process_cwds() {
        let mut ports = vec![
            ListeningPort {
                port: 3000,
                address: "localhost".to_string(),
                protocol: "tcp".to_string(),
                pid: 1,
                process_name: "node".to_string(),
                url: "http://localhost:3000".to_string(),
                project_context: PortProjectContext::Unknown,
            },
            ListeningPort {
                port: 5432,
                address: "localhost".to_string(),
                protocol: "tcp".to_string(),
                pid: 2,
                process_name: "postgres".to_string(),
                url: "http://localhost:5432".to_string(),
                project_context: PortProjectContext::Unknown,
            },
            ListeningPort {
                port: 9999,
                address: "localhost".to_string(),
                protocol: "tcp".to_string(),
                pid: 3,
                process_name: "service".to_string(),
                url: "http://localhost:9999".to_string(),
                project_context: PortProjectContext::Unknown,
            },
        ];

        annotate_project_context(&mut ports, Path::new("/repo"), |pid| match pid {
            1 => Some(PathBuf::from("/repo/app")),
            2 => Some(PathBuf::from("/opt/database")),
            _ => None,
        });

        assert_eq!(ports[0].project_context, PortProjectContext::Project);
        assert_eq!(ports[1].project_context, PortProjectContext::Other);
        assert_eq!(ports[2].project_context, PortProjectContext::Unknown);
    }

    #[cfg(not(windows))]
    #[test]
    fn parses_lsof_cwd_output() {
        let raw = "p123\nfcwd\nn/Users/example/project\n";

        assert_eq!(
            super::parse_lsof_cwd_output(raw),
            Some(PathBuf::from("/Users/example/project"))
        );
    }

    #[test]
    fn normalizes_url_hosts_for_local_preview() {
        assert_eq!(normalize_url_host("*"), "localhost");
        assert_eq!(normalize_url_host("[::1]"), "localhost");
        assert_eq!(normalize_url_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(make_port_url("*", 5173), "http://localhost:5173");
    }
}
