use super::*;
use crate::sessions::live::codex::fake::{launched, launcher, until};

#[tokio::test]
async fn connects_once_and_reuses_the_handshaken_process() {
    let (launcher, mut peers) = launcher();
    let server = CodexServer::new(launcher);
    let connecting = tokio::spawn({
        let server = Arc::clone(&server);
        async move { server.connect().await.map(|c| c.epoch) }
    });
    let mut peer = launched(&mut peers).await;
    peer.handshake().await;
    let first = connecting.await.unwrap().unwrap();
    assert_eq!(server.connect().await.unwrap().epoch, first);
    assert!(peers.try_recv().is_err(), "no second process");
}

#[tokio::test]
async fn routes_by_thread_and_refuses_requests_nobody_owns() {
    let (launcher, mut peers) = launcher();
    let server = CodexServer::new(launcher);
    let connecting = tokio::spawn({
        let server = Arc::clone(&server);
        async move { server.connect().await.map(|c| c.epoch) }
    });
    let mut peer = launched(&mut peers).await;
    peer.handshake().await;
    let epoch = connecting.await.unwrap().unwrap();
    let (inbox, mut inbound) = mpsc::unbounded_channel();
    server.register("th", epoch, inbox);

    peer.notify(
        "turn/started",
        json!({"threadId": "th", "turn": {"id": "u"}}),
    );
    peer.notify("thread/started", json!({"thread": {"id": "th"}}));
    assert!(
        matches!(inbound.recv().await, Some(Inbound::Notification { method, .. }) if method == "turn/started")
    );
    assert!(
        matches!(inbound.recv().await, Some(Inbound::Notification { method, .. }) if method == "thread/started")
    );

    peer.request(
        5,
        "item/commandExecution/requestApproval",
        json!({"threadId": "other"}),
    );
    let refused = peer.next().await;
    assert_eq!(refused["id"], 5);
    assert_eq!(refused["error"]["code"], -32601);
}

#[tokio::test]
async fn an_exit_tells_its_sessions_and_repeated_exits_stop_restarts() {
    let (launcher, mut peers) = launcher();
    let server = CodexServer::new(launcher);
    for round in 0..CRASH_LIMIT {
        let connecting = tokio::spawn({
            let server = Arc::clone(&server);
            async move { server.connect().await.map(|c| c.epoch) }
        });
        let mut peer = launched(&mut peers).await;
        peer.handshake().await;
        let epoch = connecting.await.unwrap().unwrap();
        let (inbox, mut inbound) = mpsc::unbounded_channel();
        server.register(&format!("th{round}"), epoch, inbox);
        peer.close();
        assert!(
            matches!(inbound.recv().await, Some(Inbound::Notification { method, .. }) if method == SERVER_EXITED)
        );
        let stopped = Arc::clone(&server);
        until("the exit to be processed", move || {
            stopped
                .running
                .try_lock()
                .map(|r| r.is_none())
                .unwrap_or(false)
        })
        .await;
    }
    let refused = server.connect().await.err().expect("restarts stop");
    assert!(refused.contains("连续退出"), "{refused}");
    assert!(
        peers.try_recv().is_err(),
        "nothing launched once the budget is spent"
    );
}

#[tokio::test]
async fn an_unused_server_is_stopped_without_counting_as_a_crash() {
    let (launcher, mut peers) = launcher();
    let server = CodexServer::new(launcher);
    let connecting = tokio::spawn({
        let server = Arc::clone(&server);
        async move { server.connect().await.map(|c| c.epoch) }
    });
    let mut peer = launched(&mut peers).await;
    peer.handshake().await;
    let epoch = connecting.await.unwrap().unwrap();
    let (inbox, _inbound) = mpsc::unbounded_channel();
    server.register("th", epoch, inbox);
    let later = now_ms() + SERVER_IDLE_MS;
    assert!(
        !server.stop_if_unused(later).await,
        "a routed thread keeps it"
    );
    server.unregister("th");
    assert!(
        !server.stop_if_unused(now_ms()).await,
        "not idle long enough"
    );
    assert!(server.stop_if_unused(later + 1).await);
    assert!(!server.is_running().await);
    peer.close();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(server.crashes.lock().is_empty());
}

#[cfg(unix)]
mod process {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in `codex`: answers `--version`, then the initialize handshake.
    fn fake_codex(version: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeroric-fake-codex-{}-{}",
            std::process::id(),
            version.replace('.', "_")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("codex");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do last=\"$a\"; done\n\
                 if [ \"$last\" = \"--version\" ]; then echo \"codex-cli {version}\"; exit 0; fi\n\
                 if [ \"$last\" = \"app-server\" ] && [ \"$RUST_LOG\" = \"error\" ] && [ \"$FAKE_MARK\" = \"set\" ]; then\n\
                 read line\necho '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\ncat > /dev/null\nfi\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    fn spec(script: std::path::PathBuf) -> ProcessSpec {
        ProcessSpec {
            program: script.to_string_lossy().into_owned(),
            args: vec![],
            env: vec![("FAKE_MARK".into(), "set".into())],
            unset_env: vec![],
            cwd: None,
        }
    }

    #[tokio::test]
    async fn a_real_process_is_version_checked_spawned_and_handshaken() {
        let server = CodexServer::new(process_launcher(spec(fake_codex("0.162.1"))));
        server
            .connect()
            .await
            .expect("handshake with the fake codex");
        assert!(server.is_running().await);
    }

    #[tokio::test]
    async fn an_old_codex_is_refused_with_an_upgrade_hint() {
        let server = CodexServer::new(process_launcher(spec(fake_codex("0.100.0"))));
        let refused = server.connect().await.err().expect("too old");
        assert!(refused.contains("请升级"), "{refused}");
    }
}
