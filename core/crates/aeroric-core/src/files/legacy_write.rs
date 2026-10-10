//! Exact legacy desktop write composition. Distinct from the native writer:
//! strict absolute-path guard, hex snapshot before `fs::write`, no size cap,
//! no destination resolution and no atomic rename.

use super::legacy_history::record_snapshot_before_write;
use super::legacy_path::validate_path_within;

/// Guard → lossy resolved text → hex snapshot with the **original** project
/// string → `fs::write`. Every refusal happens before any history entry or
/// file change; the snapshot helper keeps its own root/file validation.
pub fn write_file_content(
    project_path: &str,
    file_path: &str,
    content: &str,
) -> Result<(), String> {
    let validated_path = validate_path_within(file_path, project_path, false)?;
    let validated_path_text = validated_path.to_string_lossy();
    record_snapshot_before_write(project_path, &validated_path_text, content)?;
    std::fs::write(validated_path, content).map_err(|e| e.to_string())
}
