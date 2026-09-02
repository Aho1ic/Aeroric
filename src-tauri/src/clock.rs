//! Unix epoch 毫秒时间戳的共享实现。
//!
//! 这两个函数原先在 6 个模块里各存一份(`skills` / `agent_ops` / `local_history` /
//! `notebook::attachments` / `notebook::trash` / `remote::auth`),实现语义相同,
//! 只差返回型(`i64` / `u64`)和 `unwrap_or(0)` 的写法。
//!
//! 保留两种返回型而不是统一成一种:调用点的下游类型不同(有的进 JSON 的
//! `i64` 字段,有的做 `u64` 的时间差计算),硬统一会在 23 个调用点引入
//! `as` 转换 —— 那才是真正容易出错的地方。

use std::time::{SystemTime, UNIX_EPOCH};

/// 当前 Unix epoch 毫秒,`u64`。
///
/// 系统时钟早于 epoch 时返回 0 —— 调用方一律把 0 当"未知时间"处理,
/// 这比 panic 更合适(时钟异常不该让笔记附件写不进去)。
pub(crate) fn now_ms_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_millis() as u64)
        .unwrap_or(0)
}

/// 当前 Unix epoch 毫秒,`i64`。语义同 [`now_ms_u64`]。
pub(crate) fn now_ms_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|delta| delta.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{now_ms_i64, now_ms_u64};

    /// 2020-01-01T00:00:00Z。断言时钟基准是真实 epoch 毫秒 ——
    /// 曾经踩过"假时钟从 1000 起算,UI 上显示 1970"的坑,而 1970 正好是
    /// 「没读到时间」的哨兵值,两者在界面上无法区分。
    const YEAR_2020_MS: u64 = 1_577_836_800_000;

    #[test]
    fn u64_variant_returns_real_epoch_millis() {
        let now = now_ms_u64();
        assert!(now > YEAR_2020_MS, "时间戳看起来不是真实 epoch 毫秒: {now}");
    }

    #[test]
    fn i64_variant_returns_real_epoch_millis() {
        let now = now_ms_i64();
        assert!(
            now > YEAR_2020_MS as i64,
            "时间戳看起来不是真实 epoch 毫秒: {now}"
        );
    }

    #[test]
    fn both_variants_agree() {
        let a = now_ms_u64();
        let b = now_ms_i64();
        let delta = (b - a as i64).abs();
        assert!(
            delta < 1_000,
            "两个变体差了 {delta} ms,超出同一次调用的合理范围"
        );
    }

    #[test]
    fn is_monotonic_across_calls() {
        let first = now_ms_u64();
        let second = now_ms_u64();
        assert!(second >= first, "{second} < {first}");
    }
}
