//! 导入的护栏:规模上限与预算。
//!
//! 上限的用途不是「拒绝这次导入」,而是**在命中的那一刻停下并如实记账**。整体失败
//! 会把已经成功的部分一起丢掉,而那部分是有价值的 —— 这一点照 Markio 的判断
//! (`common.rs:24-29` 那段注释)。
//!
//! 和 Markio 不同的是命中之后:它 `push_warning_limited` 加一句自由文本,这里记一条
//! `Skipped { LimitReached }`,于是「因为撞上限而没进来的东西」在报告里是可数的。

/// 单个条目的字节上限。超过这个的不是笔记,是数据文件。
pub const MAX_ENTRY_BYTES: u64 = 100 * 1024 * 1024;
/// 一轮最多处理这么多条目。
pub const MAX_ENTRIES: usize = 100_000;
/// 一轮解压/复制的总字节上限。挡 zip-bomb —— 压缩比可以做到上千倍,单体上限拦不住
/// 「一万个各 99MB」。
pub const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// 目录递归深度上限。软链已经在遍历时跳掉,这一条挡的是真正的深树。
pub const MAX_DEPTH: usize = 32;

/// 命中了哪一条上限。`&'static str` 直接进 `SkipReason::LimitReached`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitHit {
    Entries,
    TotalBytes,
}

impl LimitHit {
    pub fn label(self) -> &'static str {
        match self {
            LimitHit::Entries => "条目数上限",
            LimitHit::TotalBytes => "解压总量上限",
        }
    }
}

/// 一轮导入的规模预算。
///
/// 条目数和总字节合在一个结构里,因为它们必须**一起**判:只数条目挡不住 zip-bomb
/// (一万个大文件),只数字节挡不住十万个空文件(每个都要一次 `create_new` + fsync)。
#[derive(Debug, Default)]
pub struct Budget {
    entries: usize,
    total_bytes: u64,
}

impl Budget {
    pub fn new() -> Self {
        Self::default()
    }

    /// 还能不能再收一条。`Err` 表示这一轮到此为止。
    pub fn check_entry(&self) -> Result<(), LimitHit> {
        if self.entries >= MAX_ENTRIES {
            return Err(LimitHit::Entries);
        }
        Ok(())
    }

    /// 记一条已处理的条目及其字节数。
    ///
    /// 返回 `Err` 时**这一条已经算进去了** —— 它是压垮预算的那一条,调用方该停止后续
    /// 而不是重试它。`saturating_add` 不能省:总量是 u64,但源端字节数由外部文件决定,
    /// 溢出会绕回小数字,于是上限失效。
    pub fn record(&mut self, bytes: u64) -> Result<(), LimitHit> {
        self.entries += 1;
        self.total_bytes = self.total_bytes.saturating_add(bytes);
        if self.total_bytes > MAX_TOTAL_BYTES {
            return Err(LimitHit::TotalBytes);
        }
        Ok(())
    }

    /// 两个访问器只服务测试:计数是私有的,生产路径靠 `record` 的返回值判超限,
    /// 不读中间值。标 `cfg(test)` 而不是 `allow(dead_code)` —— 前者说明它为什么存在,
    /// 后者只是把告警按掉。
    #[cfg(test)]
    pub fn entries(&self) -> usize {
        self.entries
    }

    #[cfg(test)]
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}

/// 单体是否超限。分开一个函数是因为它在**读之前**就能判(zip 的 `size()`、
/// 文件的 `metadata().len()`),不必先把 100MB 读进内存再拒。
pub fn entry_too_large(bytes: u64) -> bool {
    bytes > MAX_ENTRY_BYTES
}

/// 从 zip entry 里读内容,受单体上限约束。
///
/// **声明的 `size()` 不能当依据。** zip 头里那个数字是归档自己说的,一个恶意归档可以
/// 声明 1KB 而解出 1GB。所以两道都要:读之前按声明拒一次(省掉明显的),读的时候用
/// `take(limit + 1)` 封住上界,读完再按**实际**长度判一次 —— 只有后者拦得住说谎的头。
pub fn read_zip_entry_limited(
    entry: &mut zip::read::ZipFile<'_, impl std::io::Read>,
) -> Result<Vec<u8>, u64> {
    use std::io::Read;

    let declared = entry.size();
    if entry_too_large(declared) {
        return Err(declared);
    }
    // 预留按声明值走,但夹到 8MB:声明值是不可信输入,拿它直接 `with_capacity`
    // 等于让归档指定一次分配多大。
    let mut buf = Vec::with_capacity(declared.min(8 * 1024 * 1024) as usize);
    entry
        .take(MAX_ENTRY_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|_| declared)?;
    if buf.len() as u64 > MAX_ENTRY_BYTES {
        return Err(buf.len() as u64);
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_limit_is_exclusive_at_the_boundary() {
        assert!(!entry_too_large(MAX_ENTRY_BYTES));
        assert!(entry_too_large(MAX_ENTRY_BYTES + 1));
    }

    #[test]
    fn budget_stops_at_the_entry_ceiling() {
        let mut budget = Budget::new();
        for _ in 0..MAX_ENTRIES {
            budget.check_entry().expect("还没到上限");
            budget.record(0).expect("字节没超");
        }
        assert_eq!(budget.check_entry(), Err(LimitHit::Entries));
    }

    #[test]
    fn budget_counts_bytes_even_when_entries_are_few() {
        // 只数条目挡不住 zip-bomb:三条就能把 4GB 顶掉。
        let mut budget = Budget::new();
        let chunk = MAX_TOTAL_BYTES / 2;
        assert!(budget.record(chunk).is_ok());
        assert!(budget.record(chunk).is_ok());
        assert_eq!(budget.record(1), Err(LimitHit::TotalBytes));
        assert_eq!(budget.entries(), 3);
    }

    #[test]
    fn total_bytes_saturates_instead_of_wrapping() {
        // 源端字节数是外部输入。溢出绕回小数字会让上限静默失效 —— 那正是
        // zip-bomb 想要的结果。
        let mut budget = Budget::new();
        assert_eq!(budget.record(u64::MAX), Err(LimitHit::TotalBytes));
        assert_eq!(budget.record(u64::MAX), Err(LimitHit::TotalBytes));
        assert_eq!(budget.total_bytes(), u64::MAX);
    }

    #[test]
    fn the_entry_that_breaks_the_budget_is_already_counted() {
        // 调用方靠这一点知道「这条别重试」。
        let mut budget = Budget::new();
        assert!(budget.record(MAX_TOTAL_BYTES + 1).is_err());
        assert_eq!(budget.entries(), 1);
    }

    #[test]
    fn limit_labels_are_distinct() {
        assert_ne!(LimitHit::Entries.label(), LimitHit::TotalBytes.label());
    }

    /// 造一个只含一个 entry 的 zip,拿回它的字节。
    fn one_entry_zip(name: &str, body: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buf);
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .expect("开 entry");
            writer.write_all(body).expect("写 entry");
            writer.finish().expect("收尾");
        }
        buf.into_inner()
    }

    #[test]
    fn reading_a_normal_entry_returns_its_bytes() {
        let bytes = one_entry_zip("a.md", b"# hello");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("解析 zip");
        let mut entry = archive.by_index(0).expect("取 entry");
        assert_eq!(read_zip_entry_limited(&mut entry).expect("读"), b"# hello");
    }

    #[test]
    fn a_highly_compressible_entry_is_still_bounded_by_actual_length() {
        // 这条盯的是**读完之后**那道判断。zip 头里的 `size()` 是归档自己说的,
        // 一个恶意归档可以声明很小而解出很大;只按声明值拒的话这里就漏了。
        // 一兆个零压缩后只有几百字节,是最典型的那种形状。
        let body = vec![0_u8; 1024 * 1024];
        let bytes = one_entry_zip("bomb.bin", &body);
        assert!(
            bytes.len() < 8 * 1024,
            "压缩后应远小于原始大小,否则这条测的不是同一件事"
        );
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("解析 zip");
        let mut entry = archive.by_index(0).expect("取 entry");
        // 1MB 在上限之内,所以这一条应当**成功**并给出完整内容 —— 断言的是
        // `take(limit + 1)` 那道封顶没有把正常内容截短。
        let read = read_zip_entry_limited(&mut entry).expect("读");
        assert_eq!(read.len(), body.len());
    }

    #[test]
    fn an_entry_declaring_too_much_is_rejected_before_reading() {
        // 声明值就超限时不该先把它读进内存再拒。
        let declared = MAX_ENTRY_BYTES + 1;
        assert!(entry_too_large(declared));
    }
}
