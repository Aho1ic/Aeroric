//! 拉起 / 复用 `dsh web` 后端进程。
//!
//! 这一块从 `dsh_webui.rs` 整块搬出来,内容一行没改。它管的是**进程生命周期**:
//! 拼子命令参数、把 stdout/stderr 抽干并从里面认出启动 URL、等它就绪、
//! 以及在 lifecycle 锁上排队以免两次启动互相踩。
//!
//! 会话层的东西(RPC、事件流、终端渲染)不在这里 —— 那些在父模块和
//! `api_client` / `event_stream` / `terminal_render` 里。

use super::*;

/// 传给 `dsh` 的子命令参数(在用户配置的 `launch.args` 之后)。
///
/// `--no-open`:我们只把 `dsh web` 当 headless RPC 后端,会话跑在 Aeroric 自己的
/// 终端里。上游 web-app 的 `openBrowser` 默认为 true,不带这个 flag 每次拉起后端
/// 都会顺带弹一个系统浏览器 —— 看起来就像"启动终端跳去了网页版"。
/// `printUrl` 在 web-app 的 cordis.patch.yml 里硬编码 true、与 `openBrowser` 相互
/// 独立,所以这里照常能读到启动 URL 那行。
pub(super) fn dsh_web_command_args() -> [&'static str; 4] {
    ["web", "--port", "0", "--no-open"]
}

#[derive(Default)]
pub(super) struct DshWebStartupOutput {
    stdout: String,
    stderr: String,
}

pub(super) fn append_bounded_output(target: &mut String, line: &str) {
    target.push_str(line);
    target.push('\n');
    if target.len() <= DSH_WEB_OUTPUT_LIMIT {
        return;
    }
    let remove_at_least = target.len() - DSH_WEB_OUTPUT_LIMIT;
    let split = target
        .char_indices()
        .find_map(|(index, _)| (index >= remove_at_least).then_some(index))
        .unwrap_or(target.len());
    target.drain(..split);
}

pub(super) fn startup_output_detail(output: &Arc<Mutex<DshWebStartupOutput>>) -> String {
    let output = output.lock();
    let mut sections = Vec::new();
    if !output.stderr.trim().is_empty() {
        sections.push(format!("stderr:\n{}", output.stderr.trim()));
    }
    if !output.stdout.trim().is_empty() {
        sections.push(format!("stdout:\n{}", output.stdout.trim()));
    }
    sections.join("\n")
}

pub(super) fn attach_startup_output(
    error: String,
    output: &Arc<Mutex<DshWebStartupOutput>>,
) -> String {
    let detail = startup_output_detail(output);
    if detail.is_empty() || error.contains(&detail) {
        error
    } else {
        format!("{error}\n{detail}")
    }
}

async fn finish_dsh_web_output_drains(
    stdout: tokio::task::JoinHandle<()>,
    stderr: tokio::task::JoinHandle<()>,
) {
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        let _ = tokio::join!(stdout, stderr);
    })
    .await;
}

pub(super) fn parse_dsh_web_startup_url(line: &str) -> Result<Option<(String, u16)>, String> {
    let Some(value) = line.trim().strip_prefix("dsh web:").map(str::trim) else {
        return Ok(None);
    };
    let parsed = Url::parse(value)
        .map_err(|error| format!("DSH Web reported an invalid startup URL {value:?}: {error}"))?;
    if parsed.scheme() != "http" {
        return Err(format!("DSH Web reported a non-HTTP startup URL: {value}"));
    }
    let host = parsed
        .host()
        .ok_or_else(|| format!("DSH Web startup URL has no host: {value}"))?;
    let loopback = match &host {
        Host::Domain(domain) => domain.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => IpAddr::V4(*address).is_loopback(),
        Host::Ipv6(address) => IpAddr::V6(*address).is_loopback(),
    };
    if !loopback {
        return Err(format!(
            "DSH Web reported a non-loopback startup URL: {value}"
        ));
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(format!(
            "DSH Web reported an unsupported startup URL shape: {value}"
        ));
    }
    let port = parsed
        .port()
        .ok_or_else(|| format!("DSH Web startup URL has no port: {value}"))?;
    if port == 0 {
        return Err("DSH Web reported port 0 instead of its allocated port".to_string());
    }
    let base_url = match host {
        Host::Domain(domain) => format!("http://{domain}:{port}"),
        Host::Ipv4(address) => format!("http://{address}:{port}"),
        Host::Ipv6(address) => format!("http://[{address}]:{port}"),
    };
    Ok(Some((base_url, port)))
}

async fn drain_dsh_web_output<R>(
    reader: R,
    is_stderr: bool,
    startup_url: Option<Arc<Mutex<Option<oneshot::Sender<Result<(String, u16), String>>>>>>,
    output: Arc<Mutex<DshWebStartupOutput>>,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => {
                let message = format!("Failed to read DSH Web process output: {error}");
                {
                    let mut output = output.lock();
                    append_bounded_output(
                        if is_stderr {
                            &mut output.stderr
                        } else {
                            &mut output.stdout
                        },
                        &message,
                    );
                }
                if let Some(startup_url) = &startup_url {
                    if let Some(sender) = startup_url.lock().take() {
                        let _ = sender.send(Err(message));
                    }
                }
                break;
            }
        };
        {
            let mut output = output.lock();
            append_bounded_output(
                if is_stderr {
                    &mut output.stderr
                } else {
                    &mut output.stdout
                },
                &line,
            );
        }
        if let Some(startup_url) = &startup_url {
            match parse_dsh_web_startup_url(&line) {
                Ok(Some(url)) => {
                    if let Some(sender) = startup_url.lock().take() {
                        let _ = sender.send(Ok(url));
                    }
                }
                Err(error) => {
                    if let Some(sender) = startup_url.lock().take() {
                        let _ = sender.send(Err(error));
                    }
                }
                Ok(None) => {}
            }
        }
    }
}

pub(super) fn exited_dsh_web_error(
    status: std::process::ExitStatus,
    output: &Arc<Mutex<DshWebStartupOutput>>,
) -> String {
    let detail = startup_output_detail(output);
    if detail.is_empty() {
        format!("DSH Web exited before becoming ready ({status})")
    } else {
        format!("DSH Web exited before becoming ready ({status})\n{detail}")
    }
}

async fn wait_for_dsh_web_url(
    child: &mut Child,
    startup_url: oneshot::Receiver<Result<(String, u16), String>>,
    deadline: Instant,
    output: &Arc<Mutex<DshWebStartupOutput>>,
) -> Result<(String, u16), String> {
    tokio::pin!(startup_url);
    loop {
        tokio::select! {
            result = &mut startup_url => {
                return result
                    .map_err(|_| {
                        let detail = startup_output_detail(output);
                        if detail.is_empty() {
                            "DSH Web did not report its startup URL".to_string()
                        } else {
                            format!("DSH Web did not report its startup URL\n{detail}")
                        }
                    })?;
            }
            _ = sleep(Duration::from_millis(100)) => {
                if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                    return Err(exited_dsh_web_error(status, output));
                }
                if Instant::now() >= deadline {
                    let detail = startup_output_detail(output);
                    return Err(if detail.is_empty() {
                        "DSH Web did not report its startup URL within 30 seconds".to_string()
                    } else {
                        format!("DSH Web did not report its startup URL within 30 seconds\n{detail}")
                    });
                }
            }
        }
    }
}

pub(super) async fn check_health(
    url: &str,
    child: &mut Child,
    deadline: Instant,
    output: &Arc<Mutex<DshWebStartupOutput>>,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Err(exited_dsh_web_error(status, output));
        }
        let last_error = match tokio::time::timeout_at(deadline, client.get(url).send()).await {
            Ok(Ok(response)) if response.status().is_success() => {
                return Ok(());
            }
            Ok(Ok(response)) => format!("HTTP {}", response.status()),
            Ok(Err(error)) => error.to_string(),
            Err(_) => "startup deadline elapsed during the health request".to_string(),
        };
        if Instant::now() >= deadline {
            let detail = startup_output_detail(output);
            let suffix = if detail.is_empty() {
                String::new()
            } else {
                format!("\n{detail}")
            };
            return Err(format!(
                "Health check timed out after 30 seconds: {last_error}{suffix}"
            ));
        }
        sleep(Duration::from_millis(250)).await;
    }
}

pub(super) async fn ensure_dsh_webui_locked(
    agent: &str,
    state: &DshWebUiManager,
) -> Result<DshWebUiState, String> {
    if state.shutting_down.load(Ordering::Acquire) {
        return Err("DSH Web is shutting down".to_string());
    }
    let mut stale_process = {
        let mut processes = state.processes.write();
        if let Some(process) = processes.get_mut(agent) {
            if process.state.status == WebUiStatus::Running {
                match process.child.try_wait() {
                    Ok(None) => return Ok(process.state.clone()),
                    Ok(Some(status)) => {
                        process.state.status = WebUiStatus::Error;
                        process.state.error = Some(exited_dsh_web_error(status, &process.output));
                    }
                    Err(error) => {
                        process.state.status = WebUiStatus::Error;
                        process.state.error = Some(format!(
                            "Could not inspect the existing DSH Web process: {error}"
                        ));
                    }
                }
            }
        }
        processes.remove(agent)
    };
    if let Some(mut process) = stale_process.take() {
        let _ = DshWebUiManager::stop_process(&mut process.child).await;
    }

    let home = crate::dsh_home::ensure_dsh_home_for(agent)?;
    let launch = crate::app_settings::get_agent_launch_spec(agent);
    if let Some(root) = &launch.working_dir {
        if !root.join("node_modules").is_dir() {
            return Err(format!(
                "DeepSeek Harness source at {} has no node_modules. Run `pnpm install` and `pnpm run build` in that directory, then retry.",
                root.display()
            ));
        }
        // checkout 启动依赖 pnpm,而这条分支以前从不校验它:`working_dir` 恒为
        // Some,所以下面那个 PATH 检查永远走不到,缺 pnpm 的机器会一路走到 spawn
        // 的 ENOENT,报成"dsh 没装"——dsh 明明配好了,缺的是 pnpm。
        if !launch.program.contains('/')
            && !launch.program.contains('\\')
            && crate::platform::detect_path(&launch.program).is_empty()
        {
            return Err(format!(
                "`{}` was not found in PATH, so the DeepSeek Harness source checkout at {} cannot be launched. \
                 Install pnpm (for example `corepack enable pnpm`), or point the DSH executable at an \
                 Aeroric-managed install, which needs neither pnpm nor a build step.",
                launch.program,
                root.display()
            ));
        }
        let missing = dsh_checkout_missing_artifacts(root);
        if !missing.is_empty() {
            return Err(checkout_not_built_error(root, &missing));
        }
    } else if !launch.program.contains('/') && !launch.program.contains('\\') {
        // A GUI app does not inherit the interactive shell's PATH. Resolve
        // the same login-shell PATH used for child processes before spawning,
        // so a missing global dsh is reported before an opaque ENOENT.
        if crate::platform::detect_path(&launch.program).is_empty() {
            return Err(format!(
                "DeepSeek Harness executable `{}` was not found in PATH. Configure the DSH executable or select its source directory, then run `pnpm install` and `pnpm run build` there.",
                launch.program
            ));
        }
    }

    let mut cmd = Command::new(&launch.program);
    // Windows 上 dsh 解析成 dsh.cmd,Rust 会经 cmd.exe 启动它。不带
    // CREATE_NO_WINDOW 就会弹出一个真实的 cmd 控制台窗口,并且因为这是常驻
    // sidecar,窗口会跟着整个会话一直停在桌面上——而不是一闪而过。
    // 同时把它放进独立进程组:dsh web 会再拉起自己的子进程,停止时必须能按组
    // 收掉(见 `stop_process`),否则残留进程会一直占着端口。
    crate::subprocess::configure_terminable_tokio_process_tree(&mut cmd);
    cmd.args(&launch.args);
    if let Some(working_dir) = &launch.working_dir {
        cmd.current_dir(working_dir);
    }
    // `--patch` is a launcher-level option for profile/headless invocations.
    // The official `dsh web` alias rejects parent `--patch` options, so passing
    // Aeroric's headless overlays here makes the Web process exit immediately
    // with: "web takes none of parent --patch ...". Web/API settings are
    // persisted through DSH_HOME and its RPC, so no patch is needed here.
    cmd.args(dsh_web_command_args())
        .envs(launch.extra_env)
        .env("PATH", crate::app_settings::get_login_shell_path())
        .env("DSH_HOME", &home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            if error.kind() == std::io::ErrorKind::NotFound {
                return Err(
                    "DeepSeek Harness is not installed or not found in PATH. Configure dsh_path with the dsh executable or its source directory, then run `pnpm install` and `pnpm run build`.".to_string(),
                );
            }
            return Err(format!("Failed to spawn dsh web: {error}"));
        }
    };

    let pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture DSH Web stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture DSH Web stderr".to_string())?;
    let output = Arc::new(Mutex::new(DshWebStartupOutput::default()));
    let (startup_url_tx, startup_url_rx) = oneshot::channel();
    let startup_url_tx = Arc::new(Mutex::new(Some(startup_url_tx)));
    let stdout_drain = tokio::spawn(drain_dsh_web_output(
        stdout,
        false,
        Some(startup_url_tx.clone()),
        output.clone(),
    ));
    let stderr_drain = tokio::spawn(drain_dsh_web_output(
        stderr,
        true,
        Some(startup_url_tx),
        output.clone(),
    ));

    let deadline = Instant::now() + DSH_WEB_STARTUP_TIMEOUT;
    let (url, port) =
        match wait_for_dsh_web_url(&mut child, startup_url_rx, deadline, &output).await {
            Ok(value) => value,
            Err(error) => {
                let _ = DshWebUiManager::stop_process(&mut child).await;
                finish_dsh_web_output_drains(stdout_drain, stderr_drain).await;
                return Err(explain_dsh_web_failure(
                    attach_startup_output(error, &output),
                    launch.working_dir.as_deref(),
                ));
            }
        };

    let mut initial_state = DshWebUiState {
        agent: agent.to_string(),
        port,
        url: Some(url.clone()),
        pid,
        status: WebUiStatus::Starting,
        error: None,
    };

    let health_check_result = check_health(&url, &mut child, deadline, &output).await;

    match health_check_result {
        Ok(_) => {
            initial_state.status = WebUiStatus::Running;
            let mut processes = state.processes.write();
            processes.insert(
                agent.to_string(),
                WebUiProcess {
                    child,
                    state: initial_state.clone(),
                    output,
                },
            );
        }
        Err(e) => {
            initial_state.status = WebUiStatus::Error;
            let error = format!("DSH Web failed to become ready at {url}: {e}");
            initial_state.error = Some(error.clone());
            let _ = DshWebUiManager::stop_process(&mut child).await;
            finish_dsh_web_output_drains(stdout_drain, stderr_drain).await;
            return Err(explain_dsh_web_failure(
                attach_startup_output(error, &output),
                launch.working_dir.as_deref(),
            ));
        }
    }

    Ok(initial_state)
}

/// 拿 lifecycle 锁,但别在升级期间无限期挂着。
///
/// 只有「等超了 **且** 确实有安装/升级在跑」才放弃。单看超时是不够的:另一次
/// start 也会占着锁,而 DSH Web 冷启动本身就允许 30s
/// ([`DSH_WEB_STARTUP_TIMEOUT`]),那种情况必须继续等 —— 等到了还能靠
/// `newer_start_result` 直接复用它的结果。
async fn acquire_lifecycle_guard<'lock>(
    lifecycle_lock: &'lock Arc<AsyncMutex<()>>,
    wait: Duration,
    upgrade_is_running: impl Fn() -> bool,
) -> Result<tokio::sync::MutexGuard<'lock, ()>, String> {
    match tokio::time::timeout(wait, lifecycle_lock.lock()).await {
        Ok(guard) => Ok(guard),
        Err(_) if upgrade_is_running() => Err(
            "DSH Web is busy finishing an install or upgrade. Try again once it completes."
                .to_string(),
        ),
        // 不是升级占的锁 —— 那就是另一次启动,老实等完。
        Err(_) => Ok(lifecycle_lock.lock().await),
    }
}

pub(super) async fn ensure_dsh_webui(
    agent: &str,
    state: &DshWebUiManager,
) -> Result<DshWebUiState, String> {
    let observed_generation = state.start_generation(agent);
    let lifecycle_lock = state.lifecycle_lock(agent);
    let _lifecycle_guard =
        acquire_lifecycle_guard(&lifecycle_lock, DSH_LIFECYCLE_BUSY_TIMEOUT, || {
            crate::agent_ops::binary_operation_is_running(agent)
        })
        .await?;
    if let Some(result) = state.newer_start_result(agent, observed_generation) {
        return result;
    }
    let result = ensure_dsh_webui_locked(agent, state).await;
    state.record_start_result(agent, result.clone());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_allocated_loopback_dsh_web_urls() {
        assert_eq!(
            parse_dsh_web_startup_url("dsh web: http://127.0.0.1:43127"),
            Ok(Some(("http://127.0.0.1:43127".to_string(), 43127)))
        );
        assert_eq!(
            parse_dsh_web_startup_url("dsh web: http://[::1]:51844/"),
            Ok(Some(("http://[::1]:51844".to_string(), 51844)))
        );
        assert_eq!(parse_dsh_web_startup_url("warming up"), Ok(None));
        assert!(parse_dsh_web_startup_url("dsh web: http://0.0.0.0:15800")
            .expect_err("a non-loopback listener is rejected")
            .contains("non-loopback"));
        assert!(
            parse_dsh_web_startup_url("dsh web: https://127.0.0.1:15800")
                .expect_err("the startup protocol must stay HTTP")
                .contains("non-HTTP")
        );
        assert!(parse_dsh_web_startup_url("dsh web: http://127.0.0.1:0")
            .expect_err("port zero is not an allocated listener")
            .contains("allocated port"));
    }

    #[test]
    fn retains_only_the_tail_of_dsh_web_process_output() {
        let mut output = String::new();
        let prefix = "x".repeat(DSH_WEB_OUTPUT_LIMIT);
        append_bounded_output(&mut output, &prefix);
        append_bounded_output(&mut output, "final diagnostic");

        assert!(output.len() <= DSH_WEB_OUTPUT_LIMIT);
        assert!(output.chars().filter(|character| *character == 'x').count() < prefix.len());
        assert!(output.ends_with("final diagnostic\n"));
    }

    #[test]
    fn keeps_the_web_backend_from_opening_a_browser() {
        let args = dsh_web_command_args();
        // 只当 headless RPC 后端用:少了 --no-open,上游默认会弹系统浏览器,
        // 表现就是"启动终端跳到网页版"。
        assert!(args.contains(&"--no-open"));
        // 端口仍然交给 OS 选,启动 URL 由 stdout 那行回传。
        assert_eq!(args, ["web", "--port", "0", "--no-open"]);
    }

    /// 升级会攥着 lifecycle 锁跑几分钟,这期间开终端必须拿到一句解释,
    /// 而不是无限期挂在 await 上。这里用毫秒级超时,不必真等 3 秒。
    #[tokio::test]
    async fn acquiring_the_lifecycle_lock_gives_up_while_an_upgrade_holds_it() {
        let manager = DshWebUiManager::new();
        let lifecycle_lock = manager.lifecycle_lock("dsh");
        let upgrade_guard = lifecycle_lock.clone().lock_owned().await;

        let error = acquire_lifecycle_guard(&lifecycle_lock, Duration::from_millis(20), || true)
            .await
            .expect_err("the blocked start reports the upgrade instead of hanging");
        assert!(
            error.contains("install or upgrade"),
            "unexpected error: {error}"
        );

        // 升级放手之后就该正常拿到锁。
        drop(upgrade_guard);
        let _reacquired = acquire_lifecycle_guard(&lifecycle_lock, Duration::from_secs(1), || true)
            .await
            .expect("the lock is available once the upgrade releases it");
    }

    /// 另一次 start 占着锁时不能改口说「在升级」:DSH Web 冷启动本身就允许 30s,
    /// 等到了还能复用它的结果。
    #[tokio::test]
    async fn acquiring_the_lifecycle_lock_waits_out_a_concurrent_start() {
        let manager = DshWebUiManager::new();
        let lifecycle_lock = manager.lifecycle_lock("dsh");
        let slow_start = lifecycle_lock.clone().lock_owned().await;

        let waiter_lock = lifecycle_lock.clone();
        let waiter = tokio::spawn(async move {
            // 没有安装/升级在跑,所以超时也只能继续等。
            acquire_lifecycle_guard(&waiter_lock, Duration::from_millis(20), || false)
                .await
                .map(|_| ())
        });

        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(
            !waiter.is_finished(),
            "the waiter keeps waiting instead of bailing out"
        );
        drop(slow_start);
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("the waiter proceeds once the slow start releases the lock")
            .expect("the waiter task completes")
            .expect("waiting out a concurrent start is not an error");
    }
}
