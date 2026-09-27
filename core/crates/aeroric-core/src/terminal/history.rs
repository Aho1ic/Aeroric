//! 终端历史的压实策略(P3-03 Action 4)——"有界 replay 与 history compaction 共享一个 owner"。
//!
//! 两条**有界保留面**如今都在 core `terminal/` 落户:内存侧是 [`super::stream`] 的
//! `ByteStream`(1 MiB 窗口、显式 Gap、字节是真相、**不在截断点重新对齐**——那是消费方
//! 投影的职责);磁盘侧是本模块的历史压实表(保留 [`HISTORY_MAX_BYTES`]、允许超到
//! [`HISTORY_COMPACT_SLACK_BYTES`] 再压回)。owner 的含义是**策略只有一份实现**:原先
//! 内联在宿主 `storage.rs` 的 `utf8_tail_bytes`、str 版 `utf8_tail` 与读侧的续字节跳过
//! 是三份近似重复,这里收敛为 [`utf8_tail_bytes`] 与
//! [`skip_leading_utf8_continuations`] 两个纯函数;文件 I/O 与目录巡检仍是宿主的事
//! ——core 给决策,宿主给事实,与 exit/input 的分工同一条线。
//!
//! 两条面的**差异是刻意的,不要"统一"**:replay 窗口按原始字节切,落在字符中间就如实
//! 交出中间字节(有集成用例钉着);磁盘历史按字符边界对齐后才落盘,因为它的读者直接把
//! 字节当文本。它们共享的只有"保留尾部、丢头部、对丢失如实"这条形状,不是同一套参数。

/// 磁盘终端历史的保留上限(宿主 `MAX_TERMINAL_HISTORY_BYTES` 的唯一来源)。
pub const HISTORY_MAX_BYTES: usize = 8 * 1024 * 1024;

/// 压实前允许文件长到上限之上多少。
///
/// 为什么必须有余量:压实是 O(文件) 的(读末尾 + 原子重写整份)。若一到上限就压实,
/// 之后**每一个** PTY 片段(读缓冲 32 KB,活跃 TUI 每秒几十个)都要读 8 MB、转换 8 MB、
/// 重写 8 MB——宿主实测三个 reader 线程各约 40% CPU 耗在这里,同时每次瞬时分配约
/// 24 MB,RSS 随碎片台阶式上涨。留出余量后代价摊薄成"每写满 slack 一次 O(文件)",
/// 接近 O(1)。4 MB:文件上界变成 12 MB(磁盘代价可接受),而压实频率降到原来的约
/// 1/128(按 32 KB 片段算)。读侧本来只取末 [`HISTORY_MAX_BYTES`],读语义不变。
pub const HISTORY_COMPACT_SLACK_BYTES: usize = 4 * 1024 * 1024;

/// 一条历史保留策略:压实的判定与"压回多少"都从这里算,调用方不再各自内联算术。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryRetention {
    pub max_bytes: usize,
    pub slack_bytes: usize,
}

impl HistoryRetention {
    /// 宿主沿用至今的那组参数。名字里的 legacy 是"迁移来源",不是"待换"。
    pub const LEGACY: HistoryRetention = HistoryRetention {
        max_bytes: HISTORY_MAX_BYTES,
        slack_bytes: HISTORY_COMPACT_SLACK_BYTES,
    };

    /// 高水位:超过它才值得付一次 O(文件) 的压实。
    pub fn high_water(&self) -> u64 {
        (self.max_bytes as u64).saturating_add(self.slack_bytes as u64)
    }

    /// "该追加还是该压实"的唯一判定:追加当且仅当 `current + incoming ≤ 高水位`。
    ///
    /// 追加路径(current=文件长,incoming=新片段)与目录巡检路径(incoming=0,只问
    /// "文件超标没有")是**同一个问题**——这正是两条调用点共用一张表的理由。
    pub fn needs_compaction(&self, current_len: u64, incoming_len: usize) -> bool {
        current_len.saturating_add(incoming_len as u64) > self.high_water()
    }

    /// 压实一次后,磁盘尾部应保留多少字节再接上切好的新片段。
    ///
    /// 压实回 [`HISTORY_MAX_BYTES`](不是高水位)——压实后要留出整个 slack 窗口供后续
    /// 纯 append 消费,这才是摊薄成立的前提。
    pub fn disk_tail_bytes(&self, incoming_kept_len: usize) -> u64 {
        self.max_bytes.saturating_sub(incoming_kept_len) as u64
    }
}

/// 取末 `max_bytes` 字节,并把起点推到 UTF-8 字符边界。
///
/// 只推起点、不做校验:传进来的两段(文件尾部 + 新片段)各自已在字符边界上,拼接后
/// 仍然是边界对齐的,这里唯一的职责是二次裁剪后不要切在字符中间。对合法 UTF-8,它与
/// "逐字符 `is_char_boundary` 推进"逐字节等价(有测试拿 std 判定当 oracle);对非法
/// 字节,连续续字节的极端输入靠调用方给上限(见
/// [`skip_leading_utf8_continuations`]),本函数保持宿主原先"无上限推进"的语义不变。
pub fn utf8_tail_bytes(bytes: &[u8], max_bytes: usize) -> &[u8] {
    if bytes.len() <= max_bytes {
        return bytes;
    }
    let mut start = bytes.len() - max_bytes;
    while start < bytes.len() && (bytes[start] & 0b1100_0000) == 0b1000_0000 {
        start += 1;
    }
    &bytes[start..]
}

/// 读侧的"掐头去续字节":返回应跳过的字节数,至多 `max_skip` 个。
///
/// 文件被拦腰读取时,起点处至多挂着一个不完整序列的**前几个**续字节(合法 UTF-8 的
/// 序列至多 4 字节,所以宿主读路径传 4);上限同时保证"整段都是续字节的垃圾"不会被
/// 无限推进。语义与宿主原先内联的 `take(4).take_while(…)` 完全一致,泛化出上限只是
/// 为了让写侧与读侧能共用同一份"什么是续字节"的定义。
pub fn skip_leading_utf8_continuations(bytes: &[u8], max_skip: usize) -> usize {
    bytes
        .iter()
        .take(max_skip)
        .take_while(|b| (**b & 0b1100_0000) == 0b1000_0000)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tail_within_the_cap_is_returned_verbatim() {
        let bytes = b"abc".as_slice();
        for cap in [0usize, 1, 2, 3, 4, 128] {
            let expected_len = bytes.len().min(cap);
            assert_eq!(
                utf8_tail_bytes(bytes, cap),
                &bytes[bytes.len() - expected_len..]
            );
        }
    }

    #[test]
    fn the_cut_never_lands_inside_a_multi_byte_sequence() {
        // "你"(3 B) + "😀"(4 B) + "中"(3 B) = 10 B;逐个切点检查起点都落在字符边界上。
        let text = "你😀中";
        let bytes = text.as_bytes();
        for cap in 0..bytes.len() {
            let kept = utf8_tail_bytes(bytes, cap);
            let start = bytes.len() - kept.len();
            assert!(
                text.is_char_boundary(start),
                "cap={cap} 的切点 {start} 落在字符中间"
            );
            assert!(
                kept.len() + 3 >= cap.min(10) || start == 0,
                "边界推进最多丢一个序列"
            );
        }
        // 足够大的 cap 就是整段原样。
        assert_eq!(utf8_tail_bytes(bytes, 10), bytes);
    }

    #[test]
    fn the_bytes_cut_matches_the_char_boundary_oracle_on_valid_utf8() {
        // 迁移前宿主 str 版的算法(is_char_boundary 推进)原样重放当 oracle:
        // 对合法 UTF-8,字节版必须与它逐字节相等。
        fn legacy_str_tail(content: &str, max_bytes: usize) -> &str {
            if content.len() <= max_bytes {
                return content;
            }
            let mut start = content.len() - max_bytes;
            while start < content.len() && !content.is_char_boundary(start) {
                start += 1;
            }
            &content[start..]
        }
        for text in ["hello", "你好", "😀中😀", "a😀b你c", "你好😀世界OK"] {
            let bytes = text.as_bytes();
            for cap in 0..=bytes.len() + 2 {
                assert_eq!(
                    utf8_tail_bytes(bytes, cap),
                    legacy_str_tail(text, cap).as_bytes(),
                    "text={text:?} cap={cap}"
                );
            }
        }
    }

    #[test]
    fn the_read_side_skip_is_capped_and_matches_the_legacy_take4() {
        // 宿主读路径原先内联的 take(4).take_while(…) 原样重放当 oracle。
        fn legacy_skip(bytes: &[u8]) -> usize {
            bytes
                .iter()
                .take(4)
                .take_while(|b| (**b & 0b1100_0000) == 0b1000_0000)
                .count()
        }
        let cases: Vec<Vec<u8>> = vec![
            vec![],
            vec![b'a'],
            "你".as_bytes()[1..].to_vec(), // "你" 的后两个续字节
            "😀".as_bytes()[1..].to_vec(), // emoji 的后三个续字节
            vec![0b1000_0000; 6],          // 全是续字节的垃圾:上限必须封住
            "ab你".as_bytes().to_vec(),    // 起点不在续字节:0
        ];
        for bytes in &cases {
            assert_eq!(
                skip_leading_utf8_continuations(bytes, 4),
                legacy_skip(bytes)
            );
        }
        assert_eq!(skip_leading_utf8_continuations(&[0b1000_0000; 6], 4), 4);
        assert_eq!(
            skip_leading_utf8_continuations(&[0b1000_0000; 6], usize::MAX),
            6
        );
    }

    #[test]
    fn the_high_water_table_says_append_only_below_the_line() {
        let retention = HistoryRetention::LEGACY;
        let high = retention.high_water();
        assert_eq!(
            high,
            (HISTORY_MAX_BYTES + HISTORY_COMPACT_SLACK_BYTES) as u64
        );
        // 恰好压线:追加(不压实)。超一个字节:压实。
        assert!(!retention.needs_compaction(high, 0));
        assert!(retention.needs_compaction(high, 1));
        assert!(retention.needs_compaction(0, high as usize + 1));
        assert!(!retention.needs_compaction(0, high as usize));
    }

    #[test]
    fn compaction_targets_the_cap_not_the_high_water() {
        let retention = HistoryRetention::LEGACY;
        // 追加路径:新片段切尾后占多少,磁盘就只保留 max - 那么多。
        let incoming = utf8_tail_bytes("😀中".as_bytes(), 5);
        let disk_tail = retention.disk_tail_bytes(incoming.len());
        assert_eq!(
            disk_tail as usize + incoming.len(),
            retention.max_bytes,
            "压实回的是 max,不是 max + slack"
        );
        // 目录巡检路径(incoming=0):整份按 max 重写。
        assert_eq!(retention.disk_tail_bytes(0), HISTORY_MAX_BYTES as u64);
    }

    #[test]
    fn the_directory_pass_and_the_append_pass_answer_the_same_question() {
        // 同一张表:目录巡检的"超标才压"就是 incoming=0 的追加判定。
        let retention = HistoryRetention::LEGACY;
        let oversized = retention.high_water() + 1;
        let just_fit = retention.high_water();
        assert!(retention.needs_compaction(oversized, 0));
        assert!(!retention.needs_compaction(just_fit, 0));
    }
}
