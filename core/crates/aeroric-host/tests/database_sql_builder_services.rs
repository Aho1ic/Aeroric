//! Slice 96: SQL builder / grid preview pure functions served by the host.
//! Pure functions: no state, no secrets, no IO — every reply is computed from
//! the request alone, so the assertions are exact SQL strings carried over the
//! wire (legacy expectations verbatim). Options are built through the core
//! re-exports so the wire exercises the real serde of the real types.

use aeroric_core::database::grid as grid_biz;
use aeroric_core::database::sql_builders as builders;
use aeroric_protocol::envelope::Frame;
use aeroric_protocol::error::ErrorCode;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

mod host_harness;
use host_harness::Harness;

fn success(reply: Frame) -> Value {
    match reply {
        Frame::Success(reply) => reply.result,
        other => panic!("expected success, got {other:?}"),
    }
}

fn failure_code(reply: Frame) -> ErrorCode {
    let Frame::Failure(reply) = reply else {
        panic!("expected failure, got {reply:?}");
    };
    reply.error.code
}

async fn call(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    params: Value,
) -> Frame {
    Harness::call(
        reader,
        writer,
        &Harness::request("sql-builder-request", method, params),
    )
    .await
}

async fn call_options(
    reader: &mut BufReader<OwnedReadHalf>,
    writer: &mut OwnedWriteHalf,
    method: &str,
    options: Value,
) -> Frame {
    call(reader, writer, method, json!({ "options": options })).await
}

async fn authenticated(host: &Harness) -> (BufReader<OwnedReadHalf>, OwnedWriteHalf) {
    let (mut reader, mut writer) = host.connect().await;
    success(Harness::handshake(&mut reader, &mut writer, &host.token, "sql-builders").await);
    (reader, writer)
}

fn postgres() -> Option<builders::DatabaseType> {
    Some(builders::DatabaseType::Postgres)
}

fn plain_column(
    name: &str,
    data_type: &str,
    is_primary_key: bool,
) -> builders::EditableStructureColumn {
    builders::EditableStructureColumn {
        id: format!("col-{name}"),
        name: name.to_string(),
        data_type: data_type.to_string(),
        is_nullable: !is_primary_key,
        default_value: String::new(),
        comment: String::new(),
        is_primary_key,
        extra: None,
        original: None,
        original_position: None,
        marked_for_drop: false,
        character_set: String::new(),
        collation: String::new(),
    }
}

fn test_column_info() -> grid_biz::DataGridColumnInfo {
    grid_biz::DataGridColumnInfo {
        name: "id".to_string(),
        data_type: "integer".to_string(),
        is_nullable: false,
        is_primary_key: true,
        column_default: None,
        extra: None,
    }
}

fn test_table_meta() -> grid_biz::DataGridTableMeta {
    grid_biz::DataGridTableMeta {
        catalog: None,
        database: None,
        schema: Some("public".to_string()),
        table_name: "users".to_string(),
        primary_keys: vec!["id".to_string()],
        columns: Some(vec![
            test_column_info(),
            grid_biz::DataGridColumnInfo {
                name: "email".to_string(),
                data_type: "text".to_string(),
                is_nullable: true,
                is_primary_key: false,
                column_default: None,
                extra: None,
            },
        ]),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn twenty_sql_builder_methods_are_served_by_the_host() {
    let host = Harness::start("sql-builders").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    // ── db_admin_sql 族（legacy 期望逐字）──
    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_create_database_sql",
            serde_json::to_value(builders::CreateDatabaseSqlOptions {
                database_type: Some(builders::DatabaseType::Mysql),
                driver_profile: Some("mysql".to_string()),
                target: None,
                parent: None,
                name: "app".to_string(),
                charset: Some("utf8mb4".to_string()),
                collation: Some("utf8mb4_unicode_ci".to_string()),
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(
        reply,
        json!("CREATE DATABASE `app` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;")
    );

    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_duckdb_attach_database_sql",
            serde_json::to_value(builders::DuckDbAttachDatabaseSqlOptions {
                path: "/tmp/app's.duckdb".to_string(),
                name: "app db".to_string(),
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(reply, json!("ATTACH '/tmp/app''s.duckdb' AS \"app db\";"));

    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_drop_database_sql",
            serde_json::to_value(builders::DatabaseNameSqlOptions {
                database_type: postgres(),
                name: "app".to_string(),
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(reply, json!("DROP DATABASE \"app\";"));

    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_create_schema_sql",
            serde_json::to_value(builders::SchemaNameSqlOptions {
                database_type: postgres(),
                name: "analytics".to_string(),
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(reply, json!("CREATE SCHEMA \"analytics\";"));

    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_drop_schema_sql",
            serde_json::to_value(builders::SchemaNameSqlOptions {
                database_type: postgres(),
                name: "analytics".to_string(),
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(reply, json!("DROP SCHEMA \"analytics\" CASCADE;"));

    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_rename_object_sql",
            serde_json::to_value(builders::RenameObjectSqlOptions {
                database_type: postgres(),
                object_type: builders::DatabaseObjectType::Table,
                schema: Some("public".to_string()),
                old_name: "users".to_string(),
                new_name: "app_users".to_string(),
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(
        reply,
        json!("ALTER TABLE \"public\".\"users\" RENAME TO \"app_users\";")
    );

    // ── db_admin_sql 族其余七条：wire 回执与 core 直调逐字节一致 ──
    let admin_cases: Vec<(&str, Value, Value)> = vec![
        (
            "database.dbx_build_drop_table_sql",
            serde_json::to_value(builders::TableAdminSqlOptions {
                database_type: postgres(),
                schema: Some("public".to_string()),
                table_name: "users".to_string(),
                cascade: Some(true),
                identifier_quote: None,
            })
            .unwrap(),
            serde_json::to_value(builders::build_drop_table_sql(
                builders::TableAdminSqlOptions {
                    database_type: postgres(),
                    schema: Some("public".to_string()),
                    table_name: "users".to_string(),
                    cascade: Some(true),
                    identifier_quote: None,
                },
            ))
            .unwrap(),
        ),
        (
            "database.dbx_build_truncate_table_sql",
            serde_json::to_value(builders::TableAdminSqlOptions {
                database_type: postgres(),
                schema: Some("public".to_string()),
                table_name: "users".to_string(),
                cascade: None,
                identifier_quote: None,
            })
            .unwrap(),
            serde_json::to_value(builders::build_truncate_table_sql(
                builders::TableAdminSqlOptions {
                    database_type: postgres(),
                    schema: Some("public".to_string()),
                    table_name: "users".to_string(),
                    cascade: None,
                    identifier_quote: None,
                },
            ))
            .unwrap(),
        ),
        (
            "database.dbx_build_empty_table_sql",
            serde_json::to_value(builders::TableAdminSqlOptions {
                database_type: postgres(),
                schema: Some("public".to_string()),
                table_name: "users".to_string(),
                cascade: None,
                identifier_quote: None,
            })
            .unwrap(),
            serde_json::to_value(builders::build_empty_table_sql(
                builders::TableAdminSqlOptions {
                    database_type: postgres(),
                    schema: Some("public".to_string()),
                    table_name: "users".to_string(),
                    cascade: None,
                    identifier_quote: None,
                },
            ))
            .unwrap(),
        ),
        (
            "database.dbx_build_drop_object_sql",
            serde_json::to_value(builders::DropObjectSqlOptions {
                database_type: postgres(),
                object_type: builders::DatabaseObjectType::View,
                schema: Some("public".to_string()),
                name: "stale_view".to_string(),
                signature: None,
                identifier_quote: None,
            })
            .unwrap(),
            serde_json::to_value(builders::build_drop_object_sql(
                builders::DropObjectSqlOptions {
                    database_type: postgres(),
                    object_type: builders::DatabaseObjectType::View,
                    schema: Some("public".to_string()),
                    name: "stale_view".to_string(),
                    signature: None,
                    identifier_quote: None,
                },
            ))
            .unwrap(),
        ),
        (
            "database.dbx_build_drop_table_child_object_sql",
            serde_json::to_value(builders::DropTableChildObjectSqlOptions {
                database_type: postgres(),
                object_type: builders::TableChildObjectType::Index,
                schema: Some("public".to_string()),
                table_name: "users".to_string(),
                name: "users_email_idx".to_string(),
            })
            .unwrap(),
            // build_drop_table_child_object_sql 返回 Result——wire 侧已解包，
            // 期望值同样只取 Ok 半边（{"Ok": …} 是测试自身的序列化误差）。
            serde_json::to_value(
                builders::build_drop_table_child_object_sql(
                    builders::DropTableChildObjectSqlOptions {
                        database_type: postgres(),
                        object_type: builders::TableChildObjectType::Index,
                        schema: Some("public".to_string()),
                        table_name: "users".to_string(),
                        name: "users_email_idx".to_string(),
                    },
                )
                .unwrap(),
            )
            .unwrap(),
        ),
        (
            "database.dbx_build_duplicate_table_structure_sql",
            serde_json::to_value(builders::DuplicateTableStructureSqlOptions {
                database_type: postgres(),
                schema: Some("public".to_string()),
                source_name: "users".to_string(),
                target_name: "users_backup".to_string(),
                table_comment: None,
                column_comments: Vec::new(),
                identifier_quote: None,
            })
            .unwrap(),
            serde_json::to_value(builders::build_duplicate_table_structure_sql(
                builders::DuplicateTableStructureSqlOptions {
                    database_type: postgres(),
                    schema: Some("public".to_string()),
                    source_name: "users".to_string(),
                    target_name: "users_backup".to_string(),
                    table_comment: None,
                    column_comments: Vec::new(),
                    identifier_quote: None,
                },
            ))
            .unwrap(),
        ),
        (
            "database.dbx_build_single_column_alter_sql",
            serde_json::to_value(builders::SingleColumnAlterSqlOptions {
                database_type: postgres(),
                schema: Some("public".to_string()),
                table_name: "users".to_string(),
                column: plain_column("email", "text", false),
            })
            .unwrap(),
            serde_json::to_value(builders::build_single_column_alter_sql(
                builders::SingleColumnAlterSqlOptions {
                    database_type: postgres(),
                    schema: Some("public".to_string()),
                    table_name: "users".to_string(),
                    column: plain_column("email", "text", false),
                },
            ))
            .unwrap(),
        ),
    ];
    for (method, options, expected) in admin_cases {
        let reply = success(call_options(&mut reader, &mut writer, method, options).await);
        assert_eq!(reply, expected, "{method} wire reply must match core");
    }

    // ── table_structure_sql 整表变更 ──
    let structure_options = builders::TableStructureSqlOptions {
        database_type: postgres(),
        schema: Some("public".to_string()),
        table_name: "users".to_string(),
        columns: vec![plain_column("email", "text", false)],
        indexes: Vec::new(),
        foreign_keys: Vec::new(),
        triggers: Vec::new(),
        table_comment: None,
        original_table_comment: None,
        partitioned: false,
        is_gaussdb_m_mode: false,
    };
    let expected = serde_json::to_value(builders::build_table_structure_change_sql(
        structure_options.clone(),
    ))
    .unwrap();
    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_table_structure_change_sql",
            serde_json::to_value(structure_options).unwrap(),
        )
        .await,
    );
    assert_eq!(reply, expected);
    assert!(!reply["statements"].as_array().unwrap().is_empty());

    // ── database_search_sql 族（legacy 期望逐字；raw JSON 证明 decode 走真类型）──
    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_database_search_sql",
            json!({
                "databaseType": "postgres",
                "schema": "public",
                "tableName": "users",
                "columns": [
                    { "name": "id", "data_type": "integer", "is_primary_key": true },
                    { "name": "email", "data_type": "text", "is_primary_key": false }
                ],
                "term": "alice",
                "limit": 10
            }),
        )
        .await,
    );
    assert_eq!(reply["searchableColumns"], json!(["email"]));
    assert!(reply["sql"]
        .as_str()
        .unwrap()
        .contains("FROM \"public\".\"users\""));

    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_search_result_where",
            json!({
                "databaseType": "postgres",
                "columns": [
                    { "name": "id", "data_type": "integer", "is_primary_key": true }
                ],
                "resultColumns": ["id", "email"],
                "row": [42, "alice@example.com"],
                "matchedColumns": ["email"]
            }),
        )
        .await,
    );
    assert_eq!(reply, json!("\"id\" = 42"));

    // ── data_grid_sql 族（legacy 期望逐字）──
    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_data_grid_copy_insert_statement",
            serde_json::to_value(grid_biz::DataGridCopyInsertStatementOptions {
                database_type: postgres(),
                identifier_quote: None,
                table_meta: Some(test_table_meta()),
                columns: vec!["id".to_string(), "email".to_string()],
                column_types: None,
                source_columns: Some(vec![Some("id".to_string()), Some("email".to_string())]),
                rows: vec![vec![json!(1), json!("alice@example.com")]],
                exclude_primary_keys: false,
                include_computed_columns: false,
                insert_mode: Default::default(),
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(
        reply,
        json!(
            "INSERT INTO \"public\".\"users\" (\"id\", \"email\") VALUES (1, 'alice@example.com');"
        )
    );

    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_data_grid_copy_update_statements",
            serde_json::to_value(grid_biz::DataGridCopyUpdateStatementOptions {
                database_type: postgres(),
                identifier_quote: None,
                table_meta: test_table_meta(),
                columns: vec!["id".to_string(), "email".to_string()],
                source_columns: Some(vec![Some("id".to_string()), Some("email".to_string())]),
                rows: vec![vec![json!(1), json!("alice@example.com")]],
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(
        reply,
        json!([
            "UPDATE \"public\".\"users\" SET \"email\" = 'alice@example.com' WHERE \"id\" = 1;"
        ])
    );

    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_build_data_grid_context_filter_condition",
            serde_json::to_value(grid_biz::DataGridContextFilterConditionOptions {
                database_type: postgres(),
                identifier_quote: None,
                column_name: "email".to_string(),
                mode: grid_biz::DataGridContextFilterMode::Like,
                value: json!("alice@example.com"),
                values: Vec::new(),
                end_value: None,
                // legacy 期望（无 ::text cast）以 email/text 列元数据为前提——
                // 整数列的 LIKE 会被 builder 加 ::text 转型，那是另一条语义。
                column_info: Some(grid_biz::DataGridColumnInfo {
                    name: "email".to_string(),
                    data_type: "text".to_string(),
                    is_nullable: true,
                    is_primary_key: false,
                    column_default: None,
                    extra: None,
                }),
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(reply, json!("\"email\" LIKE '%alice@example.com%'"));

    // ── grid 预览：只 prepare 不执行 ──
    let reply = success(
        call_options(
            &mut reader,
            &mut writer,
            "database.dbx_preview_grid_sql",
            serde_json::to_value(grid_biz::DataGridSaveStatementOptions {
                database_type: postgres(),
                identifier_quote: None,
                table_meta: test_table_meta(),
                columns: vec!["id".to_string(), "email".to_string()],
                source_columns: None,
                rows: Vec::new(),
                dirty_rows: Vec::new(),
                deleted_rows: Vec::new(),
                new_rows: Vec::new(),
            })
            .unwrap(),
        )
        .await,
    );
    assert_eq!(reply["executed"], json!(false));
    assert_eq!(reply["rowsAffected"], json!(0));
    assert!(reply["statements"].is_array());
    assert!(reply["rollbackStatements"].is_array());
}

#[tokio::test(flavor = "multi_thread")]
async fn malformed_options_are_invalid_argument() {
    let host = Harness::start("sql-builders-malformed").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    let reply = call_options(
        &mut reader,
        &mut writer,
        "database.dbx_build_drop_database_sql",
        Value::Null,
    )
    .await;
    assert_eq!(failure_code(reply), ErrorCode::InvalidArgument);

    let reply = call(
        &mut reader,
        &mut writer,
        "database.dbx_build_drop_database_sql",
        json!({ "options": "DROP DATABASE x" }),
    )
    .await;
    assert_eq!(failure_code(reply), ErrorCode::InvalidArgument);

    // params 不是对象同样拒绝（不是 panic、不是本地回落）。
    let reply = call(
        &mut reader,
        &mut writer,
        "database.dbx_build_drop_database_sql",
        json!("options"),
    )
    .await;
    assert_eq!(failure_code(reply), ErrorCode::InvalidArgument);
}

#[tokio::test(flavor = "multi_thread")]
async fn unregistered_database_family_stays_unsupported() {
    let host = Harness::start("sql-builders-boundary").await;
    let (mut reader, mut writer) = authenticated(&host).await;

    // 切片九十七/九十八上收了执行面与 schema／驱动面；仍未上收的驱动专属
    // 面必须保持不可达：宿主不虚报能力。
    for method in [
        "database.dbx_mongo_find_documents",
        "database.dbx_start_transfer",
    ] {
        let reply = call(&mut reader, &mut writer, method, json!({})).await;
        assert_eq!(
            failure_code(reply),
            ErrorCode::UnsupportedCapability,
            "{method} must stay unregistered until the execution slice"
        );
    }
}
