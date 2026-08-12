use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::Ws;

/// 双向逐帧盲转发。relay 不解析或持久化 E2EE 业务载荷。
pub(crate) async fn splice(a: Ws, b: Ws) {
    let (mut a_sink, mut a_stream) = a.split();
    let (mut b_sink, mut b_stream) = b.split();
    let a_to_b = async {
        while let Some(Ok(message)) = a_stream.next().await {
            let closing = matches!(message, Message::Close(_));
            if b_sink.send(message).await.is_err() || closing {
                break;
            }
        }
        let _ = b_sink.close().await;
    };
    let b_to_a = async {
        while let Some(Ok(message)) = b_stream.next().await {
            let closing = matches!(message, Message::Close(_));
            if a_sink.send(message).await.is_err() || closing {
                break;
            }
        }
        let _ = a_sink.close().await;
    };
    tokio::join!(a_to_b, b_to_a);
}
