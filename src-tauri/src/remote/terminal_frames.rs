//! 终端流二进制帧(kind 0x74 't',v1)。
//!
//! 16 字节头 + payload,小端:
//! ```text
//! [0]=kind  [1]=version  [2]=opcode  [3]=0
//! [4..8]=streamId(u32)   [8..16]=seq(u64)
//! ```
//! streamId 由客户端在 Subscribe 时分配(连接内唯一),后续双向帧都携带;
//! seq 是服务端 Output 帧的流内单调计数,客户端发帧填 0。
//! 快照即断线恢复机制:重连后重新 Subscribe,历史文件尾部保证不丢行。

pub const TERMINAL_FRAME_KIND: u8 = 0x74;
pub const TERMINAL_FRAME_VERSION: u8 = 1;
pub const HEADER_BYTES: usize = 16;
/// 单帧 payload 上限,与 server 侧 WS 消息上限对齐。
pub const MAX_PAYLOAD_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TerminalOpcode {
    /// 服务端→客户端:live 输出(payload=UTF-8 字节)
    Output = 1,
    /// 服务端→客户端:快照开始(payload=JSON 元数据 {cols,rows,live})
    SnapshotStart = 2,
    SnapshotChunk = 3,
    SnapshotEnd = 4,
    /// 服务端→客户端:PTY 尺寸变化通知(payload=JSON {cols,rows})
    Resized = 5,
    /// 服务端→客户端:流级错误(payload=UTF-8 文案),发completed后流关闭
    Error = 6,
    /// 客户端→服务端:终端输入(payload=UTF-8 字节)
    Input = 7,
    /// 客户端→服务端:调整 PTY 尺寸(payload=JSON {cols,rows})
    Resize = 8,
    /// 客户端→服务端:订阅(payload=JSON {taskId})
    Subscribe = 9,
    Unsubscribe = 10,
}

impl TerminalOpcode {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Output,
            2 => Self::SnapshotStart,
            3 => Self::SnapshotChunk,
            4 => Self::SnapshotEnd,
            5 => Self::Resized,
            6 => Self::Error,
            7 => Self::Input,
            8 => Self::Resize,
            9 => Self::Subscribe,
            10 => Self::Unsubscribe,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalFrame {
    pub opcode: TerminalOpcode,
    pub stream_id: u32,
    pub seq: u64,
    pub payload: Vec<u8>,
}

impl TerminalFrame {
    pub fn new(opcode: TerminalOpcode, stream_id: u32, seq: u64, payload: Vec<u8>) -> Self {
        Self {
            opcode,
            stream_id,
            seq,
            payload,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_BYTES + self.payload.len());
        out.push(TERMINAL_FRAME_KIND);
        out.push(TERMINAL_FRAME_VERSION);
        out.push(self.opcode as u8);
        out.push(0);
        out.extend_from_slice(&self.stream_id.to_le_bytes());
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// 非终端帧(kind/version 不符)返回 None 交上层忽略;畸形长度也归为 None。
pub fn decode_terminal_frame(bytes: &[u8]) -> Option<TerminalFrame> {
    if bytes.len() < HEADER_BYTES || bytes.len() > HEADER_BYTES + MAX_PAYLOAD_BYTES {
        return None;
    }
    if bytes[0] != TERMINAL_FRAME_KIND || bytes[1] != TERMINAL_FRAME_VERSION {
        return None;
    }
    let opcode = TerminalOpcode::from_u8(bytes[2])?;
    let stream_id = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let seq = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    Some(TerminalFrame {
        opcode,
        stream_id,
        seq,
        payload: bytes[HEADER_BYTES..].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_all_opcodes() {
        for opcode in [
            TerminalOpcode::Output,
            TerminalOpcode::SnapshotStart,
            TerminalOpcode::SnapshotChunk,
            TerminalOpcode::SnapshotEnd,
            TerminalOpcode::Resized,
            TerminalOpcode::Error,
            TerminalOpcode::Input,
            TerminalOpcode::Resize,
            TerminalOpcode::Subscribe,
            TerminalOpcode::Unsubscribe,
        ] {
            let frame = TerminalFrame::new(
                opcode,
                42,
                u64::MAX - 7,
                b"\xe4\xbd\xa0\xe5\xa5\xbd".to_vec(),
            );
            let decoded = decode_terminal_frame(&frame.encode()).expect("decode");
            assert_eq!(decoded, frame);
        }
    }

    #[test]
    fn rejects_foreign_and_malformed_frames() {
        assert!(decode_terminal_frame(&[]).is_none());
        assert!(decode_terminal_frame(&[0u8; 15]).is_none());
        // 错误 kind
        let mut frame = TerminalFrame::new(TerminalOpcode::Output, 1, 1, vec![]).encode();
        frame[0] = 0x00;
        assert!(decode_terminal_frame(&frame).is_none());
        // 错误 version
        let mut frame = TerminalFrame::new(TerminalOpcode::Output, 1, 1, vec![]).encode();
        frame[1] = 99;
        assert!(decode_terminal_frame(&frame).is_none());
        // 未知 opcode
        let mut frame = TerminalFrame::new(TerminalOpcode::Output, 1, 1, vec![]).encode();
        frame[2] = 200;
        assert!(decode_terminal_frame(&frame).is_none());
        // 超限 payload
        let huge = TerminalFrame::new(
            TerminalOpcode::Output,
            1,
            1,
            vec![0u8; MAX_PAYLOAD_BYTES + 1],
        );
        assert!(decode_terminal_frame(&huge.encode()).is_none());
    }
}
