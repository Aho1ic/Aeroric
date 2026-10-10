//! Existing legacy text/image IO, not native read_write or preview_decision.
//! Callers supply an already validated path; root/policy and scheduling stay
//! at the adapter boundary. Metadata limits remain point-in-time observations.

use base64::Engine;
use std::path::Path;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePreviewData {
    pub data_url: String,
    pub mime_type: String,
    pub byte_length: u64,
}

pub const MAX_IMAGE_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

fn previewable_image_mime_type(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

/// Read the validated path with the old descriptor-metadata/UTF-8 contract.
pub fn read_file_content(validated_path: &Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(validated_path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.len() > 2 * 1024 * 1024 {
        return Err(format!(
            "File too large ({:.1} MB)",
            meta.len() as f64 / 1024.0 / 1024.0
        ));
    }
    let mut buf = String::with_capacity(meta.len() as usize);
    std::io::BufReader::new(file)
        .read_to_string(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf)
}

/// Read the validated path using legacy extension-only MIME and data-URL wire.
/// The caller decides the symlink policy; this function is not root authority.
pub fn read_image_preview(validated_path: &Path) -> Result<ImagePreviewData, String> {
    use std::io::Read;

    let mime_type = previewable_image_mime_type(validated_path)
        .ok_or_else(|| "Unsupported image format".to_string())?;

    let file = std::fs::File::open(validated_path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if meta.len() > MAX_IMAGE_PREVIEW_BYTES {
        return Err(format!(
            "Image too large ({:.1} MB)",
            meta.len() as f64 / 1024.0 / 1024.0
        ));
    }

    let mut bytes = Vec::with_capacity(meta.len() as usize);
    std::io::BufReader::new(file)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;

    Ok(ImagePreviewData {
        data_url: format!(
            "data:{};base64,{}",
            mime_type,
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
        mime_type: mime_type.to_string(),
        byte_length: meta.len(),
    })
}
