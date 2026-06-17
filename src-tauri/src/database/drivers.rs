use serde_json::Value;

const DRIVER_MANIFEST_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../dbx/crates/dbx-core/assets/database-drivers.manifest.json"
));

pub(crate) fn driver_manifest_value() -> Result<Value, String> {
    serde_json::from_str(DRIVER_MANIFEST_JSON).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn dbx_driver_manifest() -> Result<Value, String> {
    driver_manifest_value()
}

#[cfg(test)]
mod tests {
    use super::driver_manifest_value;

    #[test]
    fn dbx_driver_manifest_contains_core_drivers() {
        let manifest = driver_manifest_value().unwrap();
        let drivers = manifest["drivers"].as_array().unwrap();
        let driver_types = drivers
            .iter()
            .filter_map(|driver| driver["dbType"].as_str())
            .collect::<Vec<_>>();

        assert!(driver_types.contains(&"sqlite"));
        assert!(driver_types.contains(&"postgres"));
        assert!(driver_types.contains(&"mysql"));
        assert!(driver_types.contains(&"redis"));
        assert!(driver_types.contains(&"mongodb"));
    }
}
