//! 文件服务的**第二批**宿主方法（Task P8-05 切片七）：格式化、本地历史、路径复制。
//!
//! 与第一批（`register_file_services` 的九条）同一条约定：**客户端只传 `projectId` ＋
//! 相对路径**，项目根由宿主从项目记录里查（`find_project_root`）。这不是风格问题——
//! 让客户端直接传绝对路径等于把"能读能写哪些文件"的决定权交出去，而服务端的权威校验
//! （`files::path_policy`）本来就是按"项目根 ＋ 相对路径"设计的。
//!
//! 五个方法的实现全部落在**已经上收的 core**（`files::format`/`files::local_history`），
//! 这里只做三件事：把请求参数翻译成 core 的入参、把 `projectId` 解析成根、把结果包成帧。
//! 于是"同一个能力在 Tauri 与宿主上语义一致"这件事由**同一份实现**保证，而不是靠对齐两
//! 份代码。

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::files::{format as file_format, local_history as history};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::Value;

use crate::dispatcher::{failure, ServiceSet};
use crate::services::ok;

/// 注册第二批文件服务。
pub fn register_file_services_batch2(
    set: &mut ServiceSet,
    context: Arc<CoreContext>,
    root_resolver: Arc<dyn ProjectRootResolver>,
) {
    {
        let context = Arc::clone(&context);
        let resolver = Arc::clone(&root_resolver);
        set.register(
            "files.format_file",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let resolver = Arc::clone(&resolver);
                Box::pin(async move { files_format_file(request, context, resolver) })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let resolver = Arc::clone(&root_resolver);
        set.register(
            "files.list_local_history",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let resolver = Arc::clone(&resolver);
                Box::pin(
                    async move { files_local_history(request, context, resolver, HistoryOp::List) },
                )
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let resolver = Arc::clone(&root_resolver);
        set.register(
            "files.read_local_history_entry",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let resolver = Arc::clone(&resolver);
                Box::pin(
                    async move { files_local_history(request, context, resolver, HistoryOp::Read) },
                )
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let resolver = Arc::clone(&root_resolver);
        set.register(
            "files.restore_local_history_entry",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let resolver = Arc::clone(&resolver);
                Box::pin(async move {
                    files_local_history(request, context, resolver, HistoryOp::Restore)
                })
            }),
        );
    }
    {
        let context = Arc::clone(&context);
        let resolver = Arc::clone(&root_resolver);
        set.register(
            "files.copy_paths_to_directory",
            Arc::new(move |request: Request, _call| {
                let context = Arc::clone(&context);
                let resolver = Arc::clone(&resolver);
                Box::pin(async move { files_copy_paths(request, context, resolver) })
            }),
        );
    }
}

/// 项目 id → 项目根。抽成端口是为了让本模块的用例能在没有项目记录的临时目录上跑，
/// 也让宿主的两条路径（桌面 adapter 与 CLI）能各给各的实现。
pub trait ProjectRootResolver: Send + Sync {
    fn resolve(&self, context: &Arc<CoreContext>, project_id: &str) -> Result<String, String>;
}

/// 生产实现：扫项目记录找 `path`（与 `services.rs::find_project_root` 同款）。
pub struct RecordRootResolver;

impl ProjectRootResolver for RecordRootResolver {
    fn resolve(&self, context: &Arc<CoreContext>, project_id: &str) -> Result<String, String> {
        let projects = aeroric_core::storage::load_projects(context).map_err(|e| e.to_string())?;
        for record in projects {
            if aeroric_core::projects::mutations::text(&record, "id") == Some(project_id) {
                return aeroric_core::projects::mutations::text(&record, "path")
                    .map(str::to_string)
                    .ok_or_else(|| format!("Project record {project_id} has no path"));
            }
        }
        Err(format!("Project not found: {project_id}"))
    }
}

/// 项目根：**先 canonicalize 再用**。
///
/// 项目记录里存的是用户给的路径，而 macOS 上 `/tmp` 是 `/private/tmp` 的符号链接——
/// 不 canonicalize 的根与 canonicalize 过的文件比对前缀时会判成"文件在项目外"
/// （本模块的用例第一次跑就是这么红的）。core 的 `resolve_within` 与
/// `local_history::relative_file_path` 都假定调用方给的是同一个规范的根。
fn project_root(
    request: &Request,
    context: &Arc<CoreContext>,
    resolver: &Arc<dyn ProjectRootResolver>,
) -> Result<std::path::PathBuf, Box<Frame>> {
    let project_id = param(request, "projectId")?;
    let raw = resolver
        .resolve(context, &project_id)
        .map_err(|error| Box::new(failure(request.id.clone(), ErrorCode::NotFound, &error)))?;
    aeroric_core::files::path_policy::validate_project_root(&raw).map_err(|error| {
        Box::new(failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            &error,
        ))
    })
}

fn param(request: &Request, key: &str) -> Result<String, Box<Frame>> {
    request
        .params
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            Box::new(failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &format!("Missing param: {key}"),
            ))
        })
}

fn value_frame(request: &Request, result: Result<Value, String>) -> Frame {
    match result {
        Ok(value) => ok(request, value),
        Err(error) => failure(request.id.clone(), ErrorCode::InvalidArgument, &error),
    }
}

/// `files.format_file`：格式化前先记快照（core 的 `format_file` 自带这个钩子）。
fn files_format_file(
    request: Request,
    context: Arc<CoreContext>,
    resolver: Arc<dyn ProjectRootResolver>,
) -> Frame {
    let path = match param(&request, "path") {
        Ok(value) => value,
        Err(frame) => return *frame,
    };
    let root_path = match project_root(&request, &context, &resolver) {
        Ok(root) => root,
        Err(frame) => return *frame,
    };
    // 快照钩子：先记再格式化，失败即中止（与 Tauri 侧同序；core 的 `format_file` 保证
    // 非零退出时原文不变）。
    let snapshot = |absolute: &str| -> Result<(), String> {
        let file = std::path::PathBuf::from(absolute);
        history::force_snapshot(
            history::CODE_LAYOUT,
            &root_path,
            &file,
            history::now_ms_u64(),
        )
        .map(|_| ())
    };
    let result = file_format::format_file(&root_path, &path, Some(&snapshot))
        .and_then(|result| serde_json::to_value(result).map_err(|error| error.to_string()));
    value_frame(&request, result)
}

#[derive(Clone, Copy)]
enum HistoryOp {
    List,
    Read,
    Restore,
}

fn files_local_history(
    request: Request,
    context: Arc<CoreContext>,
    resolver: Arc<dyn ProjectRootResolver>,
    op: HistoryOp,
) -> Frame {
    let path = match param(&request, "path") {
        Ok(value) => value,
        Err(frame) => return *frame,
    };
    let root_path = match project_root(&request, &context, &resolver) {
        Ok(root) => root,
        Err(frame) => return *frame,
    };
    // 相对路径经过 core 的权威校验（拒 `..`/绝对路径/空段）。
    let file = match aeroric_core::files::path_policy::resolve_within(&root_path, &path) {
        Ok(file) => file,
        Err(error) => {
            return failure(
                request.id.clone(),
                ErrorCode::InvalidArgument,
                &error.to_string(),
            )
        }
    };
    // 三个操作返回**不同**的类型（列表 / 快照）⇒ 先在各自分支里序列化，再统一成 `Value`，
    // 否则 `match` 的各臂类型不一致（编译期就会红，这里留一行说明为什么是这个形状）。
    let result: Result<Value, String> = match op {
        HistoryOp::List => history::list_entries(history::CODE_LAYOUT, &root_path, &file)
            .and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string())),
        HistoryOp::Read => {
            let entry_id = match param(&request, "entryId") {
                Ok(value) => value,
                Err(frame) => return *frame,
            };
            history::read_entry(history::CODE_LAYOUT, &root_path, &file, &entry_id)
                .and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string()))
        }
        HistoryOp::Restore => {
            let entry_id = match param(&request, "entryId") {
                Ok(value) => value,
                Err(frame) => return *frame,
            };
            history::restore_entry(
                history::CODE_LAYOUT,
                &root_path,
                &file,
                &entry_id,
                history::now_ms_u64(),
            )
            .and_then(|value| serde_json::to_value(value).map_err(|e| e.to_string()))
        }
    };
    value_frame(&request, result)
}

/// `files.copy_paths_to_directory`：把项目内的若干相对路径复制到项目内的目标目录。
///
/// **两侧都过权威校验**（源与目标都 `resolve_within`），所以既不能借 `..` 读外面，
/// 也不能写到外面；目标目录不存在就建（`create_dir_all`），已存在的同名文件**拒绝**
/// 而不是覆盖——覆盖是不可撤回的，而"复制失败"用户可以重来。
fn files_copy_paths(
    request: Request,
    context: Arc<CoreContext>,
    resolver: Arc<dyn ProjectRootResolver>,
) -> Frame {
    let target_dir = match param(&request, "targetDir") {
        Ok(value) => value,
        Err(frame) => return *frame,
    };
    let sources: Vec<String> = request
        .params
        .get("paths")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if sources.is_empty() {
        return failure(
            request.id.clone(),
            ErrorCode::InvalidArgument,
            "Missing param: paths",
        );
    }
    let root_path = match project_root(&request, &context, &resolver) {
        Ok(root) => root,
        Err(frame) => return *frame,
    };
    // 目标目录可能还不存在 ⇒ 不能用 `resolve_within`（它要求目标已存在）。这里用
    // **相对路径校验 ＋ 根下拼接**：`..`/绝对路径在 `validate_relative_path` 就被拒，
    // 拼接结果按构造落根内，然后才 `create_dir_all`。
    if let Err(error) = aeroric_core::files::path_policy::validate_relative_path(&target_dir) {
        return failure(request.id.clone(), ErrorCode::InvalidArgument, &error);
    }
    let result = copy_into(&root_path, &root_path.join(&target_dir), &sources);
    value_frame(&request, result)
}

fn copy_into(
    root: &std::path::Path,
    target: &std::path::Path,
    sources: &[String],
) -> Result<Value, String> {
    std::fs::create_dir_all(target).map_err(|error| format!("Create target failed: {error}"))?;
    let mut copied = Vec::new();
    for source in sources {
        let from = aeroric_core::files::path_policy::resolve_within(root, source)
            .map_err(|error| error.to_string())?;
        let name = from
            .file_name()
            .ok_or_else(|| format!("Source has no file name: {source}"))?;
        let to = target.join(name);
        if to.exists() {
            return Err(format!(
                "Refusing to overwrite {}: the target already exists",
                to.display()
            ));
        }
        std::fs::copy(&from, &to).map_err(|error| format!("Copy failed: {error}"))?;
        copied.push(to.to_string_lossy().into_owned());
    }
    Ok(serde_json::json!({ "copied": copied }))
}
