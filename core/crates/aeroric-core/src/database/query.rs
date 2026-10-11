//! P7-04 Action 1：database 模块（core 独占；production assessment/SQL risk
//! 纯函数——与 `src-tauri/src/database/query.rs` 同源语义 verbatim 上收；
//! DBX 业务主体在 dbx-core，这里承载 assessment 形状与确认链）。
use dbx_core::models::connection::ConnectionConfig;
use dbx_core::sql_risk::SqlRisk;
use serde::{Deserialize, Serialize};

/// SQL 风险评估结果（与 Tauri `ProductionSqlAssessment` camelCase 字段对齐）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionSqlAssessment {
    pub requires_confirmation: bool,
    pub is_mutation: bool,
    pub production_databases: Vec<String>,
}

/// 目标评估结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionTargetAssessment {
    pub requires_confirmation: bool,
    pub production_databases: Vec<String>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// SQL 生产风险评估（verbatim 上收；sql_risk 分类在 dbx-core）。
pub fn assess_production_sql(
    config: &ConnectionConfig,
    database: Option<String>,
    sql: &str,
) -> Result<ProductionSqlAssessment, String> {
    let risk = dbx_core::sql_risk::classify_sql_risk_for_database(sql, config.db_type)?;
    let is_mutation = risk != SqlRisk::ReadOnly;
    let active_database = non_empty(database)
        .or_else(|| config.database.clone())
        .unwrap_or_default();
    let requires_confirmation = is_mutation
        && dbx_core::production_safety::targets_production_database(config, &active_database, sql);
    let production_databases = if !requires_confirmation || config.is_production {
        Vec::new()
    } else if dbx_core::production_safety::is_production_database(config, &active_database) {
        vec![active_database]
    } else {
        config.production_databases.clone()
    };

    Ok(ProductionSqlAssessment {
        requires_confirmation,
        is_mutation,
        production_databases,
    })
}

/// 目标生产评估（verbatim 上收）。
pub fn assess_production_target(
    config: &ConnectionConfig,
    database: Option<String>,
) -> ProductionTargetAssessment {
    let active_database = non_empty(database).or_else(|| config.database.clone());
    let requires_confirmation = config.is_production
        || active_database
            .as_deref()
            .map(|database| dbx_core::production_safety::is_production_database(config, database))
            .unwrap_or_else(|| !config.production_databases.is_empty());
    let production_databases = if !requires_confirmation || config.is_production {
        Vec::new()
    } else if let Some(database) = active_database {
        vec![database]
    } else {
        config.production_databases.clone()
    };

    ProductionTargetAssessment {
        requires_confirmation,
        production_databases,
    }
}

/// 生产确认（Action 3：**一次批准绑定 operationId、目标与 SQL hash**，不对
/// 后续所有 SQL 放行——确认链纯函数）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionConfirmation {
    pub operation_id: String,
    pub connection_id: String,
    pub target: String,
    pub sql_hash: String,
}

/// SQL hash（SHA-256 十六进制；确认绑定的 SQL 摘要）。
pub fn sql_hash(sql: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(sql.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 确认是否覆盖本次执行（一次批准绑定 operationId+目标+SQL hash；不同 hash
/// 或 operationId＝不覆盖）。
pub fn confirmation_covers(
    confirmation: &ProductionConfirmation,
    operation_id: &str,
    target: &str,
    sql: &str,
) -> bool {
    confirmation.operation_id == operation_id
        && confirmation.target == target
        && confirmation.sql_hash == sql_hash(sql)
}
