//! prompt 投递的附件与模式判定（P3-05 第八片，Action 1 的 `attachments` 文件）。
//!
//! 逐字迁自宿主 `src-tauri/src/dsh_webui.rs` 的 `parse_prompt_image`／
//! `normalize_prompt_mode`——发送链路上**先于任何 I/O** 的两道判定：图片载荷的
//! 双形态解析（`data:` URL 与 `media:data` 裸拼）＋四类 media 白名单＋空载荷
//! 拒绝，和 prompt mode 的两臂白名单。错误文案是在网契约（前端把 Err 字符串
//! 直接铺给用户的 toast），逐字保真。**组装 `content` 数组与发送留宿主**——那里
//! 混着 `Uuid::new_v4()` 的 requestId 铸造与 async RPC，判定与投递的分界就是这两行代码。
//! 首稿曾在 core 里发明过 `prompt_images_arg`（宿主不存在此形状）——已删；
//! verbatim 迁移意味着不顺手造接口（补记二百五十七自报）。

/// 一条图片字符串 → `(media_type, base64)`。
pub fn parse_prompt_image(value: &str) -> Result<(&str, &str), String> {
    let (media_type, data) = if let Some(rest) = value.strip_prefix("data:") {
        let (metadata, data) = rest
            .split_once(',')
            .ok_or_else(|| "Invalid DSH image data URL".to_string())?;
        let media_type = metadata
            .strip_suffix(";base64")
            .ok_or_else(|| "DSH image data URL must use base64 encoding".to_string())?;
        (media_type, data)
    } else {
        value
            .split_once(':')
            .ok_or_else(|| "Invalid DSH image payload".to_string())?
    };
    if !matches!(
        media_type,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    ) {
        return Err(format!("Unsupported DSH image media type {media_type:?}"));
    }
    if data.is_empty() {
        return Err("DSH image payload is empty".to_string());
    }
    Ok((media_type, data))
}

/// `queue`／`steer` 两臂白名单；其余响亮拒绝（文案含期望值）。
pub fn normalize_prompt_mode(mode: &str) -> Result<&str, String> {
    match mode {
        "queue" | "steer" => Ok(mode),
        other => Err(format!(
            "Unsupported DSH prompt mode {other:?}; expected queue or steer"
        )),
    }
}
