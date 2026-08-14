use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

fn is_dsh_plugin_package(package: &str) -> bool {
    package.starts_with("@deepseek-ai/dsh-") || package.starts_with("dsh-")
}

fn plugin_id(package: &str) -> &str {
    package
        .strip_prefix("@deepseek-ai/dsh-")
        .or_else(|| package.strip_prefix("dsh-"))
        .unwrap_or(package)
}

fn plugin_patch_rows(content: &str) -> Result<Vec<serde_yaml_ng::Value>, String> {
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(content)
        .map_err(|error| format!("Failed to parse Aeroric DSH plugin patch: {error}"))?
    {
        serde_yaml_ng::Value::Null => Ok(Vec::new()),
        serde_yaml_ng::Value::Sequence(rows) => Ok(rows),
        _ => Err("Aeroric DSH plugin patch must be a YAML sequence".to_string()),
    }
}

fn row_id(row: &serde_yaml_ng::Value) -> Option<&str> {
    row.as_mapping()?
        .get(serde_yaml_ng::Value::String("id".to_string()))?
        .as_str()
}

fn plugin_enabled_from_patch(content: &str, package: &str) -> Result<bool, String> {
    let id = plugin_id(package);
    let Some(row) = plugin_patch_rows(content)?
        .into_iter()
        .find(|row| row_id(row) == Some(id))
    else {
        return Ok(true);
    };
    Ok(!row
        .as_mapping()
        .and_then(|mapping| mapping.get(serde_yaml_ng::Value::String("disabled".to_string())))
        .and_then(serde_yaml_ng::Value::as_bool)
        .unwrap_or(false))
}

fn update_plugin_patch(content: &str, package: &str, enabled: bool) -> Result<String, String> {
    let id = plugin_id(package);
    let mut rows = plugin_patch_rows(content)?;
    let disabled_key = serde_yaml_ng::Value::String("disabled".to_string());
    if let Some(row) = rows.iter_mut().find(|row| row_id(row) == Some(id)) {
        let mapping = row
            .as_mapping_mut()
            .ok_or_else(|| "DSH plugin patch row must be a mapping".to_string())?;
        mapping.insert(disabled_key, serde_yaml_ng::Value::Bool(!enabled));
    } else {
        let mut mapping = serde_yaml_ng::Mapping::new();
        mapping.insert(
            serde_yaml_ng::Value::String("id".to_string()),
            serde_yaml_ng::Value::String(id.to_string()),
        );
        mapping.insert(disabled_key, serde_yaml_ng::Value::Bool(!enabled));
        rows.push(serde_yaml_ng::Value::Mapping(mapping));
    }
    serde_yaml_ng::to_string(&serde_yaml_ng::Value::Sequence(rows))
        .map_err(|error| format!("Failed to serialize Aeroric DSH plugin patch: {error}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DshPlugin {
    pub name: String,
    pub version: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[tauri::command]
pub async fn list_dsh_plugins(agent: String) -> Result<Vec<DshPlugin>, String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let profile_dir = home.join("profiles").join("default");
    let package_json_path = profile_dir.join("package.json");

    if !package_json_path.exists() {
        return Ok(Vec::new());
    }

    let content = tokio::fs::read_to_string(&package_json_path)
        .await
        .map_err(|e| format!("Failed to read package.json: {}", e))?;

    let package_json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse package.json: {}", e))?;

    let mut plugins = Vec::new();

    if let Some(deps) = package_json.get("dependencies").and_then(|v| v.as_object()) {
        for (name, version) in deps {
            if is_dsh_plugin_package(name) {
                let enabled = is_plugin_enabled(&home, name).await?;
                plugins.push(DshPlugin {
                    name: name.clone(),
                    version: version.as_str().unwrap_or("unknown").to_string(),
                    enabled,
                    description: None,
                });
            }
        }
    }

    Ok(plugins)
}

#[tauri::command]
pub async fn install_dsh_plugin(
    agent: String,
    package: String,
    version: Option<String>,
) -> Result<DshPlugin, String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let profile_dir = home.join("profiles").join("default");

    if !profile_dir.exists() {
        tokio::fs::create_dir_all(&profile_dir)
            .await
            .map_err(|e| format!("Failed to create profile directory: {}", e))?;
    }

    let package_spec = if let Some(ver) = version {
        format!("{}@{}", package, ver)
    } else {
        package.clone()
    };

    let output = Command::new("npm")
        .arg("install")
        .arg("--prefix")
        .arg(&profile_dir)
        .arg(&package_spec)
        .output()
        .await
        .map_err(|e| format!("Failed to execute npm install: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("npm install failed: {}", stderr));
    }

    let installed_version = get_installed_version(&profile_dir, &package).await?;

    Ok(DshPlugin {
        name: package,
        version: installed_version,
        enabled: true,
        description: None,
    })
}

#[tauri::command]
pub async fn uninstall_dsh_plugin(agent: String, package: String) -> Result<(), String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let profile_dir = home.join("profiles").join("default");

    let output = Command::new("npm")
        .arg("uninstall")
        .arg("--prefix")
        .arg(&profile_dir)
        .arg(&package)
        .output()
        .await
        .map_err(|e| format!("Failed to execute npm uninstall: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("npm uninstall failed: {}", stderr));
    }

    Ok(())
}

#[tauri::command]
pub async fn toggle_dsh_plugin(
    agent: String,
    package: String,
    enabled: bool,
) -> Result<(), String> {
    let home = crate::dsh_home::ensure_dsh_home_for(&agent)?;
    let patch_path = crate::dsh_home::plugins_patch_path_in(&home);
    tokio::task::spawn_blocking(move || {
        let content = std::fs::read_to_string(&patch_path).unwrap_or_default();
        let updated = update_plugin_patch(&content, &package, enabled)?;
        crate::storage::atomic_write(&patch_path, &updated)
            .map_err(|error| format!("Failed to write DSH plugin patch: {error}"))
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn is_plugin_enabled(home: &Path, package: &str) -> Result<bool, String> {
    let patch_path = crate::dsh_home::plugins_patch_path_in(home);

    if !patch_path.exists() {
        return Ok(true);
    }

    let content = tokio::fs::read_to_string(&patch_path)
        .await
        .map_err(|e| format!("Failed to read DSH plugin patch: {e}"))?;
    plugin_enabled_from_patch(&content, package)
}

async fn get_installed_version(profile_dir: &Path, package: &str) -> Result<String, String> {
    let package_json_path = profile_dir.join("package.json");

    if !package_json_path.exists() {
        return Ok("unknown".to_string());
    }

    let content = tokio::fs::read_to_string(&package_json_path)
        .await
        .map_err(|e| format!("Failed to read package.json: {}", e))?;

    let package_json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse package.json: {}", e))?;

    if let Some(version) = package_json
        .get("dependencies")
        .and_then(|v| v.as_object())
        .and_then(|deps| deps.get(package))
        .and_then(|v| v.as_str())
    {
        Ok(version.to_string())
    } else {
        Ok("unknown".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_cordis_row_id_from_supported_package_names() {
        assert_eq!(plugin_id("@deepseek-ai/dsh-tool-web"), "tool-web");
        assert_eq!(plugin_id("dsh-tool-web"), "tool-web");
    }

    #[test]
    fn plugin_patch_updates_only_the_exact_yaml_row() {
        let input = "- id: tool-web-extra\n  disabled: true\n  config:\n    keep: yes\n- id: tool-web\n  disabled: true\n";
        let updated = update_plugin_patch(input, "@deepseek-ai/dsh-tool-web", true).unwrap();

        assert!(plugin_enabled_from_patch(&updated, "@deepseek-ai/dsh-tool-web").unwrap());
        assert!(!plugin_enabled_from_patch(&updated, "dsh-tool-web-extra").unwrap());
        assert!(updated.contains("keep: yes"));
    }

    #[test]
    fn plugin_patch_rejects_non_sequence_yaml_without_overwriting_it() {
        let error = update_plugin_patch("llm-deepseek: {}\n", "dsh-tool-web", false)
            .expect_err("mapping-shaped settings are not a plugin patch");
        assert!(error.contains("must be a YAML sequence"));
    }
}
