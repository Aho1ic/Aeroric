use parking_lot::Mutex;
use std::sync::Arc;
use tauri::{Emitter, Manager};

use usage::CodexRpcClient;

mod agent_assist;
mod agent_family_maps;
mod agent_ops;
mod agent_tools;
mod agent_usage;
mod analytics;
mod app_settings;
mod clock;
/// 只在测试构建里编译:守卫 command 定义集合与 `generate_handler!` 注册集合一致。
#[cfg(test)]
mod command_registration_tests;
mod conda;
mod config;
/// P2-05:这个进程能不能当业务写者(ADR-021/027)的唯一判定点。
mod core_adapter;
/// P2-05 验收 #2 的证据:真执行器 + 真 PTY + 真子进程跑一项 fake 任务。只在测试构建里存在,
/// 因为它带着不会出现在产品路径上的脚手架。
#[cfg(test)]
mod core_adapter_tests;
/// 复用原生 core 的纯函数(P2-02 Action 5);写入权仍在 storage.rs,移交是 P2-05。
mod core_bridge;
mod custom_themes;
mod dap;
mod database;
mod diagnostics;
mod docker;
mod dsh_home;
mod dsh_plugins;
mod dsh_protocol;
mod dsh_webui;
mod event_names;
mod event_watcher;
mod formatter;
mod fs;
mod git;
mod git_path_guard;
mod hooks;
mod host_client;
#[cfg(test)]
mod host_client_tests;
mod host_supervisor;
mod local_history;
mod local_router;
mod local_router_commands;
mod lsp;
mod mcp;
mod node_runtime;
mod notebook;
mod notification;
mod omp_home;
mod path_guard;
mod permissions;
mod platform;
mod ports;
mod posix_fs;
mod protocol_decode;
// Not yet on the product path: `TaskService` is what will construct this, and that wiring is
// the step that also flips the writer role (ADR-021). The tests here are the evidence until then.
#[allow(
    dead_code,
    reason = "P2-05: constructed by the TaskService wiring step"
)]
mod legacy_task_executor;
mod platform_adapter;
mod pty;
mod remote;
mod remote_fs;
mod remote_git;
mod remote_os;
mod report;
mod run_config;
/// ADR-025 的 `(task_id, generation)` 命名空间：资源型操作寻址最新已注册代，
/// 意图型操作打全部在飞代。消费点是把 `TaskManager` 重键的那一步。
#[allow(
    dead_code,
    reason = "wired in by the TaskManager rekey, the next step of P2-05"
)]
mod run_registry;
mod search;
mod secrets;
mod session;
mod session_dsh;
mod session_omp;
mod sftp;
mod sftp_windows;
mod skills;
mod ssh;
mod ssh_hostkey;
mod ssh_proxy;
mod startup_diagnostics;
mod storage;
mod storage_backend;
mod storage_conn;
mod storage_location;
mod storage_oauth;
mod subprocess;
mod tests;
mod usage;
mod usage_index;
mod wsl;
mod wsl_fs;
mod wsl_git;

/// TaskManager 锁序表(2026-09 全量审查固化;新增多锁函数前先对照)。
///
/// 全局顺序(仅列会被同时持有的锁;叶子锁单独持有,不参与排序):
/// 1. `cancelled_tasks` / `manually_completed_tasks` —— 同族函数内成对出现,
///    先 cancelled 后 manually(见 pty.rs finalize/cancel/complete 三处);
/// 2. 四族 sessions(codex/claude/dsh/omp)彼此独立,从不同时持有两族;
/// 3. **嵌套持有**时的方向只有一个:`claimed_session_paths` → 会话表(dsh/omp 的登记侧)。
///    拆除侧不嵌套——先删会话条目(临时锁)、再单独取 claim 做 ADR-031 的反查释放,
///    所以它不构成反向持锁顺序。任何"同时持有两边"的新代码必须按登记侧这个方向写。
///    3a. ADR-030:判断"是否最后一个退出者"要先 retire 自己再读 `runs`(叶子锁,不嵌套)。
/// 4. `pty_masters` → `pty_writers` → `child_handles` → `pending_pty_sizes`
///    按 reset/cleanup 流程固定次序取放;
/// 5. dsh_webui 侧:`completed_tasks` → `active_sessions`(三处全局同序,
///    见 dispatch_mux_frame / send_terminal_text_if_current / begin_task_completion);
/// 6. `codex_rpc`、`wsl_active_ids`、`initial_input_signals`、`runs` 为叶子锁。
///
/// 约束:任何锁不得跨越 `.await` 持有(parking_lot 无中毒,但 await 上的
/// 死锁不受锁序保护);omp/远端 waiter 线程取锁必须用显式作用域限界。
/// 委托壳(P3-01 切片三):14 张容器表与 `RunRegistry` 已一次性 move 进
/// core 的 `aeroric_core::runtime::registry::TaskRegistry`——禁止双真相源,所以
/// 这里不是副本。上面这段锁序表随本体搬进了 core 的 registry.rs(此处保留原表
/// 文本作为宿主侧的对照锚);本壳只保留宿主专属的 `codex_rpc` 单例
/// (ADR-020 豁免它不是任务键)。
#[derive(Default)]
pub struct TaskManager {
    registry: aeroric_core::runtime::TaskRegistry,
    /// 任务作用域 watcher 的唯一入口(切片四):spawn 站点经它拿名字去重、token
    /// 与有界拆除。会话 watcher 按 (task, generation) 去重;reader/退出监控用
    /// `spawn_unique`(仅记账)。
    pub(crate) supervisor: Arc<aeroric_core::runtime::WatcherSupervisor>,
    /// 原始终端字节流的归属地(P3-03 切片二)。reader 先把 bytes 落到这里,
    /// 旧的前端字符串只是它的一个增量解码投影;控制租约(input/resize)也按
    /// `(task_id, generation)` 从这里分发。
    pub(crate) terminal: Arc<aeroric_core::terminal::TerminalService>,
    /// Persistent `codex app-server` process reused across `read_usage_snapshot` calls.
    pub(crate) codex_rpc: Arc<Mutex<Option<CodexRpcClient>>>,
}

impl std::ops::Deref for TaskManager {
    type Target = aeroric_core::runtime::TaskRegistry;

    fn deref(&self) -> &Self::Target {
        &self.registry
    }
}

impl std::ops::DerefMut for TaskManager {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.registry
    }
}

// `ExitSettlement` 随 P3-01 切片三 move 进 core(它是 TaskRegistry::settle_marks_for_exit
// 的返回类型)。re-export 保持 `crate::ExitSettlement` 路径不变。
pub(crate) use aeroric_core::runtime::registry::ExitSettlement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExitAction {
    Exit,
    Restart,
}

impl ExitAction {
    fn event_name(self) -> &'static str {
        match self {
            Self::Exit => "app-exit-requested",
            Self::Restart => "app-restart-requested",
        }
    }
}

#[derive(Default)]
struct ExitState {
    frontend_ready: bool,
    pending: Option<ExitAction>,
    authorized: Option<ExitAction>,
}

#[derive(Default)]
struct ExitAuthorization(Mutex<ExitState>);

impl ExitAuthorization {
    fn authorize(&self, action: ExitAction) {
        self.0.lock().authorized = Some(action);
    }

    fn take_authorization(&self, action: ExitAction) -> bool {
        let mut state = self.0.lock();
        if state.authorized == Some(action) {
            state.authorized = None;
            true
        } else {
            false
        }
    }

    /// Mark the frontend lifecycle listener as installed and return one request
    /// that arrived before the listener was ready.
    fn mark_frontend_ready(&self) -> Option<ExitAction> {
        let mut state = self.0.lock();
        state.frontend_ready = true;
        state.pending.take()
    }

    /// Queue one lifecycle request until the frontend is ready. When it is
    /// already ready, the caller should emit the event immediately.
    fn queue_or_emit(&self, action: ExitAction) -> bool {
        let mut state = self.0.lock();
        if state.frontend_ready {
            true
        } else {
            state.pending.get_or_insert(action);
            false
        }
    }

    fn requeue(&self, action: ExitAction) {
        let mut state = self.0.lock();
        if state.authorized.is_none() {
            state.pending.get_or_insert(action);
        }
    }
}

fn emit_lifecycle_request(app: &tauri::AppHandle, action: ExitAction) {
    if let Err(error) = app.emit(action.event_name(), ()) {
        app.state::<ExitAuthorization>().requeue(action);
        eprintln!("Failed to notify frontend about {:?}: {error}", action);
    }
}

fn request_lifecycle_action(app: &tauri::AppHandle, action: ExitAction) {
    if app.state::<ExitAuthorization>().queue_or_emit(action) {
        emit_lifecycle_request(app, action);
    }
}

/// Allow an internal operation that has already completed its frontend save
/// handshake to finish an ordinary `app.exit` request.
pub(crate) fn authorize_app_exit(app: &tauri::AppHandle) {
    app.state::<ExitAuthorization>().authorize(ExitAction::Exit);
}

pub(crate) fn authorize_app_restart(app: &tauri::AppHandle) {
    app.state::<ExitAuthorization>()
        .authorize(ExitAction::Restart);
}

// `impl TaskManager` 的全部方法(resource_run/writer_for/settle_marks_for_exit/release_run_sessions
// 等)随 P3-01 切片三 move 进 core 的 `TaskRegistry`;`ExitSettlement` 也是。TaskManager
// 现在 Deref 到它,全部调用点经解引用不变。

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

#[tauri::command]
fn exit_app_after_task_flush(app: tauri::AppHandle) {
    authorize_app_exit(&app);
    app.exit(0);
}

#[tauri::command]
fn restart_app_after_task_flush(app: tauri::AppHandle) {
    authorize_app_restart(&app);
    app.request_restart();
}

#[tauri::command]
fn app_exit_listener_ready(app: tauri::AppHandle) {
    if let Some(action) = app.state::<ExitAuthorization>().mark_frontend_ready() {
        emit_lifecycle_request(&app, action);
    }
}

/// `main` 的前置分支:ssh 以 `--ssh-proxy-bridge` 拉起本程序时进入代理桥模式。
pub fn try_run_ssh_proxy_bridge() -> bool {
    ssh_proxy::try_run_ssh_proxy_bridge()
}

/// `main` 的前置分支:权限面板以 `--probe-system-permissions` 拉起本程序,借一个**新
/// 进程**问出系统当前记的授权(macOS 把 TCC 判定缓存在进程里,本进程问不到)。
/// 必须在 Tauri 启动之前处理:探测进程不该开窗口,也不该初始化任何应用状态。
pub fn try_run_permission_probe_bridge() -> bool {
    permissions::try_run_probe_bridge()
}

/// 应用壳构建失败时把原因落到用户看得见的地方。
///
/// GUI 进程的 stderr 在双击启动时无处可见,所以三条路一起走:stderr(命令行启动时
/// 有用)、`~/.aeroric/startup-error.log`(可让用户捞给我们;数据目录不可写时静默
/// 跳过,反正那也常是失败本因)、以及原生弹窗(唯一能当场看到的)。
fn report_fatal_startup_error(message: &str) {
    eprintln!("[aeroric] fatal: cannot start application: {message}");

    if let Ok(dir) = storage::aeroric_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("startup-error.log"),
            format!("cannot start application: {message}\n"),
        );
    }

    #[cfg(target_os = "macos")]
    {
        // osascript 是系统自带的,不引入依赖,且此刻 Tauri 的对话框插件还不可用。
        // 单引号在 AppleScript 字符串里需要转义,否则弹窗会因语法错误而不显示。
        let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "display dialog \"Aeroric cannot start:\n\n{escaped}\" with title \"Aeroric\" buttons {{\"OK\"}} with icon stop"
        );
        let _ = std::process::Command::new("osascript")
            .arg("-e")
            .arg(script)
            .status();
    }
    #[cfg(windows)]
    {
        // mshta 在所有受支持的 Windows 上都在,不用引 winapi。
        let escaped = message.replace(['\'', '\r', '\n'], " ");
        let script = format!("javascript:alert('Aeroric cannot start: {escaped}');close()");
        let _ = std::process::Command::new("mshta").arg(script).status();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // 桌面环境不一定装了哪个,依次试,都没有就只剩 stderr 与日志文件。
        for (program, args) in [
            (
                "zenity",
                vec![
                    "--error".to_string(),
                    format!("--text=Aeroric cannot start: {message}"),
                ],
            ),
            (
                "kdialog",
                vec![
                    "--error".to_string(),
                    format!("Aeroric cannot start: {message}"),
                ],
            ),
        ] {
            if std::process::Command::new(program)
                .args(&args)
                .status()
                .is_ok()
            {
                break;
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if notification::try_run_update_helper() {
        return;
    }
    let app = tauri::Builder::default()
        .setup(|app| {
            // 不再 `.expect()`:构造已改为逐级降级(见 `DbxState::new_blocking`),
            // 磁盘不可用时退到临时目录或内存库,原因记进启动诊断由前端横幅告知。
            app.manage(crate::database::dbx_state::DbxState::new_blocking());
            // P2-05：原生 core 的共享状态进 managed state。macOS 上这一步会真的去取写者租约
            // （ADR-021），所以第二实例从此以只读启动并如实告知；但**这不是写权移交**——任务表
            // 仍由 storage.rs 保存，没有任何 mutation 走 TaskService。失败不阻塞启动：
            // 装载不上就记一条启动降级，旧客户端照常跑。
            let core_owns_business_plane = crate::core_adapter::install_core_services(
                app.handle(),
                &app.package_info().version.to_string(),
            );
            // 后台预热 login shell 环境，避免第一次启动任务时阻塞
            std::thread::spawn(|| {
                crate::app_settings::get_login_shell_path();
            });
            // 安装 hook 脚本与用户级配置注入(失败不阻塞启动,前端可查询状态)。
            // 结果写入缓存,供 run_task/resume_task 的 hook 信任检查零阻塞读取。
            std::thread::spawn(|| {
                crate::hooks::cache_status(crate::hooks::ensure_installed());
            });
            // 压实历次运行遗留的超标终端历史。放后台线程:这是 O(超标文件总量) 的
            // 读+重写,不能压在启动路径上。见 compact_oversized_terminal_histories。
            std::thread::spawn(|| {
                let _ = crate::storage::compact_oversized_terminal_histories();
            });
            // P8-05 Action 2 的互斥：业务面外置（已经有别的进程当宿主）时，这些后台服务
            // **一个都不起**——两套 watcher 会双发事件、两套用量索引会同时写统计库、
            // 两个 remote server 会抢同一个端口。判据是 `CorePlacement`，不是"租约拿没拿到"：
            // 只有前者能表达"我本就不该是写者"。
            if core_owns_business_plane {
                // 启动 hook 事件文件 watcher
                crate::event_watcher::start(app.handle().clone());
                crate::usage_index::start(app.handle().clone());
                crate::notebook::sync::daemon::start(app.handle().clone());
                crate::local_router_commands::init(app.handle());
                // 手机远程连接:按持久化配置决定是否自动拉起 WS 服务
                crate::remote::install_terminal_history_resolver();
                crate::remote::init(app.handle().clone());
            } else {
                crate::startup_diagnostics::record(
                    "host-attached",
                    "本进程未拥有业务面：watcher/用量索引/笔记调度/local router/remote 由外部宿主负责"
                        .to_string(),
                    app.handle()
                        .state::<crate::core_adapter::CoreGateway>()
                        .host_status()
                        .unwrap_or("external host")
                        .to_string(),
                );
                // 终端历史 resolver 是纯读侧（远程回放要用），外置时仍然装。
                crate::remote::install_terminal_history_resolver();
            }
            // P8-03 刀二：setup 完成，remote task.create/resume 的 ready 闸放行
            // （原 React startupReadyRef 语义的宿主化）。
            app.state::<std::sync::Arc<remote::RemoteState>>()
                .mark_ready();
            Ok(())
        })
        .manage(TaskManager::default())
        .manage(std::sync::Arc::new(platform_adapter::HostPlatformServices)
            as std::sync::Arc<dyn aeroric_core::runtime::PlatformServices>)
        .manage(run_config::RunConfigState::default())
        .manage(dap::DebugState::default())
        .manage(std::sync::Arc::new(remote::RemoteState::new()))
        .manage(local_router_commands::LocalRouterManager::for_app())
        .manage(dsh_webui::DshWebUiManager::new())
        .manage(std::sync::Arc::new(
            notebook::state::NotebookState::default(),
        ))
        .manage(ExitAuthorization::default())
        .on_window_event(|window, event| {
            // macOS: 点关闭按钮(红灯)时隐藏窗口而非退出,与 Cmd+W 行为一致;
            // 点 Dock 图标可唤回(见下方 Reopen 处理)。
            #[cfg(target_os = "macos")]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                hide_window_to_dock(window.clone());
                api.prevent_close();
            }
            #[cfg(not(target_os = "macos"))]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Window close events can arrive before the webview has finished
                // registering its listener. Route them through the same
                // replayable lifecycle handshake as menu/application exits.
                api.prevent_close();
                request_lifecycle_action(window.app_handle(), ExitAction::Exit);
            }
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            hide_main_window,
            exit_app_after_task_flush,
            restart_app_after_task_flush,
            app_exit_listener_ready,
            pty::run_task,
            pty::resume_task,
            pty::cancel_task,
            pty::complete_task,
            pty::get_active_task_ids,
            pty::validate_agent_launch,
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
            storage_conn::storage_list_connections,
            storage_conn::storage_secret_keys,
            storage_conn::storage_save_connection,
            storage_conn::storage_delete_connection,
            storage_conn::storage_touch_connection,
            storage_oauth::storage_oauth_authorize,
            storage_oauth::storage_oauth_credential_options,
            sftp::storage_protocols,
            sftp::storage_capabilities,
            sftp::storage_test_connection,
            sftp::storage_unmount_connection,
            git::generate_commit_message,
            agent_assist::generate_task_name,
            report::generate_weekly_report,
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
            git::git_init,
            git::git_clone,
            git::git_fetch,
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
            analytics::read_usage_statistics,
            usage_index::refresh_usage_statistics_index,
            session::read_session_messages,
            session::read_session_message_page,
            session::read_session_id,
            session::session_supports_native_resume,
            session::recover_task_session,
            session::adopt_session_for_agent,
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
            storage::read_task_terminal_history,
            storage::delete_task_terminal_histories,
            remote::remote_server_status,
            remote::remote_server_start,
            remote::remote_server_stop,
            remote::remote_select_lan_ip,
            remote::remote_update_config,
            remote::remote_create_invite,
            remote::remote_list_devices,
            remote::remote_revoke_device,
            remote::remote_complete_task_request,
            ssh::load_ssh_connections,
            ssh::get_ssh_connection_password,
            ssh::save_ssh_connections,
            ssh::delete_ssh_connection,
            ssh::open_ssh_shell,
            ssh::kill_ssh_shell,
            ssh::run_remote_task,
            ssh::resume_remote_task,
            ssh::cancel_remote_task,
            ssh_hostkey::check_ssh_host_key,
            ssh_hostkey::trust_ssh_host_key,
            wsl::get_wsl_status,
            wsl::list_wsl_distributions,
            wsl::probe_wsl_distribution,
            wsl::read_wsl_environment,
            wsl::load_wsl_settings,
            wsl::save_wsl_settings,
            wsl::read_wsl_config_file,
            wsl::write_wsl_config_file,
            wsl::get_wsl_agent_status,
            wsl::upgrade_wsl_agent_versions,
            wsl::read_wsl_agent_config,
            wsl::write_wsl_agent_config,
            wsl::restart_wsl,
            wsl::validate_wsl_project_path,
            wsl::open_wsl_shell,
            wsl::kill_wsl_shell,
            wsl::run_wsl_task,
            wsl::resume_wsl_task,
            wsl::cancel_wsl_task,
            config::read_wsl_project_config,
            config::write_wsl_project_config,
            wsl_fs::wsl_read_dir_entries,
            wsl_fs::wsl_read_file_content,
            wsl_fs::wsl_read_image_preview,
            wsl_fs::wsl_write_file_content,
            wsl_fs::wsl_create_file,
            wsl_fs::wsl_create_directory,
            wsl_fs::wsl_delete_path,
            wsl_fs::wsl_rename_path,
            wsl_fs::wsl_copy_paths_to_directory,
            wsl_git::wsl_git_status,
            wsl_git::wsl_git_changes,
            wsl_git::wsl_git_list_branches,
            wsl_git::wsl_git_log,
            wsl_git::wsl_git_commit_detail,
            wsl_git::wsl_git_remote_counts,
            wsl_git::wsl_git_show_diff,
            wsl_git::wsl_git_show_commit_diff,
            wsl_git::wsl_git_show_file_diff,
            wsl_git::wsl_git_file_diff,
            wsl_git::wsl_git_stage,
            wsl_git::wsl_git_unstage,
            wsl_git::wsl_git_stage_files,
            wsl_git::wsl_git_unstage_files,
            wsl_git::wsl_git_stage_all,
            wsl_git::wsl_git_unstage_all,
            wsl_git::wsl_git_commit,
            wsl_git::wsl_git_discard_file,
            wsl_git::wsl_git_discard_files,
            wsl_git::wsl_git_discard_all,
            wsl_git::wsl_git_push,
            wsl_git::wsl_git_pull,
            wsl_git::wsl_git_blame_file,
            wsl_git::wsl_git_branch_graph,
            wsl_git::wsl_git_stash_list,
            wsl_git::wsl_git_stash_diff,
            wsl_git::wsl_git_stash_push,
            wsl_git::wsl_git_stash_apply,
            wsl_git::wsl_git_stash_drop,
            wsl_git::wsl_git_conflict_files,
            wsl_git::wsl_git_conflict_preview,
            wsl_git::wsl_git_resolve_conflict,
            app_settings::load_app_settings,
            app_settings::save_app_settings,
            agent_usage::record_agent_config_usage,
            agent_usage::load_agent_usage_snapshot,
            app_settings::update_proxy_settings,
            app_settings::update_notebook_embedding_settings,
            app_settings::test_proxy_connection,
            app_settings::update_agent_path_settings,
            app_settings::update_builtin_agent_access,
            app_settings::update_dsh_reasoning_effort,
            app_settings::update_custom_agent_access,
            local_router_commands::get_local_router_status,
            local_router_commands::set_local_router_enabled,
            local_router_commands::update_local_router_settings,
            local_router_commands::switch_local_router_target,
            local_router_commands::reset_local_router_circuit,
            local_router_commands::get_local_router_requests,
            app_settings::export_agent_config_bundle,
            app_settings::export_all_agent_config_bundle,
            app_settings::import_agent_config_bundle,
            app_settings::import_all_agent_config_bundle,
            app_settings::import_cc_switch_config,
            app_settings::save_agent_paths,
            app_settings::save_custom_agent_profile,
            app_settings::setup_agent_profile,
            app_settings::detect_agent_models,
            app_settings::list_agent_models,
            app_settings::update_custom_agent_models,
            app_settings::update_custom_agent_context,
            app_settings::update_custom_agent_artifact_tool,
            app_settings::update_custom_agent_chat_completions_proxy,
            app_settings::probe_chat_bridge_python,
            app_settings::delete_custom_agent_profile,
            app_settings::rename_custom_agent_profile,
            app_settings::save_send_shortcut,
            app_settings::save_shift_enter_newline,
            app_settings::update_auto_cleanup_settings,
            app_settings::update_weekly_report_settings,
            dsh_plugins::list_dsh_plugins,
            dsh_plugins::install_dsh_plugin,
            dsh_plugins::uninstall_dsh_plugin,
            dsh_plugins::toggle_dsh_plugin,
            dsh_plugins::get_dsh_settings_snapshot,
            dsh_plugins::open_dsh_config_file,
            dsh_plugins::save_dsh_plugin_settings,
            dsh_plugins::set_dsh_default_preset,
            dsh_webui::start_dsh_webui,
            dsh_webui::stop_dsh_webui,
            dsh_webui::get_dsh_webui_status,
            dsh_webui::get_dsh_protocol_capabilities,
            dsh_webui::invoke_dsh_remote,
            dsh_webui::run_dsh_task,
            dsh_webui::prompt_dsh_task,
            dsh_webui::cancel_dsh_task,
            dsh_webui::complete_dsh_task,
            omp_home::update_omp_thinking_level,
            omp_home::get_omp_thinking_level,
            dsh_webui::list_dsh_commands,
            dsh_webui::execute_dsh_command,
            dsh_webui::list_dsh_message_feedback,
            dsh_webui::put_dsh_message_feedback,
            dsh_webui::delete_dsh_message_feedback,
            dsh_webui::list_dsh_agent_presets,
            dsh_webui::list_dsh_agent_preset_details,
            dsh_webui::set_dsh_web_default_preset,
            // Session extended
            dsh_webui::list_dsh_sessions,
            dsh_webui::get_dsh_session_history,
            dsh_webui::rename_dsh_session,
            dsh_webui::fork_dsh_session,
            dsh_webui::search_dsh_sessions,
            dsh_webui::update_dsh_session_queue,
            // Workspace
            dsh_webui::list_dsh_workspaces,
            dsh_webui::create_dsh_workspace,
            dsh_webui::rename_dsh_workspace,
            dsh_webui::delete_dsh_workspace,
            dsh_webui::reorder_dsh_workspaces,
            dsh_webui::move_dsh_session_in_workspace,
            dsh_webui::archive_dsh_session,
            // Credentials
            dsh_webui::describe_dsh_credentials,
            dsh_webui::set_dsh_credential,
            dsh_webui::unset_dsh_credential,
            // LLM
            dsh_webui::list_dsh_llm_providers,
            dsh_webui::list_dsh_llm_models,
            dsh_webui::discover_dsh_llm_models,
            // Subagent
            dsh_webui::list_dsh_subagents,
            dsh_webui::get_dsh_subagent_history,
            dsh_webui::prompt_dsh_subagent,
            dsh_webui::interrupt_dsh_subagent,
            // Goals
            dsh_webui::create_dsh_goal,
            dsh_webui::edit_dsh_goal,
            dsh_webui::pause_dsh_goal,
            dsh_webui::resume_dsh_goal,
            dsh_webui::complete_dsh_goal,
            dsh_webui::clear_dsh_goals,
            // Skills
            dsh_webui::list_dsh_skills,
            // Host
            dsh_webui::describe_dsh_host,
            dsh_webui::list_dsh_host_directory,
            dsh_webui::create_dsh_host_directory,
            dsh_webui::open_dsh_host_path,
            // AgentPreset extended
            dsh_webui::select_dsh_session_preset,
            dsh_webui::read_dsh_agent_preset,
            dsh_webui::copy_dsh_agent_preset,
            dsh_webui::open_dsh_agent_preset_document,
            dsh_webui::remove_dsh_agent_preset,
            // Settings extended
            dsh_webui::open_dsh_settings_document,
            dsh_webui::replace_dsh_settings,
            dsh_webui::mutate_dsh_settings,
            dsh_webui::update_dsh_settings,
            // Approval / Question responses + attachment
            dsh_webui::respond_dsh_remote_event,
            dsh_webui::get_dsh_session_attachment,
            // host-wide remote.mux subscription
            dsh_webui::start_dsh_host_events,
            dsh_webui::stop_dsh_host_events,
            // host.pickDirectory
            dsh_webui::pick_dsh_host_directory,
            // session.export (会话日志 ZIP)
            dsh_webui::export_dsh_session_log,
            app_settings::detect_agent_paths,
            app_settings::detect_agent_versions_for_settings,
            app_settings::detect_agent_version,
            app_settings::upgrade_agent_versions,
            agent_tools::get_agent_tool_status,
            agent_tools::get_agent_latest_versions,
            agent_tools::install_agent_tools,
            agent_tools::cancel_agent_tool_install,
            agent_ops::start_agent_operation,
            agent_ops::get_agent_operations,
            agent_ops::cancel_agent_operation,
            node_runtime::install_nodejs_on_windows,
            app_settings::get_system_fonts,
            platform::get_platform_runtime_info,
            platform::build_runnable_file_command,
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
            database::query::dbx_assess_production_sql,
            database::query::dbx_assess_production_target,
            database::query::respond_dbx_production_confirmation,
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
            permissions::list_system_permissions,
            permissions::request_system_permission,
            permissions::request_all_system_permissions,
            permissions::open_system_permission_settings,
            permissions::reset_system_permission,
            permissions::restart_app_for_permissions,
            startup_diagnostics::list_startup_degradations,
            core_adapter::apply_task_intent,
            usage::read_usage_snapshot,
            hooks::get_hook_status,
            hooks::get_hook_readiness,
            hooks::install_hooks,
            hooks::uninstall_hooks,
            skills::get_skill_hub_config,
            skills::set_skill_hub_path,
            skills::clear_skill_hub,
            skills::list_skills,
            skills::list_project_skills,
            skills::list_skill_installations,
            skills::install_skill,
            skills::uninstall_skill,
            skills::cleanup_installations_for_project,
            skills::delete_skill,
            skills::import_local_skill,
            skills::search_marketplace_skills,
            skills::get_marketplace_skill_details,
            skills::install_marketplace_skill,
            mcp::get_mcp_settings,
            mcp::set_mcp_settings,
            mcp::test_mcp_server,
            notebook::notebook_register_vault,
            notebook::notebook_unregister_vault,
            notebook::notebook_list_vaults,
            notebook::notebook_ensure_default_vault,
            notebook::notebook_ensure_project_vault,
            notebook::notebook_read_tree,
            notebook::notebook_read_order,
            notebook::notebook_write_order,
            notebook::notebook_read_icons,
            notebook::notebook_write_icons,
            notebook::notebook_open_note,
            notebook::notebook_peek_note,
            notebook::notebook_close_note,
            notebook::notebook_save_note,
            notebook::notebook_create_note,
            notebook::notebook_create_note_in_vault,
            notebook::notebook_rename_to_title,
            notebook::notebook_create_folder,
            notebook::notebook_delete_note,
            notebook::notebook_trash_list,
            notebook::notebook_trash_restore,
            notebook::notebook_trash_purge,
            notebook::notebook_trash_purge_all,
            notebook::notebook_rename_note,
            notebook::notebook_attachment_save,
            notebook::notebook_attachment_save_from_path,
            notebook::notebook_attachment_list,
            notebook::notebook_attachment_read,
            notebook::notebook_note_stat,
            notebook::notebook_vault_index,
            notebook::notebook_vault_links,
            notebook::notebook_vault_tags,
            notebook::notebook_vault_fields,
            notebook::notebook_vault_tasks,
            notebook::notebook_vault_mentions,
            notebook::notebook_link_mentions,
            notebook::notebook_rename_tag,
            notebook::notebook_list_snapshots,
            notebook::notebook_read_snapshot,
            notebook::notebook_restore_snapshot,
            notebook::notebook_migrate_legacy,
            notebook::notebook_convert_richtext,
            notebook::notebook_import_obsidian,
            notebook::notebook_import_logseq,
            notebook::notebook_import_notion,
            notebook::notebook_import_bear,
            notebook::notebook_import_roam,
            notebook::notebook_import_evernote,
            notebook::notebook_import_apple_notes,
            custom_themes::theme_custom_list,
            custom_themes::theme_custom_import,
            custom_themes::theme_custom_read,
            custom_themes::theme_custom_delete,
            custom_themes::theme_custom_dir,
            notebook::notebook_export_site_write,
            notebook::notebook_export_write_file,
            notebook::notebook_html_to_markdown,
            notebook::notebook_git_sync,
            notebook::notebook_sync_bind,
            notebook::notebook_sync_unbind,
            notebook::notebook_sync_remotes,
            notebook::notebook_sync_run,
            notebook::notebook_sync_set_auto,
            notebook::notebook_sync_status,
            notebook::notebook_sync_resolve,
            notebook::notebook_sync_resolutions,
            notebook::notebook_list_user_templates,
            notebook::rag::commands::notebook_rag_probe,
            notebook::rag::commands::notebook_rag_stats,
            notebook::rag::commands::notebook_rag_index,
            notebook::rag::commands::notebook_rag_cancel,
            notebook::rag::commands::notebook_rag_clear,
            notebook::rag::commands::notebook_rag_search,
            notebook::rag::commands::notebook_rag_context,
            notebook::rag::commands::notebook_rag_neighbors,
            notebook::rag::commands::notebook_embedding_key_status,
            notebook::rag::commands::notebook_embedding_key_set,
            notebook::rag::commands::notebook_embedding_key_clear,
        ])
        .build(tauri::generate_context!());

    // 这一层没有降级空间:应用壳都没建起来,谈不上"少个功能照样用"。
    // 但原先的 `.expect()` 只会 panic——GUI 进程的 stderr 通常没人看得到,
    // 用户体验是图标闪一下就消失。改成写日志 + 原生弹窗,让失败至少可见可报。
    let app = match app {
        Ok(app) => app,
        Err(error) => {
            report_fatal_startup_error(&error.to_string());
            std::process::exit(1);
        }
    };

    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { code, api, .. } = &event {
            let action = if *code == Some(tauri::RESTART_EXIT_CODE) {
                ExitAction::Restart
            } else {
                ExitAction::Exit
            };
            let authorization = app_handle.state::<ExitAuthorization>();
            if !authorization.take_authorization(action) {
                api.prevent_exit();
                request_lifecycle_action(app_handle, action);
            }
        }
        if let tauri::RunEvent::Exit = event {
            // G03 的 event_watcher 站点＋Action 2 的收口（P3-04 第七片，补记二百四十七）：
            // 监督者统一翻 token 并有界 join（默认 5s 宽限；阻塞在 syscall 的 PTY reader
            // 类循环被 detach，不把拆除挂死）。放在 LocalRouter/DSH 之前——watcher 线程
            // 还可能写事件/发 emit，先静默再关服务。usage_index 与 notebook daemon 两站
            // 仍各自裸线程（G03 具名归属 P8-04／P7-06）。
            let supervisor = Arc::clone(&app_handle.state::<TaskManager>().supervisor);
            let report = supervisor.shutdown();
            if report.detached > 0 {
                eprintln!(
                    "[aeroric] watcher shutdown detached {} thread(s) past grace",
                    report.detached
                );
            }
            let manager = app_handle.state::<local_router_commands::LocalRouterManager>();
            tauri::async_runtime::block_on(manager.shutdown());
            let webui_manager = app_handle.state::<dsh_webui::DshWebUiManager>();
            tauri::async_runtime::block_on(webui_manager.shutdown_all());
        }
        // macOS: 当窗口被 Cmd+W 隐藏（hide）后，点击 Dock 图标会触发 Reopen，
        // 此时没有可见窗口，需要手动把主窗口重新显示并聚焦。
        #[cfg(target_os = "macos")]
        {
            use tauri::Manager;
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        }
    });
}

#[cfg(test)]
mod run_settlement_tests {
    //! ADR-030 in executable form: the marks are one task-scoped account and only the last
    //! live run consumes them.
    use crate::run_registry::RunKey;
    use crate::{ExitSettlement, TaskManager};

    fn manager() -> TaskManager {
        TaskManager::default()
    }

    #[test]
    fn a_cancel_mark_waits_for_the_last_live_run_to_exit() {
        let tm = manager();
        tm.cancelled_tasks.lock().insert("t1".to_string());
        tm.runs.register("t1", 1);
        tm.runs.register("t1", 2);
        // Run 1 exits: it has retired itself, run 2 is still in flight.
        assert!(tm.runs.retire("t1", 1));
        assert_eq!(
            tm.settle_marks_for_exit("t1"),
            ExitSettlement::WaitingForOtherRuns,
            "the first exit must not consume the user's cancellation"
        );
        assert!(
            tm.cancelled_tasks.lock().contains("t1"),
            "waiting must leave the account untouched"
        );
        // Run 2 exits last and owns the settlement.
        assert!(tm.runs.retire("t1", 2));
        assert_eq!(
            tm.settle_marks_for_exit("t1"),
            ExitSettlement::Settle {
                cancelled: true,
                manually_completed: false
            }
        );
        assert!(!tm.cancelled_tasks.lock().contains("t1"), "consumed once");
    }

    #[test]
    fn a_task_with_a_single_run_settles_exactly_as_it_did_before() {
        // The no-double-open case is the shipping client's everyday path; ADR-030 must not
        // change what it observes.
        let tm = manager();
        tm.manually_completed_tasks.lock().insert("t1".to_string());
        tm.runs.register("t1", RunKey::UNMINTED);
        assert!(tm.runs.retire("t1", RunKey::UNMINTED));
        assert_eq!(
            tm.settle_marks_for_exit("t1"),
            ExitSettlement::Settle {
                cancelled: false,
                manually_completed: true
            }
        );
    }

    #[test]
    fn a_shared_transcript_path_stays_claimed_until_the_last_reader_is_gone() {
        // ADR-031 in one scenario: two runs read the same jsonl, the first one to leave must not
        // put the path back on the market — that is precisely the "two agents write one
        // transcript" accident ADR-020 kept the claim path-keyed to prevent.
        let tm = manager();
        let first = RunKey::minted("t1", 1);
        let second = RunKey::minted("t1", 2);
        let path = "/tmp/x.jsonl".to_string();
        for run in [&first, &second] {
            tm.claude_sessions.lock().insert(
                run.clone(),
                crate::session::ClaudeSessionInfo {
                    session_id: "s-1".to_string(),
                    session_path: path.clone(),
                    is_placeholder: false,
                },
            );
        }
        tm.claimed_session_paths.lock().insert(path.clone());

        let released = tm.release_run_sessions(&first);
        assert_eq!(
            released,
            vec![path.clone()],
            "the exit learns which path it held"
        );
        assert!(
            tm.claimed_session_paths.lock().contains(&path),
            "a sibling still reads it, so the claim must stay"
        );

        tm.release_run_sessions(&second);
        assert!(
            !tm.claimed_session_paths.lock().contains(&path),
            "the last reader is what frees it"
        );
    }

    #[test]
    fn releasing_a_run_touches_only_that_run() {
        let tm = manager();
        let mine = RunKey::minted("t1", 3);
        let sibling = RunKey::minted("t1", 4);
        tm.codex_sessions.lock().insert(
            sibling.clone(),
            crate::session::CodexSessionInfo {
                session_id: "keep".to_string(),
                session_path: "/tmp/keep.jsonl".to_string(),
            },
        );
        tm.claimed_session_paths
            .lock()
            .insert("/tmp/keep.jsonl".to_string());
        assert!(tm.release_run_sessions(&mine).is_empty());
        assert!(
            tm.has_codex_session("t1"),
            "an exit for one generation cannot unregister another one's session"
        );
        assert!(
            !tm.release_session_path_if_unreferenced("/tmp/keep.jsonl"),
            "still referenced by the sibling entry, so nothing may be released"
        );
    }

    #[test]
    fn another_tasks_run_does_not_hold_this_one_back() {
        let tm = manager();
        tm.runs.register("t1", 1);
        tm.runs.register("t2", 7);
        assert!(tm.runs.retire("t1", 1));
        assert_eq!(
            tm.settle_marks_for_exit("t1"),
            ExitSettlement::Settle {
                cancelled: false,
                manually_completed: false
            },
            "a sibling task's live run is not this task's live run"
        );
        assert!(tm.runs.is_live("t2", 7), "and it is untouched by t1's exit");
    }
}
