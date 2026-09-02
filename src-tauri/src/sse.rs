//! SSE 帧分隔符定位 —— 全 crate 五个流式消费方的共享实现。
//!
//! 这个函数原先在 `local_router` 的 `chat_bridge` / `server` / `usage` /
//! `inline_tool_calls`,以及 `dsh_webui` 的 `take_sse_frame` 里各存一份。前三份
//! 逐字节相同(`windows()` 版),`server` 与 `dsh_webui` 那两份是另外两种写法
//! (逐位置扫描 / match 臂展开)。
//!
//! 合并前用 `{\r, \n, x}` 字母表上长度 0..=9 的全部 29524 个串穷举比对过:
//! 分隔符定位 0 差异;把缓冲区反复排空(50056 次 `drain` 之后再定位)也是 0 差异
//! —— 一次调用等价不等于 `drain` 之后每一帧都等价,所以两项都验了。
//!
//! 住在 crate 根而不是 `local_router` 下:`dsh_webui` 也要用它,而 `dsh_webui`
//! 不是 `local_router` 的子模块。

/// 找到缓冲区里第一个 SSE 事件分隔符,返回 `(起始下标, 分隔符字节数)`。
///
/// SSE 允许 `\n\n` 与 `\r\n\r\n` 两种分隔。同一位置同时匹配时(不可能,但
/// 逻辑上)取 LF;不同位置取更靠左的那个 —— 必须是最靠左,否则会把一个完整
/// 事件和下一个事件的开头粘成一帧。
pub(crate) fn find_sse_delimiter(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::find_sse_delimiter;

    #[test]
    fn finds_lf_delimiter() {
        assert_eq!(find_sse_delimiter(b"data: x\n\n"), Some((7, 2)));
    }

    #[test]
    fn finds_crlf_delimiter() {
        assert_eq!(find_sse_delimiter(b"data: x\r\n\r\n"), Some((7, 4)));
    }

    #[test]
    fn returns_none_without_delimiter() {
        assert_eq!(find_sse_delimiter(b""), None);
        assert_eq!(find_sse_delimiter(b"data: partial"), None);
        assert_eq!(find_sse_delimiter(b"\n"), None);
        assert_eq!(find_sse_delimiter(b"\r\n"), None);
    }

    /// 最靠左优先。取错会把两个事件粘成一帧。
    #[test]
    fn prefers_the_leftmost_delimiter() {
        assert_eq!(find_sse_delimiter(b"a\n\nb\r\n\r\n"), Some((1, 2)));
        assert_eq!(find_sse_delimiter(b"a\r\n\r\nb\n\n"), Some((1, 4)));
    }

    /// `\r\n\r\n` 内部不含 `\n\n`,不会被误判成 2 字节分隔符 ——
    /// 误判会把剩下的 `\r\n` 留在缓冲区头部,污染下一帧。
    #[test]
    fn does_not_split_crlf_pair_into_lf() {
        assert_eq!(find_sse_delimiter(b"\r\n\r\n"), Some((0, 4)));
    }

    /// 分隔符紧跟分隔符时仍取第一个。
    #[test]
    fn handles_back_to_back_delimiters() {
        assert_eq!(find_sse_delimiter(b"\n\n\n\n"), Some((0, 2)));
    }
}
