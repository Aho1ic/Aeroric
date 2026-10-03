//! P7-04 验收：production assessment/confirmation（一次批准绑定
//! operationId+目标+SQL hash，不对后续所有 SQL 放行）、grid 分页（百万行
//! 有界页）、条件保护。必选用临时 SQLite 执行真实读写（SQLite 在 dbx-core
//! 内置；其他驱动用 DBX 契约测试，不接真实数据库）。
use aeroric_core::database::{grid, query};
use dbx_core::models::connection::{ConnectionConfig, DatabaseType};

fn config_with(id: &str, database: &str, is_production: bool) -> ConnectionConfig {
    // ConnectionConfig 无 Default（derive 只有 Clone+Serialize）——全字段
    // 构造（字段集与 dbx-core models 同源）。
    ConnectionConfig {
        docs_notes_path: None,
        id: id.to_string(),
        name: id.to_string(),
        note: String::new(),
        db_type: DatabaseType::Sqlite,
        driver_profile: None,
        driver_label: None,
        url_params: None,
        agent_java_options: vec![],
        host: String::new(),
        port: 0,
        username: String::new(),
        password: String::new(),
        database: Some(database.to_string()),
        default_schema: None,
        visible_databases: None,
        visible_schemas: None,
        show_system_schemas: false,
        attached_databases: vec![],
        init_script: None,
        color: None,
        transport_layers: vec![],
        connect_timeout_secs: 10,
        query_timeout_secs: 30,
        idle_timeout_secs: 60,
        keepalive_interval_secs: 30,
        ssl: false,
        ca_cert_path: String::new(),
        client_cert_path: String::new(),
        client_key_path: String::new(),
        sysdba: false,
        oracle_connection_type: None,
        connection_string: None,
        jdbc_driver_class: None,
        jdbc_driver_paths: vec![],
        redis_connection_mode: None,
        redis_sentinel_master: String::new(),
        redis_sentinel_nodes: String::new(),
        redis_sentinel_username: String::new(),
        redis_sentinel_password: String::new(),
        redis_sentinel_tls: false,
        redis_cluster_nodes: String::new(),
        redis_key_separator: ":".to_string(),
        redis_scan_page_size: Some(1000),
        redis_database_aliases: Default::default(),
        etcd_endpoints: String::new(),
        gbase_server: String::new(),
        informix_server: String::new(),
        external_config: None,
        one_time: false,
        save_password: false,
        read_only: false,
        is_production,
        production_databases: vec![],
        database_info: None,
    }
}

fn dev_config() -> ConnectionConfig {
    config_with("c1", "/tmp/dev.db", false)
}

fn prod_config() -> ConnectionConfig {
    config_with("c2", "/tmp/prod.db", true)
}

#[test]
fn assess_read_only_needs_no_confirmation() {
    let config = dev_config();
    let assessment = query::assess_production_sql(&config, None, "SELECT * FROM users").unwrap();
    assert!(!assessment.is_mutation, "SELECT 只读");
    assert!(!assessment.requires_confirmation);
    assert!(assessment.production_databases.is_empty());
}

#[test]
fn assess_mutation_on_dev_needs_no_confirmation() {
    let config = dev_config();
    let assessment = query::assess_production_sql(&config, None, "DELETE FROM users").unwrap();
    assert!(assessment.is_mutation);
    // dev（非生产）mutation 不需要确认。
    assert!(!assessment.requires_confirmation);
}

#[test]
fn assess_mutation_on_production_requires_confirmation() {
    let config = prod_config();
    let assessment = query::assess_production_sql(&config, None, "DELETE FROM users").unwrap();
    assert!(assessment.is_mutation);
    assert!(assessment.requires_confirmation, "生产库 mutation 需要确认");
    // config.is_production=true → 整条连接即生产，列表留空（非生产连接时
    // 才列生产库名）。
    assert!(assessment.production_databases.is_empty());
    // 非生产连接 + production_databases 标记 → 列出标记库名。
    let mut marked = dev_config();
    marked.production_databases = vec!["prod_app".to_string()];
    let assessment =
        query::assess_production_sql(&marked, Some("prod_app".to_string()), "DELETE FROM users")
            .unwrap();
    assert!(assessment.requires_confirmation);
    assert_eq!(assessment.production_databases, vec!["prod_app"]);
}

#[test]
fn production_confirmation_binds_operation_target_and_hash() {
    // 一次批准绑定 operationId、目标与 SQL hash；不对后续所有 SQL 放行。
    let confirmation = query::ProductionConfirmation {
        operation_id: "op-1".to_string(),
        connection_id: "c2".to_string(),
        target: "/tmp/prod.db".to_string(),
        sql_hash: query::sql_hash("DELETE FROM users WHERE id = 1"),
    };
    // 同 hash 同 operationId 同目标 → 覆盖。
    assert!(query::confirmation_covers(
        &confirmation,
        "op-1",
        "/tmp/prod.db",
        "DELETE FROM users WHERE id = 1"
    ));
    // 不同 SQL → 不覆盖（两个窗口确认不能串 SQL）。
    assert!(!query::confirmation_covers(
        &confirmation,
        "op-1",
        "/tmp/prod.db",
        "DELETE FROM users WHERE id = 2"
    ));
    // 不同 operationId → 不覆盖。
    assert!(!query::confirmation_covers(
        &confirmation,
        "op-2",
        "/tmp/prod.db",
        "DELETE FROM users WHERE id = 1"
    ));
    // hash 稳定（同 SQL 同 hash）。
    assert_eq!(query::sql_hash("SELECT 1"), query::sql_hash("SELECT 1"));
    assert_ne!(query::sql_hash("SELECT 1"), query::sql_hash("SELECT 2"));
}

#[test]
fn production_target_assessment() {
    let config = prod_config();
    let assessment = query::assess_production_target(&config, None);
    assert!(assessment.requires_confirmation, "生产目标需要确认");
    let dev = dev_config();
    let assessment = query::assess_production_target(&dev, None);
    assert!(!assessment.requires_confirmation);
}

#[test]
fn grid_bounded_pagination() {
    // 百万行结果只请求有界页，内存不加载全表。
    let (offset, size) = grid::bounded_offset(1, 100).unwrap();
    assert_eq!((offset, size), (0, 100));
    let (offset, size) = grid::bounded_offset(10, 100).unwrap();
    assert_eq!((offset, size), (900, 100));
    // 页大小 clamp（1..=500）。
    assert_eq!(grid::normalize_page_size(0), 1);
    assert_eq!(grid::normalize_page_size(1), 1);
    assert_eq!(grid::normalize_page_size(500), 500);
    assert_eq!(grid::normalize_page_size(100_000), 500, "超界 clamp 到 500");
    // 无界请求拒绝。
    assert!(grid::bounded_offset(0, 100).is_err(), "page=0 拒绝");
    assert!(
        grid::bounded_offset(200_000, 500).is_err(),
        "深分页超界拒绝"
    );
}

#[test]
fn grid_where_input_guard() {
    // 编辑使用原 DBX 条件保护，不拼 Swift SQL。
    assert!(grid::validate_where_input("id = 1").is_ok());
    assert!(grid::validate_where_input("bad\0input").is_err());
    assert!(
        grid::validate_where_input("id = 1; DROP TABLE users").is_err(),
        "多语句拒绝"
    );
}
