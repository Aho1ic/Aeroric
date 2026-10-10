//! 文件服务的**第二批**宿主方法（Task P8-05 切片七）：格式化、本地历史、路径复制。
//!
//! 与第一批七条非 search 方法一样，项目根由 `projectId` 对应的宿主记录决定。
//! history/copy 使用相对路径；formatter 保留既有的绝对文件路径接口，不在这里悄悄改形状。
//! 显式 target 必须先解码；未服务的 SSH/WSL 拒绝，Local 只可确认同一 canonical 项目根，
//! 不能替代 projectId 选另一个根。缺省 target 保留既有本地分支。
//!
//! 业务在 core 的 `files::format`/`files::local_history`/`files::read_write`。
//! 宿主负责参数/target/当前写权准入、权威根解析和结果帧，不复制业务算法，
//! 也不把这些 projectId 协议冒充为 legacy 绝对路径命令的等价别名。

use std::sync::Arc;

use aeroric_core::context::CoreContext;
use aeroric_core::files::{format as file_format, local_history as history};
use aeroric_protocol::envelope::{Frame, Request};
use aeroric_protocol::error::ErrorCode;
use serde_json::Value;

use crate::dispatcher::{failure, ServiceSet};
use crate::file_access::{require_local_record, write_refusal, FileProjectError, FileTarget};
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
    fn resolve(
        &self,
        context: &Arc<CoreContext>,
        project_id: &str,
    ) -> Result<String, FileProjectError>;
}

/// 生产实现：先检查记录的 location，再取原 `path`；不能把远端的显示路径当本地根。
pub struct RecordRootResolver;

impl ProjectRootResolver for RecordRootResolver {
    fn resolve(
        &self,
        context: &Arc<CoreContext>,
        project_id: &str,
    ) -> Result<String, FileProjectError> {
        let projects = aeroric_core::storage::load_projects(context)
            .map_err(|error| FileProjectError::NotFound(error.to_string()))?;
        for record in projects {
            if aeroric_core::projects::mutations::text(&record, "id") == Some(project_id) {
                require_local_record(&record)?;
                return aeroric_core::projects::mutations::text(&record, "path")
                    .map(str::to_string)
                    .ok_or_else(|| {
                        FileProjectError::NotFound(format!(
                            "Project record {project_id} has no path"
                        ))
                    });
            }
        }
        Err(FileProjectError::NotFound(format!(
            "Project not found: {project_id}"
        )))
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
        .map_err(|error| Box::new(error.into_frame(request)))?;
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
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    if let Some(frame) = write_refusal(&request, &context) {
        return frame;
    }
    let root_path = match project_root(&request, &context, &resolver) {
        Ok(root) => root,
        Err(frame) => return *frame,
    };
    if let Some(frame) = target.root_refusal(&request, &root_path) {
        return frame;
    }
    // Snapshot failure prevents spawning. An external formatter's nonzero exit
    // does not restore its writes; the snapshot remains available for recovery.
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
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    if matches!(op, HistoryOp::Restore) {
        if let Some(frame) = write_refusal(&request, &context) {
            return frame;
        }
    }
    let root_path = match project_root(&request, &context, &resolver) {
        Ok(root) => root,
        Err(frame) => return *frame,
    };
    if let Some(frame) = target.root_refusal(&request, &root_path) {
        return frame;
    }
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
/// core 对源使用 `resolve_within`，对可缺失的目标目录检查最近既存祖先，再逐项拒绝
/// 任意已存在的目标叶子（含悬空链接）。保留部分进度；这是路径时点检查，不是 TOCTOU 事务。
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
    let target = match FileTarget::decode(&request) {
        Ok(target) => target,
        Err(frame) => return *frame,
    };
    if let Some(frame) = write_refusal(&request, &context) {
        return frame;
    }
    let root_path = match project_root(&request, &context, &resolver) {
        Ok(root) => root,
        Err(frame) => return *frame,
    };
    if let Some(frame) = target.root_refusal(&request, &root_path) {
        return frame;
    }
    let result = aeroric_core::files::read_write::copy_project_paths_to_directory(
        &root_path,
        &target_dir,
        &sources,
    )
    .map(|copied| serde_json::json!({ "copied": copied }));
    value_frame(&request, result)
}
