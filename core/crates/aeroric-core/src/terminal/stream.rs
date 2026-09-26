//! 终端字节流(P3-03):原始 bytes 是唯一真相,字符串是它的投影视图。
//!
//! 设计来自计划 Action 2/4:`TerminalIdentity = (task_id, generation)`;offset 是
//! **原始字节计数**;缓冲有界——落后超过保留线的订阅者得到显式 `Gap`(带追平点),
//! 而不是一句"屏幕能完全恢复"的谎话。字节不透明:NUL、非法 UTF-8、跨帧被劈开的
//! 多字节序列都原样进流;解码是消费方的投影。

/// 一条终端流的身份:哪个任务的哪一代运行。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TerminalIdentity {
    pub task_id: String,
    pub generation: u64,
}

impl TerminalIdentity {
    pub fn new(task_id: impl Into<String>, generation: u64) -> Self {
        Self {
            task_id: task_id.into(),
            generation,
        }
    }
}

/// 一次读取的产物。`gap == true` 表示请求的 offset 已被压缩丢弃——`bytes` 是从
/// [`ByteStream::read_from`] 返回的追平点开始的现存数据,调用方必须重置视图,
/// 不能假装无缝衔接。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamChunk {
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub gap: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StreamError {
    /// 请求的 offset 超过流末端(比"还没写到的位置"更远)。
    OutOfRange { next_offset: u64 },
    /// 这条流不在册(已退场或从未存在)。与"读过头了"必须分开:前者调用方要按
    /// "历史已截断、只能重置视图"处理,后者是一个正常的追平点。
    UnknownStream,
}

/// 有界终端字节流。单写者(PTY reader 线程)、多读者(回放订阅者)。
#[derive(Debug)]
pub struct ByteStream {
    identity: TerminalIdentity,
    retained: Vec<u8>,
    /// `retained[0]` 对应的全局 offset。
    retained_from: u64,
    /// 下一个 append 的全局 offset(= 已写入总字节数)。
    next_offset: u64,
    /// 缓冲上限:超过即从头丢弃,丢到哪、`retained_from` 就走到哪。
    capacity: usize,
}

impl ByteStream {
    /// `capacity` 是"保留最近 N 字节"的硬顶;回放窗口不得超过它。
    pub fn new(identity: TerminalIdentity, capacity: usize) -> Self {
        Self {
            identity,
            retained: Vec::with_capacity(capacity.min(64 * 1024)),
            retained_from: 0,
            next_offset: 0,
            capacity,
        }
    }

    pub fn identity(&self) -> &TerminalIdentity {
        &self.identity
    }

    /// 下一次 append 会写入的全局 offset(也是流的末端)。
    pub fn next_offset(&self) -> u64 {
        self.next_offset
    }

    /// 最早的可用 offset(压缩后可能大于 0)。
    pub fn earliest_offset(&self) -> u64 {
        self.retained_from
    }

    /// 追加原始字节。缓冲超过容量即从头丢弃(保留下游还没读完的部分优先——
    /// 简化实现:丢弃到"最近 capacity 字节",落后太多的一方只能吃 Gap)。
    pub fn append(&mut self, bytes: &[u8]) {
        self.retained.extend_from_slice(bytes);
        self.next_offset += bytes.len() as u64;
        if self.retained.len() > self.capacity {
            let drop = self.retained.len() - self.capacity;
            self.retained.drain(..drop);
            self.retained_from += drop as u64;
        }
    }

    /// 从 offset 读最多 `max_bytes`。offset 落在已压缩区间 → 返回追平点的 Gap;
    /// offset 超过流末端 → `StreamError::OutOfRange`。
    pub fn read_from(&self, offset: u64, max_bytes: usize) -> Result<StreamChunk, StreamError> {
        if offset > self.next_offset {
            return Err(StreamError::OutOfRange {
                next_offset: self.next_offset,
            });
        }
        if offset < self.retained_from {
            // Gap 也要尊重 max_bytes 上限,否则"追平点之后的第一帧"可以远大于调用方
            // 愿意接受的量——慢客户端就退化成把整段缓冲一次性倒给它。
            let end = max_bytes.min(self.retained.len());
            return Ok(StreamChunk {
                offset: self.retained_from,
                bytes: self.retained[..end].to_vec(),
                gap: true,
            });
        }
        let start = (offset - self.retained_from) as usize;
        // saturating:调用方允许传 usize::MAX 表示"给我全部",直接相加会在 debug
        // 构建里溢出成 panic。
        let end = start.saturating_add(max_bytes).min(self.retained.len());
        Ok(StreamChunk {
            offset,
            bytes: self.retained[start..end].to_vec(),
            gap: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(capacity: usize) -> ByteStream {
        ByteStream::new(TerminalIdentity::new("t1", 3), capacity)
    }

    #[test]
    fn offsets_count_raw_bytes_across_appends() {
        let mut s = stream(1024);
        assert_eq!(s.next_offset(), 0);
        s.append("你好".as_bytes()); // 6 bytes (UTF-8)
        s.append(&[0x00, 0xFF]); // NUL 与非法单独字节原样
        assert_eq!(s.next_offset(), 8, "offset 是原始字节计数,与字符边界无关");

        let chunk = s.read_from(0, 1024).expect("read");
        assert_eq!(chunk.offset, 0);
        assert_eq!(chunk.bytes, b"\xe4\xbd\xa0\xe5\xa5\xbd\x00\xff");
        assert!(!chunk.gap);
    }

    #[test]
    fn reads_beyond_the_end_are_rejected_with_the_next_offset() {
        let mut s = stream(1024);
        s.append(b"abc");
        let err = s.read_from(4, 10).expect_err("越过末端");
        assert_eq!(err, StreamError::OutOfRange { next_offset: 3 });
        // 恰好等于末端是合法的空读。
        let chunk = s.read_from(3, 10).expect("read at end");
        assert!(chunk.bytes.is_empty());
    }

    #[test]
    fn compaction_gives_an_explicit_gap_not_a_fake_seamless_replay() {
        let mut s = stream(8);
        s.append(b"0123456789ABCDEF"); // 16 字节,容量 8 → 前面 8 个被丢
        assert_eq!(s.earliest_offset(), 8);

        // 请求被压缩掉的区间:拿到追平点 + gap 标记。
        let chunk = s.read_from(0, 1024).expect("read");
        assert!(chunk.gap, "落后太多的订阅者必须被告知有缺口");
        assert_eq!(chunk.offset, 8);
        assert_eq!(chunk.bytes, b"89ABCDEF");

        // offset 已落在保留区内 → 无缝读,不撒谎也不重复。10 在保留区(8..16)中间,
        // 所以拿到的是从 10 起算的 6 个字节。
        let chunk = s.read_from(10, 1024).expect("read");
        assert!(!chunk.gap);
        assert_eq!(chunk.offset, 10);
        assert_eq!(chunk.bytes, b"ABCDEF");

        // 追平到流末端之后同样无缝。
        let chunk = s.read_from(12, 1024).expect("read");
        assert!(!chunk.gap);
        assert_eq!(chunk.bytes, b"CDEF");
    }

    #[test]
    fn a_gap_read_respects_the_max_bytes_bound() {
        let mut s = stream(8);
        s.append(b"0123456789ABCDEF");
        let chunk = s.read_from(0, 3).expect("gap read");
        assert!(chunk.gap);
        assert_eq!(chunk.offset, 8);
        assert_eq!(
            chunk.bytes, b"89A",
            "Gap 的第一帧同样要守住调用方的读取上限"
        );

        // 追平之后按 offset 续读,无缝接上且不重复。
        let next = s.read_from(11, 3).expect("follow-up");
        assert!(!next.gap);
        assert_eq!(next.bytes, b"BCD");
    }

    #[test]
    fn a_multi_byte_sequence_split_across_frames_survives_byte_for_byte() {
        // 宿主管道今天是"读到就 from_utf8",跨帧劈开的序列会被截断丢弃;
        // core 的流必须让解码成为消费方的事。emoji 是 4 字节,故意在中间劈开。
        let mut s = stream(1024);
        let emoji = "😀".as_bytes(); // [0xF0, 0x9F, 0x98, 0x80]
        assert_eq!(emoji.len(), 4);
        s.append(b"\x1b[31m"); // ANSI 序列也原样
        s.append(&emoji[..2]);
        s.append(&emoji[2..]);
        s.append("中".as_bytes());

        let chunk = s.read_from(0, 1024).expect("read");
        let mut expected = Vec::new();
        expected.extend_from_slice(b"\x1b[31m");
        expected.extend_from_slice(emoji);
        expected.extend_from_slice("中".as_bytes());
        assert_eq!(chunk.bytes, expected, "原始 bytes 必须逐字节保真");

        // 解码发生在消费方:拼起来才解得开,单独任何一帧都解不开。
        let text = String::from_utf8(chunk.bytes).expect("stream bytes are valid once joined");
        assert_eq!(text, "\x1b[31m😀中");
    }

    #[test]
    fn memory_stays_bounded_under_continuous_output() {
        let mut s = stream(4096);
        let block = vec![b'x'; 64 * 1024];
        for _ in 0..64 {
            s.append(&block); // 共 4 MiB,容量只有 4 KiB
        }
        assert!(
            s.retained.len() <= s.capacity,
            "有界缓冲不得随总输出线性增长:retained={} cap={}",
            s.retained.len(),
            s.capacity
        );
        assert_eq!(s.next_offset(), 64 * 1024 * 64);
    }

    #[test]
    fn identity_names_the_task_and_generation() {
        let s = stream(1024);
        assert_eq!(
            s.identity(),
            &TerminalIdentity {
                task_id: "t1".into(),
                generation: 3
            }
        );
        // 两代同任务是两条不同的流。
        assert_ne!(
            TerminalIdentity::new("t1", 3),
            TerminalIdentity::new("t1", 4)
        );
    }
}
