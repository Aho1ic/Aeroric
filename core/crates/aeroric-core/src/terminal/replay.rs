//! 重放映射(P3-03 切片四)。这一条模块存在的理由,是侦察底稿 §8 第 1 条点名的冲突:
//!
//! - core 的 offset 是**原始字节计数**([`super::stream::ByteStream`]);
//! - 旧前端 `src/terminalRingBuffer.ts` 的绝对偏移是**已解码字符串的 UTF-16 码元计数**
//!   (`data.length`),它记的是"我已经渲染了多少个字符单元"。
//!
//! 两者在非 ASCII 输出上必然不等,而**没有任何算术能把码元换算成字节**:一段字节流
//! 解码出多少码元,取决于从哪一字节开始解(半个字符开头的区段会被替换成 U+FFFD,
//! 而真正的增量解码会把这半截留给下一帧)。回放窗口又会把保留线推到字符中间,所以
//! "从流头累计的码元数"这份账在被压缩掉的那一段上根本无从恢复。
//!
//! 因此这里的规则是明确的、而不是"差不多能用"的:
//!
//! 1. **字节 offset 是唯一的续看凭证。**服务端只接受它,只回答它;每一帧都自带它的
//!    起始字节 offset(见 [`ReplayFrame`]),客户端把"我看到的下一段从哪个字节开始"
//!    记下来,回来时就交回这个数。
//! 2. **码元游标不许被换算。**`ResumePoint::UnitsFromStart` 存在,是为了让服务端
//!    对还在用旧游标的客户端**如实说"我不能无缝续看"**,而不是猜一个字节位置——
//!    猜了就会重复渲染或静默漏掉一段。答案一律是重置视图(从保留线整段重放)。
//! 3. **追平不上的都必须明说。**被压缩掉、流不在册、游标超过末端,各是自己的理由,
//!    调用方据此决定"重置视图"还是"这是接线 bug"。
//!
//! 一条已知且刻意保留的不对称:`ByteStream` 是在**第一次 publish** 时才建立的,所以
//! "运行刚启动、还没输出"与"这条流已被 FIFO 退场"在 core 里同一个答案(UnknownStream)。
//! 后果是可接受的:客户端在没有任何历史时重置视图 = 空视图,它下次仍按 offset 0 问,
//! 一旦有输出就无缝。要消掉这层歧义得给 `TerminalService` 加一个"开流"入口,并让宿主
//! reader 在建管道时调它 —— 那是宿主侧一处新调用 + 一次行号 pin 重定位,记给
//! `grill-me [terminal-core]` 定夺,不在这里顺手加。

use super::pty::TerminalService;
use super::stream::{StreamChunk, TerminalIdentity};

/// 订阅者交回来的续看凭证。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumePoint {
    /// core 的原始字节计数 —— 唯一能无缝续看的那种。
    Bytes(u64),
    /// 旧前端的绝对偏移(UTF-16 码元数)。**只配得到"重置视图"这个回答。**
    UnitsFromStart(u64),
}

/// 为什么不能无缝续看。三种情形调用方要做的事不同,所以不能压成一个布尔。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetReason {
    /// 请求的字节已被压缩丢弃:从保留线整段重放。
    Compacted { earliest: u64 },
    /// 这条流不在册(从未存在或已退场):没有任何历史可重放。
    UnknownStream,
    /// 游标超过流末端:客户端记的位置比服务端写过的还远,属接线缺陷。
    CursorBeyondStream { next_offset: u64 },
    /// 交回的是码元游标,而它无法被换算成字节。
    UnitsNotAnchorable,
}

/// 一次重放的安排结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayPlan {
    /// 从 `offset` 起无缝衔接,不重不漏。
    Seamless { offset: u64, bytes: Vec<u8> },
    /// 必须重置视图:`bytes` 是从 `from` 起现存的全部可重放内容(可能为空)。
    Reset {
        from: u64,
        bytes: Vec<u8>,
        reason: ResetReason,
    },
}

impl ReplayPlan {
    /// 这一帧之后订阅者该记住的续看凭证。无缝与重置都返回同一个算法:
    /// **交回"已交付字节数之后"的那个字节 offset**,这样重置视图的订阅者下一轮
    /// 走的也是字节车道,不必再猜。
    pub fn resume_after(&self) -> ResumePoint {
        let (from, len) = match self {
            ReplayPlan::Seamless { offset, bytes } => (*offset, bytes.len()),
            ReplayPlan::Reset { from, bytes, .. } => (*from, bytes.len()),
        };
        ResumePoint::Bytes(from.saturating_add(len as u64))
    }

    pub fn gap(&self) -> bool {
        matches!(self, ReplayPlan::Reset { .. })
    }
}

/// 交付给订阅者的一帧。字节 offset 是这一帧在流里的绝对位置,`bytes` 原样不透明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayFrame {
    pub byte_offset: u64,
    pub bytes: Vec<u8>,
}

impl ReplayFrame {
    pub fn from_chunk(chunk: &StreamChunk) -> Self {
        Self {
            byte_offset: chunk.offset,
            bytes: chunk.bytes.clone(),
        }
    }

    /// 下一帧该从哪里开始 —— 客户端唯一需要记住的数。
    pub fn next_byte_offset(&self) -> u64 {
        self.byte_offset.saturating_add(self.bytes.len() as u64)
    }

    /// 这一帧解码后的码元数。**视图侧的记账,不参与续看凭证**:它存在的意义是让
    /// "字节数 ≠ 码元数"这件事在两侧都有可比较的数,而不是给服务端一个换算依据。
    pub fn projected_units(&self) -> u64 {
        projected_utf16_units(&self.bytes)
    }
}

/// 一段字节按 lossy 解码会产出多少 UTF-16 码元。仅当这段从字符边界开始时,它才
/// 等于增量解码器对同一段的输出(用例把这条边界钉住)。
pub fn projected_utf16_units(bytes: &[u8]) -> u64 {
    String::from_utf8_lossy(bytes).encode_utf16().count() as u64
}

/// 唯一的规划入口:把订阅者的游标变成一次可执行的交付安排。
pub fn plan_replay(
    service: &TerminalService,
    identity: &TerminalIdentity,
    cursor: ResumePoint,
    max_bytes: usize,
) -> ReplayPlan {
    let units_cursor = match cursor {
        ResumePoint::Bytes(offset) => {
            return match service.read_from(identity, offset, max_bytes) {
                Ok(chunk) if chunk.gap => ReplayPlan::Reset {
                    from: chunk.offset,
                    bytes: chunk.bytes,
                    reason: ResetReason::Compacted {
                        earliest: chunk.offset,
                    },
                },
                Ok(chunk) => ReplayPlan::Seamless {
                    offset: chunk.offset,
                    bytes: chunk.bytes,
                },
                Err(super::stream::StreamError::UnknownStream) => ReplayPlan::Reset {
                    from: 0,
                    bytes: Vec::new(),
                    reason: ResetReason::UnknownStream,
                },
                Err(super::stream::StreamError::OutOfRange { next_offset }) => ReplayPlan::Reset {
                    from: next_offset,
                    bytes: Vec::new(),
                    reason: ResetReason::CursorBeyondStream { next_offset },
                },
            };
        }
        ResumePoint::UnitsFromStart(units) => units,
    };

    // 码元游标:能诚实回答的只有一种情形 —— 客户端还没渲染任何字符,而这条流的
    // 保留线仍在 0(整段历史都还在)。其余一律重置视图,绝不猜字节位置。
    let (earliest, _next) = match service.bounds(identity) {
        Some(bounds) => bounds,
        None => {
            return ReplayPlan::Reset {
                from: 0,
                bytes: Vec::new(),
                reason: ResetReason::UnknownStream,
            }
        }
    };
    if units_cursor == 0 && earliest == 0 {
        if let Ok(chunk) = service.read_from(identity, 0, max_bytes) {
            if !chunk.gap {
                return ReplayPlan::Seamless {
                    offset: chunk.offset,
                    bytes: chunk.bytes,
                };
            }
        }
    }
    let bytes = service
        .read_from(identity, earliest, max_bytes)
        .map(|chunk| chunk.bytes)
        .unwrap_or_default();
    ReplayPlan::Reset {
        from: earliest,
        bytes,
        reason: if units_cursor == 0 {
            ResetReason::Compacted { earliest }
        } else {
            ResetReason::UnitsNotAnchorable
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> TerminalIdentity {
        TerminalIdentity::new("replay-t", 1)
    }

    fn service_with(bytes: &[u8]) -> TerminalService {
        let service = TerminalService::new();
        service.publish(&identity(), bytes);
        service
    }

    #[test]
    fn a_byte_cursor_resumes_without_repeating_or_losing_a_single_byte() {
        let text = "中文😀tail".as_bytes().to_vec();
        let service = service_with(&text);
        let plan = plan_replay(&service, &identity(), ResumePoint::Bytes(0), 4);
        // 首帧只有 4 个字节:`中` 的前 3 字节 + 下一字符的第 1 字节。
        match &plan {
            ReplayPlan::Seamless { offset, bytes } => {
                assert_eq!(*offset, 0);
                assert_eq!(bytes.len(), 4);
            }
            other => panic!("从 0 起读必须无缝,实际 {other:?}"),
        }
        // 沿 `resume_after()` 一帧帧走,拼起来逐字节等于整条流。
        let mut cursor = plan.resume_after();
        let mut joined = Vec::new();
        joined.extend_from_slice(match &plan {
            ReplayPlan::Seamless { bytes, .. } => bytes,
            _ => unreachable!(),
        });
        while joined.len() < text.len() {
            let next = plan_replay(&service, &identity(), cursor, 3);
            match &next {
                ReplayPlan::Seamless { bytes, offset } => {
                    assert_eq!(*offset as usize, joined.len(), "帧与帧之间不得有洞");
                    joined.extend_from_slice(bytes);
                }
                other => panic!("中途不该重置:{other:?}"),
            }
            cursor = next.resume_after();
        }
        assert_eq!(joined, text, "按 3 字节切帧也要一字不差");
    }

    #[test]
    fn a_cursor_landing_mid_character_is_still_answered_in_bytes() {
        // 半个字符的 offset 是这条设计的试金石:服务端不重新对齐、不替换、也不猜。
        let emoji = "😀".as_bytes().to_vec();
        let service = service_with(&emoji);
        let plan = plan_replay(&service, &identity(), ResumePoint::Bytes(2), 1024);
        match &plan {
            ReplayPlan::Seamless { offset, bytes } => {
                assert_eq!(*offset, 2);
                assert_eq!(bytes, &emoji[2..], "从第 2 字节起原样给,不做字符对齐");
            }
            other => panic!("保留区之内不该重置:{other:?}"),
        }
        // 拿这一帧的锚点回来续读,正好走到末端且不多不少。
        assert_eq!(plan.resume_after(), ResumePoint::Bytes(emoji.len() as u64));
    }

    #[test]
    fn utf16_units_are_neither_additive_nor_reversible_across_arbitrary_cuts() {
        // 本节设计的机械依据。`😀中` = 7 字节,整段 lossy 解码是 3 个码元。
        // 实测(不是推算)每个切点的分帧之和:
        //   cut 0→3  1→5  2→4  3→3  4→3  5→5  6→4  7→3
        // 两条结论都在这里钉住:
        //   ① 码元数在任意字节切帧下**不保证可加**(1/2/5/6 都对不上);
        //   ② 更糟的是**对上了也不能反推**——cut 3 落在 emoji 中间却同样得 3。
        // 所以"客户端手里的码元数 ⇒ 服务端该从哪个字节继续"这个换算根本不存在,
        // 不是麻烦而是数学上不成立:服务端只能拒收码元游标(见 a_utf16_cursor_...)。
        let whole = "😀中".as_bytes();
        assert_eq!(whole.len(), 7);
        let one_shot = projected_utf16_units(whole);
        assert_eq!(one_shot, 3);

        let sums: Vec<u64> = (0..=whole.len())
            .map(|cut| projected_utf16_units(&whole[..cut]) + projected_utf16_units(&whole[cut..]))
            .collect();
        assert_eq!(
            sums,
            vec![3, 5, 4, 3, 3, 5, 4, 3],
            "每个切点的分帧码元之和(实测口径变了说明解码器行为变了)"
        );
        for cut in [1usize, 2, 5, 6] {
            assert_ne!(
                sums[cut], one_shot,
                "切点 {cut} 应当破坏可加性,否则这条用例没测到东西"
            );
        }
        // ②的对偶:切点 3 在 emoji 内部,却与整段相等 —— 相等不能反推对齐。
        assert_eq!(sums[3], one_shot);
        assert!(
            std::str::from_utf8(&whole[..3]).is_err(),
            "cut 3 根本不是字符边界"
        );
    }

    #[test]
    fn a_utf16_cursor_never_becomes_a_guess() {
        let service = service_with("中文😀".as_bytes());
        // 字节数 12、码元数 4:两者不等的地方,正是"按码元猜字节"会错位的地方。
        let plan = plan_replay(&service, &identity(), ResumePoint::UnitsFromStart(2), 1024);
        match &plan {
            ReplayPlan::Reset { from, reason, .. } => {
                assert_eq!(*from, 0);
                assert_eq!(*reason, ResetReason::UnitsNotAnchorable);
            }
            other => panic!("码元游标绝不能被当成无缝续看:{other:?}"),
        }
        assert!(plan.gap());
    }

    #[test]
    fn a_zero_unit_cursor_on_a_fresh_stream_is_the_one_honest_seamless_case() {
        let service = service_with(b"hello");
        let plan = plan_replay(&service, &identity(), ResumePoint::UnitsFromStart(0), 1024);
        assert_eq!(
            plan,
            ReplayPlan::Seamless {
                offset: 0,
                bytes: b"hello".to_vec()
            }
        );
    }

    #[test]
    fn compaction_resets_to_the_catchup_point_and_the_cursor_then_runs_on_bytes() {
        let service = TerminalService::new();
        let id = identity();
        let block = vec![b'x'; super::super::pty::TERMINAL_REPLAY_WINDOW_BYTES + 1024];
        service.publish(&id, &block);
        let (earliest, next) = service.bounds(&id).expect("in registry");
        assert!(earliest > 0, "超过窗口却没压缩?");
        assert_eq!(next, block.len() as u64);

        let plan = plan_replay(&service, &id, ResumePoint::Bytes(0), 4096);
        match &plan {
            ReplayPlan::Reset {
                from,
                reason,
                bytes,
                ..
            } => {
                assert_eq!(*from, earliest);
                assert_eq!(*reason, ResetReason::Compacted { earliest });
                assert_eq!(bytes.len(), 4096, "重置的那一帧同样要守住读取上限");
            }
            other => panic!("落后一整段必须重置:{other:?}"),
        }
        // 重置之后拿着字节游标回来,就是无缝的。
        let follow = plan_replay(&service, &id, plan.resume_after(), 4096);
        assert!(
            matches!(follow, ReplayPlan::Seamless { .. }),
            "重置后接着走不该再重置:{follow:?}"
        );
    }

    #[test]
    fn unknown_stream_and_a_cursor_past_the_end_are_different_answers() {
        let service = TerminalService::new();
        let missing = TerminalIdentity::new("never-published", 9);
        let plan = plan_replay(&service, &missing, ResumePoint::Bytes(0), 1024);
        assert_eq!(
            plan,
            ReplayPlan::Reset {
                from: 0,
                bytes: Vec::new(),
                reason: ResetReason::UnknownStream
            }
        );

        let service = service_with(b"abc");
        let ahead = plan_replay(&service, &identity(), ResumePoint::Bytes(4), 1024);
        assert_eq!(
            ahead,
            ReplayPlan::Reset {
                from: 3,
                bytes: Vec::new(),
                reason: ResetReason::CursorBeyondStream { next_offset: 3 }
            },
            "客户端比服务端写得还远,要报成接线缺陷而不是'没有新数据'"
        );
        // 恰好等于末端是合法的追平:空的一帧,无缝。
        assert_eq!(
            plan_replay(&service, &identity(), ResumePoint::Bytes(3), 1024),
            ReplayPlan::Seamless {
                offset: 3,
                bytes: Vec::new()
            }
        );
    }

    #[test]
    fn utf16_units_counts_code_units_not_bytes_or_chars() {
        assert_eq!(projected_utf16_units("abc".as_bytes()), 3);
        assert_eq!(projected_utf16_units("中".as_bytes()), 1, "BMP 内一个码元");
        assert_eq!(projected_utf16_units("😀".as_bytes()), 2, "星外两个码元");
        assert_eq!(projected_utf16_units(b"\xff"), 1, "非法字节是一个 U+FFFD");
    }
}
