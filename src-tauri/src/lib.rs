use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;
use tauri::Manager;

use usage::CodexRpcClient;

mod agent_assist;
mod analytics;
mod app_settings;
mod conda;
mod config;
mod dap;
mod database;
mod diagnostics;
mod docker;
mod event_watcher;
mod formatter;
mod fs;
mod git;
mod hooks;
mod local_history;
mod lsp;
mod notification;
mod platform;
mod ports;
mod pty;
mod remote_fs;
mod remote_git;
mod run_config;
mod search;
mod session;
mod sftp;
mod skills;
mod ssh;
mod storage;
mod subprocess;
mod tests;
mod usage;

use session::{ClaudeSessionInfo, CodexSessionInfo};

pub struct TaskManager {
    pub(crate) pty_masters:
        Mutex<HashMap<String, Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>>>,
    pub(crate) pty_writers: Mutex<HashMap<String, Arc<Mutex<Box<dyn Write + Send>>>>>,
    pub(crate) child_handles:
        Mutex<HashMap<String, Arc<std::sync::Mutex<Box<dyn portable_pty::Child + Send + Sync>>>>>,
    pub(crate) cancelled_tasks: Mutex<HashSet<String>>,
    pub(crate) manually_completed_tasks: Mutex<HashSet<String>>,
    pub(crate) codex_sessions: Mutex<HashMap<String, CodexSessionInfo>>,
    pub(crate) claude_sessions: Mutex<HashMap<String, ClaudeSessionInfo>>,
    pub(crate) claimed_session_paths: Mutex<HashSet<String>>,
    /// Persistent `codex app-server` process reused across `read_usage_snapshot` calls.
    pub(crate) codex_rpc: Arc<Mutex<Option<CodexRpcClient>>>,
}

impl TaskManager {
    /// Atomically remove a task/shell from all PTY maps (masters, writers, children).
    /// Locks are acquired in a fixed order to prevent deadlocks.
    pub(crate) fn remove_pty_handles(&self, id: &str) {
        let mut masters = self.pty_masters.lock();
        let mut writers = self.pty_writers.lock();
        let mut children = self.child_handles.lock();
        masters.remove(id);
        writers.remove(id);
        children.remove(id);
    }
}

/// macOS: 把主窗口收起到 Dock(hide 而非退出)。
///
/// 原生全屏窗口独占一个 Space,直接 hide 会留下空 Space(黑屏),必须先退出全屏。
/// 但退出全屏是带动画的异步过渡:动画结束前 `is_fullscreen()` 仍为 true,且刚结束
/// 的一小段时间内 `hide()` 仍会被系统忽略。故先轮询等退出完成,再间隔多次 hide,
/// 让稍晚的调用落在 Space 收起之后生效(对已隐藏窗口为无操作)。
/// 见 tauri-apps/tauri#12056、electron/electron#20263。
#[cfg(target_os = "macos")]
fn hide_window_to_dock(window: tauri::Window) {
    use std::time::Duration;
    if !window.is_fullscreen().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    let _ = window.set_fullscreen(false);
    std::thread::spawn(move || {
        // 轮询等退出全屏完成(~5s 兜底)。
        let mut exited = false;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            if !window.is_fullscreen().unwrap_or(false) {
                exited = true;
                break;
            }
        }
        // 仍处于全屏(退出失败/超时)时绝不 hide,否则会重新留下黑屏的空 Space。
        if !exited {
            return;
        }
        // 退出后仍可能短暂忽略 hide,间隔多次覆盖 Space 收起的残余时间。
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(120));
            let _ = window.hide();
        }
    });
}

/// 前端 Cmd+W 走此命令收起窗口,复用与关闭按钮一致的全屏感知隐藏逻辑。
/// 仅 macOS 有实际行为(其他平台前端不会触发,见 App.tsx)。
#[tauri::command]
fn hide_main_window(window: tauri::Window) {
    #[cfg(target_os = "macos")]
    hide_window_to_dock(window);
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dbx_state = crate::database::dbx_state::DbxState::new_blocking()
                .expect("Failed to initialize DBX database state");
            app.manage(dbx_state);
            // 后台预热 login shell 环境，避免第一次启动任务时阻塞
            std::thread::spawn(|| {
                crate::app_settings::get_login_shell_path();
            });
            // 安装 hook 脚本与用户级配置注入(失败不阻塞启动,前端可查询状态)。
            // 结果写入缓存,供 run_task/resume_task 的 hook 信任检查零阻塞读取。
            std::thread::spawn(|| {
                crate::hooks::cache_status(crate::hooks::ensure_installed());
            });
            // 启动 hook 事件文件 watcher
            crate::event_watcher::start(app.handle().clone());
            Ok(())
        })
        .manage(TaskManager {
            pty_masters: Mutex::new(HashMap::new()),
            pty_writers: Mutex::new(HashMap::new()),
            child_handles: Mutex::new(HashMap::new()),
            cancelled_tasks: Mutex::new(HashSet::new()),
            manually_completed_tasks: Mutex::new(HashSet::new()),
            codex_sessions: Mutex::new(HashMap::new()),
            claude_sessions: Mutex::new(HashMap::new()),
            claimed_session_paths: Mutex::new(HashSet::new()),
            codex_rpc: Arc::new(Mutex::new(None)),
        })
        .manage(run_config::RunConfigState::default())
        .manage(dap::DebugState::default())
        .on_window_event(|window, event| {
            // macOS: 点关闭按钮(红灯)时隐藏窗口而非退出,与 Cmd+W 行为一致;
            // 点 Dock 图标可唤回(见下方 Reopen 处理)。
            // 其他平台没有托盘/Dock 唤回入口,保持默认退出行为,避免窗口隐藏后无法找回。
            #[cfg(target_os = "macos")]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                hide_window_to_dock(window.clone());
                api.prevent_close();
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (window, event);
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            hide_main_window,
            pty::run_task,
            pty::resume_task,
            pty::cancel_task,
            pty::complete_task,
            pty::get_active_task_ids,
            pty::reset_task_process,
            pty::send_input,
            pty::resize_pty,
            pty::open_shell,
            pty::kill_shell,
            fs::read_dir_entries,
            fs::open_in_system_file_manager,
            fs::read_file_content,
            fs::read_image_preview,
            fs::write_file_content,
            formatter::format_file,
            local_history::list_local_history,
            local_history::read_local_history_entry,
            local_history::restore_local_history_entry,
            fs::create_file,
            fs::create_directory,
            fs::delete_path,
            fs::rename_path,
            fs::copy_paths_to_directory,
            fs::read_clipboard_file_paths,
            fs::list_project_files,
            fs::search_project_files,
            search::search_text,
            search::remote_search_text,
            search::search_structured,
            search::remote_search_structured,
            search::replace_text_preview,
            search::remote_replace_text_preview,
            search::apply_text_replacements,
            search::remote_apply_text_replacements,
            dap::read_debug_configs,
            dap::remote_read_debug_configs,
            dap::write_debug_configs,
            dap::remote_write_debug_configs,
            dap::start_debug_config,
            dap::remote_start_debug_config,
            dap::continue_debug_config,
            dap::step_over_debug_config,
            dap::step_into_debug_config,
            dap::step_out_debug_config,
            dap::expand_debug_variable,
            dap::evaluate_debug_expression,
            dap::stop_debug_config,
            dap::read_debug_session,
            lsp::lsp_server_status,
            lsp::remote_lsp_server_status,
            lsp::lsp_open_document,
            lsp::remote_lsp_open_document,
            lsp::lsp_change_document,
            lsp::remote_lsp_change_document,
            lsp::lsp_close_document,
            lsp::remote_lsp_close_document,
            lsp::lsp_shutdown_project,
            lsp::remote_lsp_shutdown_project,
            lsp::lsp_hover,
            lsp::remote_lsp_hover,
            lsp::lsp_definition,
            lsp::remote_lsp_definition,
            lsp::lsp_references,
            lsp::remote_lsp_references,
            lsp::lsp_rename,
            lsp::remote_lsp_rename,
            lsp::lsp_apply_workspace_edit,
            lsp::remote_lsp_apply_workspace_edit,
            lsp::lsp_completion,
            lsp::remote_lsp_completion,
            lsp::lsp_signature_help,
            lsp::remote_lsp_signature_help,
            lsp::lsp_code_actions,
            lsp::remote_lsp_code_actions,
            lsp::lsp_execute_command,
            lsp::remote_lsp_execute_command,
            lsp::lsp_document_symbols,
            lsp::remote_lsp_document_symbols,
            lsp::lsp_inlay_hints,
            lsp::remote_lsp_inlay_hints,
            lsp::lsp_workspace_symbols,
            lsp::remote_lsp_workspace_symbols,
            run_config::read_run_configs,
            run_config::write_run_configs,
            run_config::remote_read_run_configs,
            run_config::remote_write_run_configs,
            run_config::start_run_config,
            run_config::remote_start_run_config,
            run_config::stop_run_config,
            run_config::read_run_process,
            tests::discover_tests,
            tests::run_tests,
            tests::remote_discover_tests,
            tests::remote_run_tests,
            remote_fs::remote_read_dir_entries,
            remote_fs::remote_read_file_content,
            remote_fs::remote_read_image_preview,
            remote_fs::remote_write_file_content,
            remote_fs::remote_create_file,
            remote_fs::remote_create_directory,
            remote_fs::remote_delete_path,
            remote_fs::remote_rename_path,
            remote_fs::remote_copy_paths_to_directory,
            remote_fs::remote_upload_local_paths_to_directory,
            sftp::sftp_read_dir,
            sftp::sftp_read_text_file,
            sftp::sftp_read_image_preview,
            sftp::sftp_read_directory_summary,
            sftp::sftp_create_directory,
            sftp::sftp_delete_paths,
            sftp::sftp_rename_path,
            sftp::sftp_copy_paths,
            sftp::sftp_move_paths,
            git::generate_commit_message,
            agent_assist::generate_task_name,
            git::git_status,
            git::git_list_branches,
            git::git_create_branch,
            git::git_checkout_branch,
            git::git_log,
            git::git_commit_detail,
            git::git_show_diff,
            git::git_show_file_diff,
            git::git_file_diff,
            git::git_stage,
            git::git_unstage,
            git::git_stage_files,
            git::git_unstage_files,
            git::git_stage_all,
            git::git_unstage_all,
            git::git_commit,
            git::git_discard_file,
            git::git_discard_files,
            git::git_discard_all,
            git::git_push,
            git::git_pull,
            git::git_remote_counts,
            git::git_blame_file,
            git::git_branch_graph,
            git::git_stash_list,
            git::git_stash_diff,
            git::git_stash_push,
            git::git_stash_apply,
            git::git_stash_drop,
            git::git_conflict_files,
            git::git_conflict_preview,
            git::git_resolve_conflict,
            ports::list_listening_ports,
            ports::remote_list_listening_ports,
            ports::remote_open_preview_tunnel,
            git::create_task_worktree,
            git::merge_task_worktree,
            git::remove_task_worktree,
            git::worktree_diff_stats,
            remote_git::remote_git_status,
            remote_git::remote_git_changes,
            remote_git::remote_git_list_branches,
            remote_git::remote_git_log,
            remote_git::remote_git_commit_detail,
            remote_git::remote_git_remote_counts,
            remote_git::remote_git_show_diff,
            remote_git::remote_git_show_commit_diff,
            remote_git::remote_git_show_file_diff,
            remote_git::remote_git_file_diff,
            remote_git::remote_git_stage,
            remote_git::remote_git_unstage,
            remote_git::remote_git_stage_files,
            remote_git::remote_git_unstage_files,
            remote_git::remote_git_stage_all,
            remote_git::remote_git_unstage_all,
            remote_git::remote_git_commit,
            remote_git::remote_git_discard_file,
            remote_git::remote_git_discard_files,
            remote_git::remote_git_discard_all,
            remote_git::remote_git_push,
            remote_git::remote_git_pull,
            remote_git::remote_git_blame_file,
            remote_git::remote_git_branch_graph,
            remote_git::remote_git_stash_list,
            remote_git::remote_git_stash_diff,
            remote_git::remote_git_stash_push,
            remote_git::remote_git_stash_apply,
            remote_git::remote_git_stash_drop,
            remote_git::remote_git_conflict_files,
            remote_git::remote_git_conflict_preview,
            remote_git::remote_git_resolve_conflict,
            diagnostics::run_diagnostics,
            diagnostics::remote_run_diagnostics,
            analytics::read_session_metrics,
            session::read_session_messages,
            session::read_session_id,
            session::recover_task_session,
            session::export_session_markdown,
            config::init_project_config,
            config::read_project_config,
            config::remote_read_project_config,
            config::write_project_config,
            config::remote_write_project_config,
            config::get_agent_config_file_path,
            config::read_agent_config_file,
            config::write_agent_config_file,
            storage::load_projects,
            storage::save_projects,
            storage::load_project_tasks,
            storage::save_project_tasks,
            storage::read_task_terminal_history,
            storage::delete_task_terminal_histories,
            ssh::load_ssh_connections,
            ssh::save_ssh_connections,
            ssh::open_ssh_shell,
            ssh::kill_ssh_shell,
            ssh::run_remote_task,
            ssh::resume_remote_task,
            ssh::cancel_remote_task,
            app_settings::load_app_settings,
            app_settings::save_app_settings,
            app_settings::save_agent_paths,
            app_settings::save_custom_agent_profile,
            app_settings::setup_agent_profile,
            app_settings::detect_agent_models,
            app_settings::list_agent_models,
            app_settings::update_custom_agent_models,
            app_settings::delete_custom_agent_profile,
            app_settings::rename_custom_agent_profile,
            app_settings::save_send_shortcut,
            app_settings::save_shift_enter_newline,
            app_settings::detect_agent_paths,
            app_settings::detect_agent_versions_for_settings,
            app_settings::detect_agent_version,
            app_settings::get_system_fonts,
            conda::detect_conda_environments,
            conda::detect_remote_conda_environments,
            database::legacy_sqlite::db_load_connections,
            database::legacy_sqlite::db_save_connections,
            database::legacy_sqlite::db_read_sql_file,
            database::legacy_sqlite::db_inspect,
            database::legacy_sqlite::db_query_table,
            database::legacy_sqlite::db_update_cell,
            database::legacy_sqlite::db_insert_row,
            database::legacy_sqlite::db_delete_row,
            database::legacy_sqlite::db_execute_sql,
            database::connections::dbx_list_connections,
            database::connections::dbx_save_connection,
            database::connections::dbx_delete_connection,
            database::connections::dbx_test_connection,
            database::connections::dbx_connect,
            database::connections::dbx_disconnect,
            database::connections::dbx_backup_sqlite_database,
            database::schema::dbx_list_databases,
            database::schema::dbx_list_schemas,
            database::schema::dbx_list_objects,
            database::schema::dbx_get_columns,
            database::schema::dbx_get_table_ddl,
            database::schema::dbx_get_object_source,
            database::query::dbx_execute_query,
            database::query::dbx_execute_multi,
            database::query::dbx_cancel_query,
            database::query::dbx_close_result_session,
            database::query::dbx_build_table_structure_change_sql,
            database::query::dbx_build_single_column_alter_sql,
            database::query::dbx_build_create_database_sql,
            database::query::dbx_build_duckdb_attach_database_sql,
            database::query::dbx_build_rename_object_sql,
            database::query::dbx_build_drop_database_sql,
            database::query::dbx_build_create_schema_sql,
            database::query::dbx_build_drop_schema_sql,
            database::query::dbx_build_drop_table_sql,
            database::query::dbx_build_truncate_table_sql,
            database::query::dbx_build_empty_table_sql,
            database::query::dbx_build_drop_object_sql,
            database::query::dbx_build_drop_table_child_object_sql,
            database::query::dbx_build_duplicate_table_structure_sql,
            database::query::dbx_build_database_search_sql,
            database::query::dbx_build_search_result_where,
            database::grid::dbx_query_table_data,
            database::grid::dbx_update_cell,
            database::grid::dbx_insert_row,
            database::grid::dbx_delete_rows,
            database::grid::dbx_preview_grid_sql,
            database::grid::dbx_build_data_grid_context_filter_condition,
            database::grid::dbx_build_data_grid_copy_insert_statement,
            database::grid::dbx_build_data_grid_copy_update_statements,
            database::import_export::dbx_export_table_csv,
            database::import_export::dbx_export_table_json,
            database::import_export::dbx_export_table_markdown,
            database::import_export::dbx_export_table_insert_sql,
            database::import_export::dbx_export_table_update_sql,
            database::import_export::dbx_export_table_xlsx,
            database::import_export::dbx_preview_table_import_file,
            database::import_export::dbx_import_table_file,
            database::import_export::dbx_export_database,
            database::import_export::dbx_execute_sql_file,
            database::redis::dbx_redis_list_databases,
            database::redis::dbx_redis_scan_keys,
            database::redis::dbx_redis_get_value,
            database::redis::dbx_redis_load_more,
            database::redis::dbx_redis_set_value,
            database::redis::dbx_redis_delete_key,
            database::redis::dbx_redis_set_ttl,
            database::redis::dbx_redis_create_key,
            database::redis::dbx_redis_hash_del,
            database::redis::dbx_redis_hash_set,
            database::redis::dbx_redis_list_remove,
            database::redis::dbx_redis_list_push,
            database::redis::dbx_redis_list_set,
            database::redis::dbx_redis_set_remove,
            database::redis::dbx_redis_set_add,
            database::redis::dbx_redis_zrem,
            database::redis::dbx_redis_zadd,
            database::redis::dbx_redis_execute_command,
            database::mongo::dbx_mongo_list_databases,
            database::mongo::dbx_mongo_list_collections,
            database::mongo::dbx_mongo_find_documents,
            database::mongo::dbx_mongo_insert_document,
            database::mongo::dbx_mongo_update_document,
            database::mongo::dbx_mongo_delete_documents,
            database::drivers::dbx_driver_manifest,
            database::transfer::dbx_start_transfer,
            database::transfer::dbx_cancel_transfer,
            database::transfer::dbx_prepare_schema_diff,
            database::transfer::dbx_generate_schema_sync_sql,
            database::transfer::dbx_prepare_data_compare,
            database::transfer::dbx_build_data_compare_sync_plan,
            database::transfer::dbx_prepare_data_compare_from_tables,
            docker::list_docker_resources,
            docker::docker_container_action,
            docker::docker_container_logs,
            docker::docker_delete_image,
            docker::docker_tag_image,
            notification::get_notifications,
            notification::get_pending_release_update,
            notification::mark_notification_read,
            notification::mark_all_notifications_read,
            notification::prepare_release_update,
            notification::restart_and_install_release_update,
            notification::install_release_update,
            usage::read_usage_snapshot,
            hooks::get_hook_status,
            hooks::get_hook_readiness,
            hooks::install_hooks,
            hooks::uninstall_hooks,
            skills::get_skill_hub_config,
            skills::set_skill_hub_path,
            skills::clear_skill_hub,
            skills::list_skills,
            skills::list_skill_installations,
            skills::install_skill,
            skills::uninstall_skill,
            skills::cleanup_installations_for_project,
            skills::delete_skill,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app_handle, _event| {
            // macOS: 当窗口被 Cmd+W 隐藏（hide）后，点击 Dock 图标会触发 Reopen，
            // 此时没有可见窗口，需要手动把主窗口重新显示并聚焦。
            #[cfg(target_os = "macos")]
            {
                use tauri::Manager;
                if let tauri::RunEvent::Reopen { .. } = _event {
                    if let Some(window) = _app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        });
}
