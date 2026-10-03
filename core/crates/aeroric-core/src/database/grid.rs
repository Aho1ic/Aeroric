//! P7-04 Action 1/4：grid 分页纯函数（core 独占；**百万行结果只请求有界页，
//! 内存不加载全表**——分页/页大小 clamp 与 grid 纯函数；DataGrid SQL 构造在
//! dbx-core data_grid_sql，这里承载分页守卫）。
use serde::Deserialize;

/// 表数据请求（与 Tauri `TableDataRequest` camelCase 字段对齐）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDataRequest {
    pub connection_id: String,
    #[serde(default)]
    pub database: Option<String>,
    pub table: String,
    #[serde(default)]
    pub page: usize,
    #[serde(default)]
    pub page_size: usize,
    #[serde(default)]
    pub order_by: Option<String>,
    #[serde(default)]
    pub where_input: Option<String>,
}

/// 页大小归一（verbatim：1..=500 clamp；百万行只请求有界页）。
pub fn normalize_page_size(page_size: usize) -> usize {
    page_size.clamp(1, 500)
}

/// 分页守卫（百万行结果只请求有界页，内存不加载全表——page/page_size clamp
/// 后的 offset 计算；无界请求拒绝）。
pub fn bounded_offset(page: usize, page_size: usize) -> Result<(usize, usize), String> {
    let size = normalize_page_size(page_size);
    if page == 0 {
        return Err("page must be >= 1".to_string());
    }
    let offset = (page - 1).saturating_mul(size);
    // 有界：offset 上限（防深分页 DoS；与宿主 MAX 上限同语义）。
    const MAX_OFFSET: usize = 10_000_000;
    if offset > MAX_OFFSET {
        return Err("page offset exceeds the bounded limit".to_string());
    }
    Ok((offset, size))
}

/// grid 条件保护（Action 4：编辑使用原 DBX 条件保护，不拼 Swift SQL——
/// where_input 的 NUL/分隔符守卫）。
pub fn validate_where_input(where_input: &str) -> Result<(), String> {
    if where_input.contains('\0') {
        return Err("where input contains NUL".to_string());
    }
    if where_input.contains(';') {
        return Err("where input contains multiple statements".to_string());
    }
    Ok(())
}
