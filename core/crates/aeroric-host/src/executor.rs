//! `CliPtyExecutor` —— 无头 host 的 PTY 执行体（P3-07 b1.1，补记二百九十一的
//! 装配清单）。
//!
//! 形状是「材料全在 core，这里只装配」：命令构造（`agents::launch` 四 plan）、
//! 会话装配（`terminal::executor::activate_pty_session`）、字节环
//! （`terminal::pty::TerminalService`——attach/events 的读点就是这里落的环）、
//! 句柄账（`runtime::registry::TaskRegistry`）、状态判定（`TaskService::observe`
//! 的 `TaskEvent::Exit`——**executor 不自己判状态**，单一 owner 不变）。
//!
//! 构造环（executor 要回报、service 要 executor）按补记二百九十一的定案解：
//! `service: Mutex<Weak<TaskService>>` 回链，serve 装配后 `attach`；回链断开时
//! 退出事件**不静默丢**——EventLog 仍发 `task-status`，载荷带
//! `serviceUnwired: true`，是「拒绝是明说的」同族。
//!
//! 与宿主 `pty::run_task_core` 的**具名差**（fake-agent 场景不需要、⑦ 评审
//! 对账）：hooks/mcp/managed_home 装配、login-shell env 与 guard ops、
//! session-evidence 的 settle 分支（`settle_exit` 的「会话算成功」半边）、
//! prompt 注入（inject_prompt_into_terminal 的打字机路径）、终端历史落盘。
//! dsh 家族在此返回 `Unsupported`（它的启动走 `dsh_host::executor`，无头宿主不接——
//! ⑦ 收尾定性；审批面＝core 瀑布经 `approval.list/respond`，已在 ⑤ 注册到 CLI host）。

use std::io::Read as _;
use std::sync::{Arc, Mutex, Weak};

use aeroric_core::agents::launch::{claude_plan, codex_plan, omp_plan, LaunchInput};
use aeroric_core::context::CoreContext;
use aeroric_core::events::EventLog;
use aeroric_core::runtime::manager::WatcherSupervisor;
use aeroric_core::runtime::registry::{RunKey, TaskRegistry};
use aeroric_core::tasks::executor::{
    CancelTarget, Execution, ExecutorFailure, LaunchRequest, LiveRuns, ResetOutcome, ResumeRequest,
    TaskEvent, TaskExecutor,
};
use aeroric_core::tasks::{ProtocolFamily, TaskService, WorkingDirectory};
use aeroric_core::terminal::executor::activate_pty_session;
use aeroric_core::terminal::pty::TerminalService;
use aeroric_core::terminal::stream::TerminalIdentity;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};

pub struct CliPtyExecutor {
    context: Arc<CoreContext>,
    registry: Arc<TaskRegistry>,
    terminal: Arc<TerminalService>,
    log: Arc<EventLog>,
    #[allow(dead_code)] // 任务托管在 b1.2 的 run 等待面接入；reader/exit 线程先直 spawn
    supervisor: Arc<WatcherSupervisor>,
    service: Mutex<Weak<TaskService>>,
    leases: Mutex<std::collections::HashMap<String, aeroric_core::terminal::lease::ControlLease>>,
}

impl CliPtyExecutor {
    pub fn new(
        context: Arc<CoreContext>,
        registry: Arc<TaskRegistry>,
        terminal: Arc<TerminalService>,
        log: Arc<EventLog>,
    ) -> Arc<Self> {
        Arc::new(Self {
            context,
            registry,
            terminal,
            log,
            supervisor: Arc::new(WatcherSupervisor::new()),
            service: Mutex::new(Weak::new()),
            leases: Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// serve 装配在 `TaskService::new` 之后回链（构造环的唯一解开点）。
    pub fn attach(&self, service: &Arc<TaskService>) {
        *self.service.lock().expect("lock") = Arc::downgrade(service);
    }

    /// 一次性输入：顶替式拿租约（宿主语义——`acquire` 就是顶替，前台优先）、
    /// 过写闸、写 PTY writer、释放。attach（⑤）长期持有时，CLI input 会顶掉
    /// 它——与宿主「前台窗口优先」同规则，票据机制保证过期 handle 的 release
    /// 是 no-op，不会把 attach 的控制权误关。
    pub fn input(&self, task_id: &str, bytes: &[u8]) -> Result<(), String> {
        use std::io::Write as _;
        let run = self
            .registry
            .resource_run(task_id)
            .ok_or_else(|| "the task has no live run on this host".to_string())?;
        let writer = self
            .registry
            .pty_writers
            .lock()
            .get(&run)
            .cloned()
            .ok_or_else(|| "the run has no writer (already retired?)".to_string())?;
        let mut leases = self.leases.lock().expect("lock");
        let lease = leases.entry(task_id.to_string()).or_default();
        let handle = lease.acquire();
        let written = lease
            .write_gate(&handle)
            .map_err(|e| format!("lease gate refused the write: {e:?}"))
            .and_then(|()| {
                let mut w = writer.lock();
                w.write_all(bytes).map_err(|e| e.to_string())?;
                w.flush().map_err(|e| e.to_string())
            });
        let _ = lease.release(&handle);
        written
    }

    /// 尺寸变更：与 input 同一条 lease 闸（前台窗口优先），过闸才碰 master。
    pub fn resize(&self, task_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let run = self
            .registry
            .resource_run(task_id)
            .ok_or_else(|| "the task has no live run on this host".to_string())?;
        let master = self
            .registry
            .master_for(task_id)
            .ok_or_else(|| "the run has no master (already retired?)".to_string())?;
        let mut leases = self.leases.lock().expect("lock");
        let lease = leases.entry(task_id.to_string()).or_default();
        let handle = lease.acquire();
        let resized = lease
            .write_gate(&handle)
            .map_err(|e| format!("lease gate refused the resize: {e:?}"))
            .and_then(|()| {
                master
                    .lock()
                    .resize(portable_pty::PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    })
                    .map_err(|e| e.to_string())
            });
        let _ = lease.release(&handle);
        let _ = run;
        resized
    }

    /// 杀一条任务的运行：**pty 前台组 killpg 优先**——`child.kill()` 只杀直接
    /// 子进程，脚本里继承 pty fd 的后代（`sleep`、node 包装层）会活着把
    /// `wait` 拖到自然退出（cancel 回归实测拖满 30 s）；`tcgetpgrp(master)`
    /// 给的是 pty 前台组，SIGKILL 整组，再补直接子进程 kill 兜底
    /// （组号取不到时至少杀到壳）。
    pub fn kill_run(&self, task_id: &str) {
        #[cfg(unix)]
        {
            if let Some(master) = self.registry.master_for(task_id) {
                let pgid = master.lock().process_group_leader();
                drop(master);
                if let Some(pgid) = pgid {
                    // SAFETY: pgid 来自 tcgetpgrp（>0 才是有效组）；killpg 对
                    // 已消失的组返回 ESRCH，无副作用。
                    unsafe {
                        libc::killpg(pgid, libc::SIGKILL);
                    }
                }
            }
        }
        if let Some(child) = self.registry.child_for(task_id) {
            let _ = child.lock().kill();
        }
    }

    /// serve 退出序的子进程策略（P3-06 Q86 具名移交、随本 Task serve 入口落地）：
    /// 优雅退出时**显式**杀掉全部在飞运行，不指望 master 关闭的隐式 SIGHUP——
    /// 无视 SIGHUP 的子进程（`trap '' HUP`）不会死。按 task_id 走 kill_run：
    /// killpg 定址最新代的前台组，旧代残余由进程退出关 master 的 SIGHUP 兜底；
    /// SIGKILL 强杀 serve 无此路径（OS 语义，子进程孤儿化）。
    pub fn kill_all_live(&self) {
        let ids: Vec<String> = {
            let handles = self.registry.child_handles.lock();
            handles.keys().map(|run| run.id().to_string()).collect()
        };
        for id in ids {
            self.kill_run(&id);
        }
    }

    /// agent → 程序与固定参数：settings.yaml 的 custom_agents 优先，内建回退
    /// 到「程序名＝agent id」（PATH 解析）。与宿主 launch_spec 的差具名在模块头。
    fn resolve_launch(&self, agent: &str) -> LaunchInput {
        let settings = aeroric_core::storage::load_settings(&self.context)
            .ok()
            .and_then(|outcome| outcome.found());
        if let Some(fields) = settings.as_ref() {
            if let Some(profile) = fields
                .get("custom_agents")
                .and_then(Value::as_array)
                .and_then(|list| {
                    list.iter()
                        .find(|profile| profile["id"].as_str() == Some(agent))
                })
            {
                let program = profile["path"]
                    .as_str()
                    .filter(|p| !p.trim().is_empty())
                    .unwrap_or(agent)
                    .to_string();
                let args = profile["args"]
                    .as_array()
                    .map(|args| {
                        args.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                return LaunchInput { program, args };
            }
        }
        LaunchInput::new(agent)
    }
}

fn to_command_builder(plan: aeroric_core::agents::launch::LaunchPlan) -> CommandBuilder {
    let mut c = CommandBuilder::new(&plan.program);
    c.args(&plan.args);
    if let Some(cwd) = &plan.cwd {
        c.cwd(cwd);
    }
    for (key, value) in &plan.env {
        c.env(key, value);
    }
    for key in &plan.unset_env {
        c.env_remove(key);
    }
    c
}

impl CliPtyExecutor {
    /// plan 之后的装配全链（launch 与 resume 共用：openpty→activate→reader→exit）。
    /// 步骤次序只此一份——分叉出第二份装配就是第二个真相。
    fn spawn_plan_core(
        &self,
        plan: aeroric_core::agents::launch::LaunchPlan,
        task_id: &str,
        project_id: &str,
        generation: u64,
        terminal_size: Option<(u16, u16)>,
        family: ProtocolFamily,
    ) -> Execution<()> {
        let command = to_command_builder(plan);
        let pty_system = native_pty_system();
        let (cols, rows) = terminal_size.unwrap_or((120, 30));
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| ExecutorFailure::Spawn {
                reason: format!("cannot open a pty: {e}"),
            })?;
        let run = RunKey::minted(task_id, generation);
        let handles = activate_pty_session(pair, command, &self.registry, &run).map_err(|e| {
            ExecutorFailure::Spawn {
                reason: e.to_string(),
            }
        })?;

        // reader：阻塞读进字节环。线程随进程退出消亡；supervisor 托管的
        // 取消面在 b1.2（run 等待 settled 时随 shutdown 一起收）。
        let terminal = Arc::clone(&self.terminal);
        let identity = TerminalIdentity::new(task_id.to_string(), generation);
        let mut reader = handles.reader;
        std::thread::spawn(move || {
            let mut buf = [0u8; 8 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        terminal.publish(&identity, &buf[..n]);
                    }
                }
            }
        });

        // exit：wait 到进程结束→经回链 observe（状态判定在 TaskService，
        // 不在这里）；回链断开时 EventLog 照发、载荷自证未接线。
        struct ExitCtx {
            registry: Arc<TaskRegistry>,
            log: Arc<EventLog>,
            service: Mutex<Weak<TaskService>>,
            family: ProtocolFamily,
        }
        let ctx = Arc::new(ExitCtx {
            registry: Arc::clone(&self.registry),
            log: Arc::clone(&self.log),
            service: Mutex::new(self.service.lock().expect("lock").clone()),
            family,
        });
        let task_id = task_id.to_string();
        let project_id = project_id.to_string();
        let child = handles.child;
        std::thread::spawn(move || {
            let code = match child.lock().wait() {
                Ok(status) => Some(status.exit_code() as i32),
                Err(_) => Some(-1),
            };
            // 退场先于上报（宿主 `run_task_core` 的 cleanup→status 顺序照抄）：
            // done 一旦可见，`has_live_run` 必须已是 false——反序会让观察者
            // 在窗口期把「已终结」误判成「还活着」（ADR-034 的存活闸读的就是
            // 这本账；Exit 事件本身是 Runtime 来源，不受闸约束，先 retire 安全）。
            let run = RunKey::minted(&task_id, generation);
            ctx.registry.remove_pty_handles(&run);
            ctx.registry.runs.retire(&task_id, generation);
            // 结算走宿主同一条判据链（settle_exit），标记在此消费：
            // - cancel 意图：宿主侧状态由前端先写（G05 的 React 权威面），CLI host
            //   没有前端——**executor 是写 Cancelled 的人**（origin=Runtime，
            //   ObservationOrigin 的注释点名 cancel 归它）；
            // - complete 意图：`TaskService::complete` 已把状态写成 done，退出不再动；
            // - 普通退出：settle_exit 判 Done/Failed（session 证据 CLI b1.2 恒
            //   Absent、信号事实 portable-pty 给不出——两处缺口在 ⑦ 具名）。
            let cancelled = ctx.registry.cancelled_tasks.lock().remove(&task_id);
            let completed = ctx
                .registry
                .manually_completed_tasks
                .lock()
                .remove(&task_id);
            let settled = if cancelled {
                Some((aeroric_core::tasks::TaskStatus::Cancelled, None))
            } else if completed {
                None
            } else {
                use aeroric_core::terminal::exit::{
                    settle_exit, ExitDecision, ExitFacts, SessionEvidence,
                };
                match settle_exit(&ExitFacts {
                    family: ctx.family,
                    exit_ok: code == Some(0),
                    exit_code: code.and_then(|c| u32::try_from(c).ok()),
                    signal: None,
                    session: SessionEvidence::Absent,
                    cancelled: false,
                    manually_completed: false,
                }) {
                    ExitDecision::NoStatus => None,
                    ExitDecision::Done => Some((aeroric_core::tasks::TaskStatus::Done, None)),
                    ExitDecision::Failed { reason } => Some((
                        aeroric_core::tasks::TaskStatus::Failed,
                        Some(reason.host_sentence()),
                    )),
                }
            };
            let Some((status, failure_reason)) = settled else {
                return;
            };
            let service = ctx.service.lock().expect("lock").upgrade();
            match service {
                Some(service) => {
                    let _ = service.observe(
                        &project_id,
                        TaskEvent::Status {
                            task_id: task_id.clone(),
                            generation,
                            status,
                            attention_requested_at_ms: None,
                            failure_reason,
                            origin: aeroric_core::tasks::executor::ObservationOrigin::Runtime,
                        },
                    );
                }
                None => {
                    ctx.log.append(
                        "task-status",
                        json!({
                            "taskId": task_id,
                            "generation": generation,
                            "status": format!("{status:?}"),
                            "serviceUnwired": true,
                        }),
                    );
                }
            }
        });
        Ok(())
    }
}

impl TaskExecutor for CliPtyExecutor {
    fn launch(&self, request: &LaunchRequest) -> Execution<()> {
        let WorkingDirectory::Local { path } = request.working_directory.clone() else {
            return Err(ExecutorFailure::Unsupported {
                reason: "the headless host runs local workspaces only; SSH/WSL land in P7-01/02"
                    .to_string(),
            });
        };
        if request.family == ProtocolFamily::Dsh {
            return Err(ExecutorFailure::Unsupported {
                reason: "dsh launch is a desktop-host path (dsh_host::executor); the headless \
                         host exposes the approval surface via approval.list/respond instead"
                    .to_string(),
            });
        }
        let input = self.resolve_launch(&request.agent);
        let mut plan = match request.family {
            ProtocolFamily::Claude => claude_plan(
                &input,
                &request.permission_mode_wire,
                &request.agent,
                request.selected_model.as_deref(),
                request.reasoning_effort.as_deref(),
            ),
            ProtocolFamily::Codex => codex_plan(
                &input,
                &request.permission_mode_wire,
                &path,
                request.selected_model.as_deref(),
                request.reasoning_effort.as_deref(),
                request.speed.as_deref(),
            ),
            ProtocolFamily::Omp => omp_plan(
                &input,
                &request.permission_mode_wire,
                &[],
                &path,
                request.selected_model.as_deref(),
                request.reasoning_effort.as_deref(),
                request.session_id.as_deref(),
                &[],
            ),
            ProtocolFamily::Dsh => unreachable!("rejected above"),
        };
        plan = plan.cwd(path);
        self.spawn_plan_core(
            plan,
            &request.task_id,
            &request.project_id,
            request.generation,
            request.terminal_size,
            request.family,
        )
    }

    /// resume＝带 `session_id` 的再连（ADR-029 的 handoff 语义由调用方
    /// TaskService 决定）；b1.1 只有 claude/codex 的 plan 吃 resume——
    /// omp 走 `omp_plan` 的 resume 参数。形状与 launch 同路径。
    fn resume(&self, request: &ResumeRequest) -> Execution<()> {
        if request.family == ProtocolFamily::Dsh {
            return Err(ExecutorFailure::Unsupported {
                reason: "dsh has no native resume; the continuation is a handoff into a new \
                         session, which core decides (ADR-029)"
                    .to_string(),
            });
        }
        let WorkingDirectory::Local { path } = request.working_directory.clone() else {
            return Err(ExecutorFailure::Unsupported {
                reason: "the headless host runs local workspaces only; SSH/WSL land in P7-01/02"
                    .to_string(),
            });
        };
        let input = self.resolve_launch(&request.agent);
        let mut plan = match request.family {
            ProtocolFamily::Claude => {
                let mut p = claude_plan(
                    &input,
                    &request.permission_mode_wire,
                    &request.agent,
                    request.selected_model.as_deref(),
                    request.reasoning_effort.as_deref(),
                );
                p = p.arg("--resume").arg(&request.session_id);
                p
            }
            ProtocolFamily::Codex => {
                let mut p = codex_plan(
                    &input,
                    &request.permission_mode_wire,
                    &path,
                    request.selected_model.as_deref(),
                    request.reasoning_effort.as_deref(),
                    request.speed.as_deref(),
                );
                p = p.arg("resume").arg(&request.session_id);
                p
            }
            ProtocolFamily::Omp => omp_plan(
                &input,
                &request.permission_mode_wire,
                &[],
                &path,
                request.selected_model.as_deref(),
                request.reasoning_effort.as_deref(),
                Some(&request.session_id),
                &[],
            ),
            ProtocolFamily::Dsh => unreachable!("rejected above"),
        };
        plan = plan.cwd(path);
        self.spawn_plan_core(
            plan,
            &request.task_id,
            &request.project_id,
            request.generation,
            request.terminal_size,
            request.family,
        )
    }

    fn cancel(&self, task_id: &str, _target: &CancelTarget) -> Execution<()> {
        // 取消意图先记账（exit 线程消费它结算 cancelled），再杀进程。
        self.registry
            .cancelled_tasks
            .lock()
            .insert(task_id.to_string());
        self.kill_run(task_id);
        Ok(())
    }

    fn complete(&self, task_id: &str, _working_directory: &str) -> Execution<()> {
        self.registry
            .manually_completed_tasks
            .lock()
            .insert(task_id.to_string());
        self.kill_run(task_id);
        Ok(())
    }

    /// 退场账：台账 retire＋取消/完成标记清除。session 归属字段回 Default——
    /// 会话文件跟踪（claude/codex jsonl 归属）在 CLI host 是 ⑦ 的具名缺口，
    /// **不假装有**：字段空＝客户端 mergeResetTaskSession 保持原归属。
    fn reset(&self, task_id: &str) -> Result<ResetOutcome, ExecutorFailure> {
        self.registry.runs.retire_task(task_id);
        self.registry.cancelled_tasks.lock().remove(task_id);
        self.registry
            .manually_completed_tasks
            .lock()
            .remove(task_id);
        Ok(ResetOutcome::default())
    }
}

/// 存活判定读同一张句柄账（ADR-036：core 问、账在持有者）。
pub struct RegistryLiveRuns {
    registry: Arc<TaskRegistry>,
}

impl RegistryLiveRuns {
    pub fn new(registry: Arc<TaskRegistry>) -> Self {
        Self { registry }
    }
}

impl LiveRuns for RegistryLiveRuns {
    fn has_live_run(&self, task_id: &str) -> bool {
        self.registry.child_for(task_id).is_some()
    }
}
