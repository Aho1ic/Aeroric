//! P6-02 Action 1/6：预览（图片/大文件/二进制/未知编码的有因只读判定）。
pub use super::path_policy::MAX_FILE_BYTES;

/// 图片预览上限（与 legacy_read::MAX_IMAGE_PREVIEW_BYTES 同值；策略独立）。
pub const MAX_IMAGE_PREVIEW_BYTES: u64 = 10 * 1024 * 1024;

/// 预览模式（Action 6：大文件/二进制/未知编码显示有原因的只读模式与显式
/// 外部打开选项——不能无限载入并卡主线程）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum PreviewDecision {
    /// 文本预览（UTF-8 可解码且不超限）。
    Text,
    /// 图片预览（扩展名＋大小上限内）。
    Image { media_type: String },
    /// 只读（有原因：超限/二进制/未知编码）。
    ReadOnly { reason: String },
}

/// 预览判定（纯函数；Action 6 的有因只读）。
pub fn preview_decision(file_name: &str, size_bytes: u64, utf8_valid: bool) -> PreviewDecision {
    let image_extensions = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tiff"];
    let extension = std::path::Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if image_extensions.contains(&extension.as_str()) {
        if size_bytes > MAX_IMAGE_PREVIEW_BYTES {
            return PreviewDecision::ReadOnly {
                reason: format!("image exceeds the {MAX_IMAGE_PREVIEW_BYTES} byte preview limit"),
            };
        }
        let media_type = match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "ico" => "image/x-icon",
            _ => "image/tiff",
        };
        return PreviewDecision::Image {
            media_type: media_type.to_string(),
        };
    }
    if size_bytes > MAX_FILE_BYTES {
        return PreviewDecision::ReadOnly {
            reason: format!("file exceeds the {MAX_FILE_BYTES} byte limit"),
        };
    }
    if !utf8_valid {
        return PreviewDecision::ReadOnly {
            reason: "file is not valid UTF-8 (binary or unknown encoding)".to_string(),
        };
    }
    PreviewDecision::Text
}

/// UTF-8 有效性（纯函数；BOM 保留——编辑器层处理）。
pub fn is_utf8(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

/// 恶意 HTML/`javascript:` link/本地文件探测不能执行或泄露（Action 4：原生
/// attributed content，永不走 HTML 渲染；链接 scheme 校验与 Sessions 侧同源）。
pub fn is_allowed_link_scheme(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("javascript:") {
        return false;
    }
    if lower.starts_with("data:") {
        return false;
    }
    matches!(
        lower.split(':').next(),
        Some("http") | Some("https") | Some("mailto")
    )
}
