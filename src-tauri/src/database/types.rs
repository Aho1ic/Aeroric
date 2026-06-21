use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum DbxDatabaseType {
    Sqlite,
    Mysql,
    Postgres,
    Duckdb,
    Redis,
    Mongodb,
    Sqlserver,
    Oracle,
    Clickhouse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectScope {
    pub kind: String,
    pub project_root: Option<String>,
    pub remote_project_path: Option<String>,
    pub ssh_connection_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AeroricDbConnectionConfig {
    pub id: String,
    pub name: String,
    pub db_type: DbxDatabaseType,
    pub read_only: bool,
    pub project_scope: Option<ProjectScope>,
    pub dbx: serde_json::Value,
    pub created_at: i64,
    pub last_opened_at: Option<i64>,
    pub migrated_from_legacy: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
}
