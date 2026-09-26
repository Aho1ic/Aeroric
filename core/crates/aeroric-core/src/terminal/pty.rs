//! 终端服务(P3-03 Action 1):原始字节流的唯一归属地,加上控制租约的分发口。
//!
//! 分工口径:core 先收下**原始 bytes**(每条流一个 [`ByteStream`],身份是
//! `(task_id, generation)`),旧客户端拿到的字符串只是这条流的**增量解码投影**
//! ([`Utf8Projection`])。宿主今天的 reader 是"读到就 `from_utf8`",所以这条
//! bytes 车道必须先于解码存在。
//!
//! 内存有界是硬要求(验收:"有界内存不随总输出线性增长"):
//! - 每条流只保留最近 [`TERMINAL_REPLAY_WINDOW_BYTES`],更早的请求得到 Gap;
//! - **已收尾**的流按插入序 FIFO 退场;
//! - **仍在写**的流绝不静默退场——那会让新建的流从 offset 0 谎报"历史完整";
//!   配额用尽时新发布被拒绝并计数,调用方记诊断,但绝不阻塞 PTY reader。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;

use super::lease::{ControlLease, LeaseError, LeaseHandle};
use super::stream::{ByteStream, StreamChunk, StreamError, TerminalIdentity};

/// 每条流的回放窗口(保留最近多少原始字节)。
pub const TERMINAL_REPLAY_WINDOW_BYTES: usize = 1024 * 1024;
/// 已收尾流的最长保留条数(超过即 FIFO 退场)。
pub const TERMINAL_MAX_FINISHED_STREAMS: usize = 64;
/// 同时在写的流上限;超过只拒绝新发布并计数,不静默丢弃已有历史。
pub const TERMINAL_MAX_LIVE_STREAMS: usize = 256;

/// 一次发布的结局。宿主只在 `RefusedFull` 时记诊断——它绝不能拖住 PTY reader。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishOutcome {
    Accepted,
    /// 在写的流已达上限,这条新流一个字节都没进缓冲。
    RefusedFull,
}

#[derive(Debug, Default)]
struct State {
    streams: HashMap<TerminalIdentity, Arc<Mutex<ByteStream>>>,
    leases: HashMap<TerminalIdentity, ControlLease>,
    /// 插入顺序,用于 FIFO 退场与"哪些流还在写"的判定。
    order: VecDeque<TerminalIdentity>,
    finished: HashMap<TerminalIdentity, u64>,
    retired_total: u64,
    refusals: u64,
}

impl State {
    /// 只在**已收尾**的流里 FIFO 退场,保证还在写的流不会被静默清空。
    fn evict_finished_locked(&mut self) {
        while self.finished.len() > TERMINAL_MAX_FINISHED_STREAMS {
            match self
                .order
                .iter()
                .position(|identity| self.finished.contains_key(identity))
            {
                Some(index) => {
                    let identity = self.order.remove(index).expect("index came from order");
                    self.streams.remove(&identity);
                    self.leases.remove(&identity);
                    self.finished.remove(&identity);
                    self.retired_total += 1;
                }
                None => break,
            }
        }
    }
}

/// 终端服务。`Send + Sync`,宿主挂在任务状态上,reader 与订阅者共用一份。
#[derive(Default)]
pub struct TerminalService {
    state: Mutex<State>,
}

impl TerminalService {
    pub fn new() -> Self {
        Self::default()
    }

    /// reader 的主入口:先落原始字节。返回 `RefusedFull` 时调用方记诊断并继续读。
    pub fn publish(&self, identity: &TerminalIdentity, bytes: &[u8]) -> PublishOutcome {
        if bytes.is_empty() {
            return PublishOutcome::Accepted;
        }
        let stream = {
            let mut state = self.state.lock();
            if let Some(existing) = state.streams.get(identity) {
                Arc::clone(existing)
            } else {
                let live = state.order.len() - state.finished.len();
                if live >= TERMINAL_MAX_LIVE_STREAMS {
                    state.refusals += 1;
                    return PublishOutcome::RefusedFull;
                }
                state.evict_finished_locked();
                let created = Arc::new(Mutex::new(ByteStream::new(
                    identity.clone(),
                    TERMINAL_REPLAY_WINDOW_BYTES,
                )));
                state.streams.insert(identity.clone(), Arc::clone(&created));
                state.order.push_back(identity.clone());
                created
            }
        };
        stream.lock().append(bytes);
        PublishOutcome::Accepted
    }

    /// 回放/订阅入口:从 `offset` 起最多读 `max_bytes`。
    pub fn read_from(
        &self,
        identity: &TerminalIdentity,
        offset: u64,
        max_bytes: usize,
    ) -> Result<StreamChunk, StreamError> {
        match self.state.lock().streams.get(identity).cloned() {
            Some(stream) => Ok(stream.lock().read_from(offset, max_bytes)?),
            None => Err(StreamError::UnknownStream),
        }
    }

    /// 这条流最早的可用 offset 与末端(订阅者据此判断要不要重置视图)。
    pub fn bounds(&self, identity: &TerminalIdentity) -> Option<(u64, u64)> {
        let stream = self.state.lock().streams.get(identity).cloned()?;
        let stream = stream.lock();
        Some((stream.earliest_offset(), stream.next_offset()))
    }

    /// 取得该流的输入/resize 控制权(顶替任何旧持有者)。
    pub fn acquire_control(&self, identity: &TerminalIdentity) -> LeaseHandle {
        let mut state = self.state.lock();
        let lease = state.leases.entry(identity.clone()).or_default();
        lease.acquire()
    }

    /// 显式释放。`false` = 这把 handle 已经不是持有者(被顶替/已释放/流已退场)。
    pub fn release_control(&self, identity: &TerminalIdentity, handle: &LeaseHandle) -> bool {
        match self.state.lock().leases.get_mut(identity) {
            Some(lease) => lease.release(handle).is_ok(),
            None => false,
        }
    }

    /// 写操作(输入/resize)的统一闸门。没有租约记录 = 无人持有 = 拒绝。
    pub fn can_write(
        &self,
        identity: &TerminalIdentity,
        handle: &LeaseHandle,
    ) -> Result<(), LeaseError> {
        let state = self.state.lock();
        match state.leases.get(identity) {
            Some(lease) => lease.write_gate(handle),
            None => Err(LeaseError::Superseded),
        }
    }

    /// 这一代运行的输出已经结束:允许它日后被 FIFO 退场。
    ///
    /// 不在 reader 退出时立刻删——重连的订阅者还要靠这段窗口追平。
    pub fn mark_finished(&self, identity: &TerminalIdentity) {
        let mut state = self.state.lock();
        let end = state
            .streams
            .get(identity)
            .map(|stream| stream.lock().next_offset())
            .unwrap_or(0);
        state.finished.insert(identity.clone(), end);
        state.evict_finished_locked();
    }

    /// 被 FIFO 退场过的流数量(静默退场只可能发生在已收尾的流上)。
    pub fn retired_count(&self) -> u64 {
        self.state.lock().retired_total
    }

    /// 因在写流配额用尽而被拒绝的发布次数。
    pub fn refusals(&self) -> u64 {
        self.state.lock().refusals
    }

    pub fn stream_count(&self) -> usize {
        self.state.lock().streams.len()
    }
}

/// 原始字节 → 字符串的**增量**投影。它是旧 reader 里那段 `leftover` 逻辑的替身,
/// 但修掉两条会真实咬人的行为:
///
/// 1. 确定非法的字节序列以前被无限期扣在 `leftover` 里:此后每读一块都重新判定
///    失败,终端从此卡住且 `leftover` 无上限增长。现在按 `from_utf8_lossy` 的口径
///    发一个 U+FFFD 并继续前进。
/// 2. 进程在半个字符中间退出时,那半截以前被直接丢掉(用户最后一段输出里的字符
///    静默消失)。现在 [`Utf8Projection::finish`] 把尾巴交出去。
///
/// 合法 UTF-8 流上两者逐字符相同(由测试钉住);而原始字节的真相已经在
/// [`TerminalService`] 的流里,投影丢了也不会丢信息。
#[derive(Default)]
pub struct Utf8Projection {
    pending: Vec<u8>,
}

impl Utf8Projection {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂进一块原始字节,返回这一块能确定解出的字符串(可能为空)。
    pub fn push(&mut self, bytes: &[u8]) -> String {
        if self.pending.is_empty() {
            // 常见路径:整块合法,一次判定就返回。
            if let Ok(text) = std::str::from_utf8(bytes) {
                return text.to_owned();
            }
        }
        let mut out = String::new();
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(bytes);
        let mut pos = 0usize;
        loop {
            match std::str::from_utf8(&buf[pos..]) {
                Ok(text) => {
                    out.push_str(text);
                    pos = buf.len();
                    break;
                }
                Err(error) => {
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        // valid_up_to 保证这段是合法 UTF-8,解不出来就是实现缺陷,
                        // 宁可丢一个字符也绝不让 reader panic 拖垮任务输出。
                        if let Ok(text) = std::str::from_utf8(&buf[pos..pos + valid]) {
                            out.push_str(text);
                        }
                        pos += valid;
                    }
                    match error.error_len() {
                        // 尾部是"可能合法但被截断"的前缀:留到下一次。
                        None => break,
                        // 确定非法:一个替换字符,跳过这几个字节继续扫。
                        Some(bad) => {
                            out.push(char::REPLACEMENT_CHARACTER);
                            pos += bad.min(buf.len() - pos);
                        }
                    }
                }
            }
        }
        self.pending = buf.split_off(pos);
        out
    }

    /// 流结束:交出还扣着的尾巴。
    pub fn finish(&mut self) -> String {
        let tail = std::mem::take(&mut self.pending);
        if tail.is_empty() {
            return String::new();
        }
        String::from_utf8_lossy(&tail).into_owned()
    }

    /// 还扣着多少字节(诊断用;不得随输出无上限增长)。
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(n: u64) -> TerminalIdentity {
        TerminalIdentity::new(format!("task-{n}"), 1)
    }

    #[test]
    fn published_bytes_are_readable_from_offset_zero_in_order() {
        let service = TerminalService::new();
        let id = identity(1);
        assert_eq!(service.publish(&id, b"abc"), PublishOutcome::Accepted);
        assert_eq!(
            service.publish(&id, "中".as_bytes()),
            PublishOutcome::Accepted
        );
        let chunk = service.read_from(&id, 0, 4096).expect("read");
        assert_eq!(chunk.bytes, b"abc\xe4\xb8\xad".to_vec());
        assert!(!chunk.gap);
        // "abc" 3 字节 +"中" 3 字节 = 6 个**原始**字节,不是 4 个字符。
        assert_eq!(service.bounds(&id), Some((0, 6)));
    }

    #[test]
    fn reading_an_unregistered_stream_is_not_the_same_as_reading_past_the_end() {
        let service = TerminalService::new();
        assert_eq!(
            service.read_from(&identity(404), 0, 8),
            Err(StreamError::UnknownStream),
            "退场/从未存在的流必须让订阅方知道\"历史已截断\",而不是当成读到末端"
        );
    }

    #[test]
    fn the_two_generations_of_one_task_are_two_different_streams() {
        let service = TerminalService::new();
        let first = TerminalIdentity::new("t", 1);
        let second = TerminalIdentity::new("t", 2);
        service.publish(&first, b"old");
        service.publish(&second, b"new");
        assert_eq!(
            service.read_from(&first, 0, 8).unwrap().bytes,
            b"old".to_vec()
        );
        assert_eq!(
            service.read_from(&second, 0, 8).unwrap().bytes,
            b"new".to_vec()
        );
    }

    #[test]
    fn control_lease_is_per_stream_and_a_superseded_holder_cannot_write() {
        let service = TerminalService::new();
        let front = identity(1);
        let back = identity(2);
        let front_handle = service.acquire_control(&front);
        let back_handle = service.acquire_control(&back);
        assert!(service.can_write(&front, &front_handle).is_ok());
        assert!(service.can_write(&back, &back_handle).is_ok());

        let renewed = service.acquire_control(&front);
        assert_eq!(
            service.can_write(&front, &front_handle),
            Err(LeaseError::Superseded)
        );
        assert!(service.can_write(&front, &renewed).is_ok());
        assert!(service.can_write(&back, &back_handle).is_ok());
        assert!(service.release_control(&front, &renewed));
        assert_eq!(
            service.can_write(&front, &renewed),
            Err(LeaseError::Superseded)
        );
    }

    #[test]
    fn a_lease_from_one_stream_cannot_write_another() {
        let service = TerminalService::new();
        let handle = service.acquire_control(&identity(9));
        assert_eq!(
            service.can_write(&identity(10), &handle),
            Err(LeaseError::Superseded),
            "拿别的流的租约写这一条流必须被拒"
        );
        assert!(!service.release_control(&identity(10), &handle));
    }

    #[test]
    fn a_full_registry_refuses_new_streams_instead_of_wiping_a_live_one() {
        // 还在写的流被静默退场的话,新建的那条会从 offset 0 开始计数——订阅者就会
        // 以为"历史完整"。所以配额用尽时拒绝并计数,已有流的 offset 一格不动。
        let service = TerminalService::new();
        for n in 0..TERMINAL_MAX_LIVE_STREAMS as u64 {
            assert_eq!(
                service.publish(&identity(n), b"x"),
                PublishOutcome::Accepted
            );
        }
        assert_eq!(service.refusals(), 0);
        assert_eq!(
            service.publish(&identity(9_000), b"x"),
            PublishOutcome::RefusedFull
        );
        assert_eq!(service.refusals(), 1);
        assert_eq!(
            service.read_from(&identity(9_000), 0, 8),
            Err(StreamError::UnknownStream)
        );
        assert_eq!(service.bounds(&identity(0)), Some((0, 1)));
        assert_eq!(service.stream_count(), TERMINAL_MAX_LIVE_STREAMS);
    }

    #[test]
    fn finished_streams_retire_in_insertion_order_without_touching_live_ones() {
        let service = TerminalService::new();
        let live = identity(1);
        service.publish(&live, b"keep-me");
        let oldest = identity(0);
        service.publish(&oldest, b"gone");
        service.mark_finished(&oldest);
        for n in 0..=TERMINAL_MAX_FINISHED_STREAMS as u64 {
            let id = identity(100 + n);
            service.publish(&id, b"x");
            service.mark_finished(&id);
        }
        assert!(
            service.retired_count() >= 1,
            "超额的已收尾流必须退场,否则内存随历史任务数线性增长"
        );
        assert_eq!(service.bounds(&live), Some((0, 7)), "还在写的流绝不被退场");
        // 在册的已收尾流 66 条、上限 64 → 按插入序退场两条:最早那条(比 live 还早)
        // 和第二早的。最近收尾的必须还在,重连的订阅者才追得平。
        assert_eq!(service.retired_count(), 2);
        assert_eq!(
            service.read_from(&oldest, 0, 8),
            Err(StreamError::UnknownStream),
            "退场从插入序最早的已收尾流开始"
        );
        assert_eq!(
            service.read_from(&identity(100), 0, 8),
            Err(StreamError::UnknownStream)
        );
        assert!(
            service
                .read_from(&identity(100 + TERMINAL_MAX_FINISHED_STREAMS as u64), 0, 8)
                .is_ok(),
            "最后收尾的那条还在窗口里"
        );
    }

    #[test]
    fn projection_equals_a_one_shot_decode_of_the_same_valid_bytes() {
        let text = "\u{1b}[31m中文😀 tail\u{1b}[0m\n";
        let bytes = text.as_bytes();
        let mut chunked = String::new();
        let mut projection = Utf8Projection::new();
        for piece in bytes.chunks(3) {
            chunked.push_str(&projection.push(piece));
        }
        chunked.push_str(&projection.finish());
        assert_eq!(chunked, text, "3 字节一切也必须原样解出(跨帧多字节)");
        assert_eq!(projection.pending_len(), 0);
    }

    #[test]
    fn a_split_multi_byte_sequence_is_held_only_until_it_can_be_decoded() {
        let mut projection = Utf8Projection::new();
        let emoji = "😀".as_bytes();
        assert_eq!(projection.push(&emoji[..2]), "");
        assert_eq!(projection.pending_len(), 2, "截断前缀留在缓冲,不该增长");
        assert_eq!(projection.push(&emoji[2..]), "😀");
        assert_eq!(projection.pending_len(), 0);
    }

    #[test]
    fn definitively_invalid_bytes_do_not_stall_the_stream() {
        // 旧实现的形状:0xC0 永远解不出 valid_up_to>0 的后继,于是终端卡住、
        // leftover 无上限增长。新实现发一个替换字符并继续前进。
        let mut projection = Utf8Projection::new();
        let mut out = String::new();
        for _ in 0..512 {
            out.push_str(&projection.push(&[0xC0, 0x80]));
        }
        out.push_str(&projection.finish());
        assert_eq!(projection.pending_len(), 0, "非法字节不得被无限期扣住");
        assert_eq!(
            out.chars()
                .filter(|c| *c == char::REPLACEMENT_CHARACTER)
                .count(),
            1024
        );
        assert!(out.len() < 4 * 1024);
    }

    #[test]
    fn the_tail_of_a_truncated_final_character_is_not_dropped() {
        // 旧实现:进程在半个字符中间退出 → 那半截静默消失。
        let mut projection = Utf8Projection::new();
        let emoji = "😀".as_bytes();
        assert_eq!(projection.push(b"done"), "done");
        assert_eq!(projection.push(&emoji[..3]), "");
        let tail = projection.finish();
        assert!(
            !tail.is_empty(),
            "流结束时扣着的字节必须交出去,不能静默丢掉"
        );
        assert_eq!(projection.pending_len(), 0);
    }

    #[test]
    fn nul_bytes_pass_through_untouched() {
        let mut projection = Utf8Projection::new();
        let text = projection.push(b"a\0b\0");
        assert_eq!(text, "a\0b\0", "NUL 是合法 UTF-8,投影不得吃掉");
        assert_eq!(projection.finish(), "");
    }
}
