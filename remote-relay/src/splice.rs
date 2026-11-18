use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, Stream, StreamExt};
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::Ws;

/// B4-2: a bridge with no frame either way for this long is dropped.
pub(crate) const SPLICE_IDLE: Duration = Duration::from_secs(120);
/// B4-2: once one side has closed, the other gets this long to drain.
pub(crate) const SPLICE_DRAIN: Duration = Duration::from_secs(5);

/// 双向逐帧盲转发。relay 不解析或持久化 E2EE 业务载荷。
///
/// B4-2(2026-11-18):一侧关闭后另一侧最多再排空 `SPLICE_DRAIN`,两侧都静默超过
/// `SPLICE_IDLE` 就断开;返回时两端连接随之释放。原先两个方向 `join!` 到底,一个
/// 不发 Close 的半开客户端能让桥(及其连接槽)永远挂着。
pub(crate) async fn splice(a: Ws, b: Ws) {
    splice_within(a, b, SPLICE_IDLE, SPLICE_DRAIN).await;
}

pub(crate) async fn splice_within(a: Ws, b: Ws, idle: Duration, drain: Duration) {
    let start = Instant::now();
    let last = AtomicU64::new(0);
    let (a_sink, a_stream) = a.split();
    let (b_sink, b_stream) = b.split();
    let a_to_b = forward(a_stream, b_sink, start, &last);
    let b_to_a = forward(b_stream, a_sink, start, &last);
    tokio::pin!(a_to_b, b_to_a);
    let watchdog = async {
        let tick = (idle / 4).max(Duration::from_millis(10));
        loop {
            tokio::time::sleep(tick).await;
            let quiet = start.elapsed().as_millis() as u64 - last.load(Ordering::Relaxed);
            if quiet >= idle.as_millis() as u64 {
                break;
            }
        }
    };
    tokio::pin!(watchdog);
    tokio::select! {
        () = &mut a_to_b => { let _ = tokio::time::timeout(drain, &mut b_to_a).await; }
        () = &mut b_to_a => { let _ = tokio::time::timeout(drain, &mut a_to_b).await; }
        () = &mut watchdog => {}
    }
}

async fn forward<S, K>(mut from: S, mut to: K, start: Instant, last: &AtomicU64)
where
    S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    K: futures_util::Sink<Message> + Unpin,
{
    while let Some(Ok(message)) = from.next().await {
        last.store(start.elapsed().as_millis() as u64, Ordering::Relaxed);
        let closing = matches!(message, Message::Close(_));
        if to.send(message).await.is_err() || closing {
            break;
        }
    }
    let _ = to.close().await;
}
