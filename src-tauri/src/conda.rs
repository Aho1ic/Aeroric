use crate::ssh::SshConnection;
pub(crate) use aeroric_core::infrastructure::conda::CondaEnvironment;
use aeroric_core::infrastructure::conda_face::{parse_conda_envs_with_python_exists, CondaProbe};
use tauri::Manager;

fn build_remote_conda_env_list_command() -> String {
    let candidates = [
        "${CONDA_EXE:-}",
        "/opt/miniconda3/bin/conda",
        "/opt/homebrew/bin/conda",
        "/usr/local/bin/conda",
        "$HOME/miniconda3/bin/conda",
        "$HOME/anaconda3/bin/conda",
        "$HOME/mambaforge/bin/conda",
        "conda",
    ];
    let script = format!(
        "for c in {}; do [ -n \"$c\" ] || continue; if command -v \"$c\" >/dev/null 2>&1 || [ -x \"$c\" ]; then raw=$(\"$c\" env list --json) || continue; if command -v python3 >/dev/null 2>&1; then python3 -c 'import json,os,sys; data=json.load(sys.stdin); data[\"envs\"]=[p for p in data.get(\"envs\", []) if isinstance(p, str) and os.path.exists(p.rstrip(\"/\") + \"/bin/python\")]; print(json.dumps(data))' <<EOF\n$raw\nEOF\nelse printf '%s\\n' \"$raw\"; fi; exit 0; fi; done; exit 1",
        candidates.join(" ")
    );
    format!("sh -lc {}", crate::ssh::shell_quote_posix(&script))
}

fn run_remote_conda_env_list(connection: &SshConnection) -> Result<Vec<u8>, String> {
    let mut command = crate::ssh::std_ssh_command_for_remote_command(
        connection,
        build_remote_conda_env_list_command(),
    );
    crate::subprocess::configure_background_command(&mut command);
    let output = command.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[tauri::command]
pub async fn detect_conda_environments<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Vec<CondaEnvironment>, String> {
    let value = crate::host_dispatch::dispatch(
        &app,
        "infra.detect_conda_environments",
        serde_json::json!({}),
        async {
            // Do not resolve local state or start a worker on External, even
            // when the host is unavailable or returns an invalid reply.
            let probe = app
                .try_state::<CondaProbe>()
                .map(|probe| probe.inner().clone())
                .unwrap_or_default();
            let environments = tauri::async_runtime::spawn_blocking(move || probe.detect())
                .await
                .map_err(|error| error.to_string())?;
            serde_json::to_value(environments).map_err(|error| error.to_string())
        },
    )
    .await?;
    serde_json::from_value(value).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn detect_remote_conda_environments(
    connection: SshConnection,
) -> Result<Vec<CondaEnvironment>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let raw = run_remote_conda_env_list(&connection)?;
        Ok(parse_conda_envs_with_python_exists(&raw, |_| true))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::build_remote_conda_env_list_command;

    #[test]
    fn remote_conda_command_checks_common_install_locations() {
        let command = build_remote_conda_env_list_command();

        assert!(command.starts_with("sh -lc "));
        assert!(command.contains("conda"));
        assert!(command.contains("env list --json"));
        assert!(command.contains("$HOME/miniconda3/bin/conda"));
    }
}

#[cfg(test)]
#[cfg(unix)]
mod command_tests;
