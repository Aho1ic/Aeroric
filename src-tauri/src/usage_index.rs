use notify::{RecursiveMode, Watcher};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

use crate::analytics::{self, UsageAgent, UsageRequest};

const INDEX_EVENT: &str = "usage-statistics-updated";
const EVENT_DEBOUNCE: Duration = Duration::from_millis(500);
/// 漏事件的兜底重扫间隔。正常路径走 FSEvents,不依赖这个值。
///
/// 原来是 5 s。每次扫描要遍历 usage 根下上千个 jsonl 并逐个 canonicalize + stat
/// （本机实测 1404 个文件、约 1.8 s 墙钟）,5 s 一轮意味着这个线程基本没停过。
/// 兜底不需要这么勤。
const FALLBACK_SCAN_INTERVAL: Duration = Duration::from_secs(30);
/// 同一个来源被重解析的最小间隔,见 `should_reparse`。
const REPARSE_THROTTLE: Duration = Duration::from_secs(30);
const MAX_DECOMPRESSED_USAGE_LOG_BYTES: usize = 512 * 1024 * 1024;
const MAX_USAGE_LOG_LINE_BYTES: usize = 16 * 1024 * 1024;
/// 游标边界指纹覆盖的字节数。续扫前用它校验「已封存前缀」没有被外部改写 ——
/// 同尺寸或更大的重写靠 size 看不出来。
const CURSOR_FINGERPRINT_BYTES: u64 = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceState {
    modified_ns: i64,
    size: i64,
}

/// 会话文件的解析家族。存进 `usage_sources.cursor_format`:续扫时不必再读文件头
/// 重新探测(codex/omp 的探测窗口是前 200 行)。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceFormat {
    Dsh,
    Omp,
    Codex,
    Claude,
}

/// [`SourceFormat`] → `cursor_format` 列的字面量。与 [`parse_source_format`] 互为逆向,
/// 两边必须同时改。
fn source_format_column(format: SourceFormat) -> &'static str {
    match format {
        SourceFormat::Dsh => "dsh",
        SourceFormat::Omp => "omp",
        SourceFormat::Codex => "codex",
        SourceFormat::Claude => "claude",
    }
}

/// `cursor_format` 列的字面量 → [`SourceFormat`]。认不出来回 `None`,调用方按
/// 「没有可用游标」处理(全量重读),而不是猜一个家族 —— 猜错等于用错解析器。
fn parse_source_format(value: &str) -> Option<SourceFormat> {
    match value {
        "dsh" => Some(SourceFormat::Dsh),
        "omp" => Some(SourceFormat::Omp),
        "codex" => Some(SourceFormat::Codex),
        "claude" => Some(SourceFormat::Claude),
        _ => None,
    }
}

/// 已封存前缀的续扫游标。
///
/// 「封存」= 这段前缀产出的记录已经入库,而且再解析一次会得到同样的结果。续扫
/// 只读 `offset` 之后的字节,前缀不再重读(85 MB 的活跃会话原来每轮整份重解析
/// 约 1.2 s)。
///
/// 封存点必须满足 `parse(前缀) ++ parse(尾段) == parse(整份)`。四个家族的解析器
/// 形状不同,这个条件落成三条规则(见 [`scan_slice`]):
/// - omp 逐行无状态 → 任意完整行边界。
/// - dsh 逐行 + 跨行 `model`,而 `model` 在扫描中每行都可观测 → 任意完整行边界,
///   把该处的 `model` 一起记下来。
/// - codex 逐行 + 跨行 `model`,但解析器只吃整段内容、`model` 只能从产出的记录
///   反推 → 封存到「最后一条产出记录所在的完整行」之后,那条记录的 model 就是
///   此处的状态。
/// - claude 按 `message.id` 在整份内容里去重(一次 API 响应的多个 content block
///   写成连续多行、共享同一个 id) → 封存点不能落在这样一段连续产出行之中。
#[derive(Clone, Debug)]
struct SourceCursor {
    /// 已封存前缀的结束字节偏移。
    offset: u64,
    /// `offset` 之前最多 [`CURSOR_FINGERPRINT_BYTES`] 字节的指纹。
    fingerprint: i64,
    /// 已封存前缀产出的记录条数,也就是尾段记录的起始 `request_index`。
    rows: i64,
    /// 封存点处的跨行 `model` 状态(dsh/codex);其余家族为空串。
    model: String,
    format: SourceFormat,
}

/// 一次扫描要落库的东西。
struct SourceScan {
    /// 本次要写入的记录。全量扫描是整份;续扫只有尾段。
    requests: Vec<UsageRequest>,
    /// 这批记录的起始 `request_index`。全量扫描为 0(先删掉这个来源的全部旧行)。
    base_index: i64,
    /// 更新后的游标。`None` = 这个来源不能续扫:zstd 的行偏移是解压后的位置,
    /// seek 不回原文件。
    cursor: Option<SourceCursor>,
}

/// 一段文本(整份或尾段)的解析结果。
struct SliceScan {
    /// 这段产出的全部记录:先封存段的,再未封存尾段的。
    requests: Vec<UsageRequest>,
    /// 这段内可封存的字节数,相对本段起点,落在行边界。
    sealed_bytes: u64,
    /// 落在封存点之前的记录条数。
    sealed_requests: usize,
    /// 封存点处的 `model` 状态。
    model: String,
}

fn database_path() -> Result<PathBuf, String> {
    Ok(crate::storage::aeroric_dir()?.join("usage-statistics.sqlite3"))
}

fn open_database() -> Result<Connection, String> {
    crate::storage::ensure_aeroric_dirs()?;
    let connection = Connection::open(database_path()?).map_err(|error| error.to_string())?;
    connection
        .busy_timeout(Duration::from_secs(3))
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(|error| error.to_string())?;
    initialize_database(&connection)?;
    Ok(connection)
}

fn initialize_database(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS usage_sources (
                path TEXT PRIMARY KEY,
                modified_ns INTEGER NOT NULL,
                size INTEGER NOT NULL,
                indexed_at INTEGER NOT NULL,
                cursor_offset INTEGER,
                cursor_fingerprint INTEGER,
                cursor_rows INTEGER,
                cursor_model TEXT,
                cursor_format TEXT
            );
            CREATE TABLE IF NOT EXISTS usage_requests (
                source_path TEXT NOT NULL,
                request_index INTEGER NOT NULL,
                timestamp REAL NOT NULL,
                date TEXT NOT NULL,
                agent TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL,
                output_tokens INTEGER NOT NULL,
                cache_creation_tokens INTEGER NOT NULL,
                cache_read_tokens INTEGER NOT NULL,
                PRIMARY KEY (source_path, request_index)
            );
            CREATE INDEX IF NOT EXISTS usage_requests_date_agent
                ON usage_requests (date, agent);
            ",
        )
        .map_err(|error| error.to_string())?;
    ensure_source_cursor_columns(connection)
}

/// 给存量库补齐游标列。
///
/// `CREATE TABLE IF NOT EXISTS` 不会给已存在的表加列,升级路径必须显式 ALTER。
/// 存量行的游标列留 NULL,按「没有可用游标」处理:指纹没变的来源本轮直接跳过,
/// `usage_requests` 里的历史用量原样留着;等文件下次变化时走一次全量重读并建立
/// 游标。整个迁移不动任何一条已入库的记录。
fn ensure_source_cursor_columns(connection: &Connection) -> Result<(), String> {
    let mut existing = HashSet::new();
    {
        let mut statement = connection
            .prepare("PRAGMA table_info(usage_sources)")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|error| error.to_string())?;
        for row in rows {
            existing.insert(row.map_err(|error| error.to_string())?);
        }
    }
    for (column, kind) in [
        ("cursor_offset", "INTEGER"),
        ("cursor_fingerprint", "INTEGER"),
        ("cursor_rows", "INTEGER"),
        ("cursor_model", "TEXT"),
        ("cursor_format", "TEXT"),
    ] {
        if existing.contains(column) {
            continue;
        }
        connection
            .execute(
                &format!("ALTER TABLE usage_sources ADD COLUMN {column} {kind}"),
                [],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn source_state(path: &Path) -> Option<SourceState> {
    let metadata = fs::metadata(path).ok()?;
    let modified_ns = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(i64::MAX as u128) as i64;
    Some(SourceState {
        modified_ns,
        size: metadata.len().min(i64::MAX as u64) as i64,
    })
}

/// 库里记着的一条来源:内容指纹 + 上次入库时刻 + 续扫游标。
///
/// `indexed_at` 是节流用的:活跃会话的 jsonl 每条消息都在追加,指纹每次都不同。
/// 有了 [`SourceCursor`] 之后每轮只读新增尾段,但一轮仍要 stat + seek + 读尾段 +
/// 写事务;500 ms 一次事件全部放行仍然是白烧 IO,所以节流保留。
#[derive(Clone, Debug)]
struct IndexedSource {
    state: SourceState,
    indexed_at: i64,
    /// 续扫游标。`None` = 升级前的存量行、或不能续扫的来源(zstd),按全量重读处理。
    cursor: Option<SourceCursor>,
}

fn load_source_states(connection: &Connection) -> Result<HashMap<String, IndexedSource>, String> {
    let mut statement = connection
        .prepare(
            "
            SELECT
                path,
                modified_ns,
                size,
                indexed_at,
                cursor_offset,
                cursor_fingerprint,
                cursor_rows,
                cursor_model,
                cursor_format
            FROM usage_sources
            ",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            let offset = row.get::<_, Option<i64>>(4)?;
            let fingerprint = row.get::<_, Option<i64>>(5)?;
            let sealed_rows = row.get::<_, Option<i64>>(6)?;
            let model = row.get::<_, Option<String>>(7)?;
            let format = row.get::<_, Option<String>>(8)?;
            // 四个必需列缺任何一个、或值不合法,就没有可用游标。宁可全量重读也不
            // 拿半个游标去 seek —— 偏移错了就是漏记或双算。
            let cursor = match (offset, fingerprint, sealed_rows, format) {
                (Some(offset), Some(fingerprint), Some(sealed_rows), Some(format))
                    if offset >= 0 && sealed_rows >= 0 =>
                {
                    parse_source_format(&format).map(|format| SourceCursor {
                        offset: offset as u64,
                        fingerprint,
                        rows: sealed_rows,
                        model: model.unwrap_or_default(),
                        format,
                    })
                }
                _ => None,
            };
            Ok((
                row.get::<_, String>(0)?,
                IndexedSource {
                    state: SourceState {
                        modified_ns: row.get(1)?,
                        size: row.get(2)?,
                    },
                    indexed_at: row.get(3)?,
                    cursor,
                },
            ))
        })
        .map_err(|error| error.to_string())?;
    let mut states = HashMap::new();
    for row in rows {
        let (path, state) = row.map_err(|error| error.to_string())?;
        states.insert(path, state);
    }
    Ok(states)
}

/// 游标边界前尾部字节的指纹。域标签防止与别处的哈希混用。
fn cursor_fingerprint(tail: &[u8]) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(b"aeroric-usage-cursor-v1");
    hasher.update(tail);
    let digest = hasher.finalize();
    i64::from_be_bytes(digest[..8].try_into().unwrap_or_default())
}

/// 读 `end` 之前最多 [`CURSOR_FINGERPRINT_BYTES`] 字节。返回后文件位置恰好停在
/// `end`,续扫可以直接从这里往后读 —— 校验指纹这一次读顺带完成了 seek。
fn read_fingerprint_window(file: &mut fs::File, end: u64) -> io::Result<Vec<u8>> {
    let length = end.min(CURSOR_FINGERPRINT_BYTES);
    file.seek(SeekFrom::Start(end - length))?;
    let mut window = vec![0u8; length as usize];
    if length > 0 {
        file.read_exact(&mut window)?;
    }
    Ok(window)
}

/// 把新封存的字节接到指纹窗口后面,只留末尾 [`CURSOR_FINGERPRINT_BYTES`] 字节。
fn roll_fingerprint_window(window: &mut Vec<u8>, sealed: &[u8]) {
    window.extend_from_slice(sealed);
    let max = CURSOR_FINGERPRINT_BYTES as usize;
    if window.len() > max {
        window.drain(..window.len() - max);
    }
}

/// 用文件里 `sealed` 之前的字节算游标指纹。全量扫描收尾时用:那条路径没有把
/// 已封存字节留在内存里(dsh 走流式,大文件不整份进内存)。
fn seal_fingerprint(path: &Path, sealed: u64) -> Option<i64> {
    let mut file = fs::File::open(path).ok()?;
    let window = read_fingerprint_window(&mut file, sealed).ok()?;
    Some(cursor_fingerprint(&window))
}

fn read_limited_line<R: BufRead>(reader: &mut R, buffer: &mut Vec<u8>) -> io::Result<usize> {
    buffer.clear();
    let mut limited = reader.take((MAX_USAGE_LOG_LINE_BYTES + 1) as u64);
    let read = limited.read_until(b'\n', buffer)?;
    if read > MAX_USAGE_LOG_LINE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "usage log line exceeds safety limit",
        ));
    }
    Ok(read)
}

/// 逐行扫描 dsh transcript 的正文。`model_in` 是进入这段时的跨行 `model` 状态。
///
/// dsh 的 `model` 每行都可观测(解析器直接吃 `&mut String`),所以任意完整行边界
/// 都能封存,只要把该处的 `model` 一起记下来。末尾没有 `\n` 的那一行照常解析(文
/// 件可能本来就不以换行结尾),但**不封存**:下一轮从封存点重读,补全后的那行会
/// 被重新解析、覆盖写入,既不会被吞掉也不会记两次。
fn scan_dsh_lines<R: BufRead>(
    mut reader: R,
    max_bytes: usize,
    model_in: &str,
) -> io::Result<SliceScan> {
    let mut line = Vec::new();
    let mut model = model_in.to_owned();
    let mut requests = Vec::new();
    let mut total_bytes = 0usize;
    let mut consumed = 0u64;
    let mut sealed_bytes = 0u64;
    let mut sealed_requests = 0usize;
    let mut sealed_model = model_in.to_owned();
    loop {
        let read = read_limited_line(&mut reader, &mut line)?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes
            .checked_add(read)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "usage log size overflow"))?;
        if total_bytes > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "usage log exceeds decompression safety limit",
            ));
        }
        consumed += read as u64;
        let complete = line.last() == Some(&b'\n');
        let text = std::str::from_utf8(&line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if let Some(request) = analytics::parse_dsh_usage_line(text, &mut model) {
            requests.push(request);
        }
        if !complete {
            break;
        }
        sealed_bytes = consumed;
        sealed_requests = requests.len();
        sealed_model.clear();
        sealed_model.push_str(&model);
    }
    Ok(SliceScan {
        requests,
        sealed_bytes,
        sealed_requests,
        model: sealed_model,
    })
}

/// 从头流式扫描一个 dsh transcript。首行不是 dsh header 时返回 `Ok(None)`,交给
/// 字符串路径按 omp/codex/claude 处理。
///
/// 流式是为了大文件:dsh 会话能到 85 MB,不整份读进内存。
fn scan_dsh_stream<R: BufRead>(mut reader: R, max_bytes: usize) -> io::Result<Option<SliceScan>> {
    let mut line = Vec::new();
    let first_bytes = read_limited_line(&mut reader, &mut line)?;
    if first_bytes == 0 {
        return Ok(None);
    }
    if first_bytes > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "usage log exceeds decompression safety limit",
        ));
    }
    let header_complete = line.last() == Some(&b'\n');
    let header = std::str::from_utf8(&line)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if !analytics::is_dsh_session(header) {
        return Ok(None);
    }
    if !header_complete {
        // 只写了半行 header:没有记录,也没有任何可封存的字节。
        return Ok(Some(SliceScan {
            requests: Vec::new(),
            sealed_bytes: 0,
            sealed_requests: 0,
            model: String::new(),
        }));
    }
    let mut scan = scan_dsh_lines(reader, max_bytes - first_bytes, "")?;
    // header 行不产出记录,但它占的字节要计进封存偏移。
    scan.sealed_bytes += first_bytes as u64;
    Ok(Some(scan))
}

/// `head` 必须以行边界结束(或为空)。从末尾往前找第一个满足 `is_boundary` 的行,
/// 返回该行末尾(含 `\n`)的字节偏移;没有则返回 0。
///
/// 只往前走到第一个命中就停,代价与「尾部那一小段」成正比,不是整份文件。
fn last_line_end(head: &str, is_boundary: impl Fn(&str) -> bool) -> usize {
    let mut end = head.len();
    while end > 0 {
        // `end` 总是紧跟在一个 `\n` 之后,所以 `end - 1` 落在字符边界上。
        let start = head[..end - 1].rfind('\n').map_or(0, |index| index + 1);
        if is_boundary(&head[start..end]) {
            return end;
        }
        end = start;
    }
    0
}

/// 把封存点的 `model` 回灌给 codex 解析器。
///
/// 它的跨行状态只由 `turn_context` 事件设置,复现这个事件比在这里另抄一份 model
/// 提取逻辑更可靠 —— 抄一份就是第二套约定,上游改了字段名这边不会跟着报错。
fn parse_codex_from_model(text: &str, model: &str) -> Vec<UsageRequest> {
    if model.is_empty() {
        return analytics::parse_codex_usage_requests(text);
    }
    let replay = serde_json::json!({ "type": "turn_context", "payload": { "model": model } });
    analytics::parse_codex_usage_requests(&format!("{replay}\n{text}"))
}

/// omp:逐行无状态,任意完整行边界都能封存。
fn scan_omp_slice(text: &str) -> SliceScan {
    let complete = text.rfind('\n').map_or(0, |index| index + 1);
    let mut requests = analytics::parse_omp_usage_requests(&text[..complete]);
    let sealed_requests = requests.len();
    requests.extend(analytics::parse_omp_usage_requests(&text[complete..]));
    SliceScan {
        requests,
        sealed_bytes: complete as u64,
        sealed_requests,
        model: String::new(),
    }
}

/// codex:跨行 `model` 只能从产出的记录反推,所以封存点取「最后一条产出记录所在
/// 的完整行」之后 —— 那条记录的 model 正是此处的状态。之后的 `turn_context` 留在
/// 未封存尾段,下一轮连同它后面的 `token_count` 一起重新解析,不会用错模型。
fn scan_codex_slice(text: &str, model_in: &str) -> SliceScan {
    let complete = text.rfind('\n').map_or(0, |index| index + 1);
    let sealed_bytes = last_line_end(&text[..complete], |line| {
        !analytics::parse_codex_usage_requests(line).is_empty()
    });
    let mut requests = parse_codex_from_model(&text[..sealed_bytes], model_in);
    let sealed_requests = requests.len();
    let model = requests
        .last()
        .map_or_else(|| model_in.to_owned(), |request| request.model.clone());
    requests.extend(parse_codex_from_model(&text[sealed_bytes..], &model));
    SliceScan {
        requests,
        sealed_bytes: sealed_bytes as u64,
        sealed_requests,
        model,
    }
}

/// claude:整份按 `message.id` 去重。
///
/// 一次 API 响应的多个 content block 被写成连续多行、共享同一个 `message.id`,整份
/// 解析会把它们收成一条(这也正是那个 dedup 存在的原因)。封存点若落在这样一段
/// 连续行之中,前缀记一条、尾段再记一条 —— 同一次响应双算。所以封存点取「最后一个
/// 不产出记录的完整行」之后:去重组必然整个落在一段连续的产出行里,这个位置一定
/// 在组的外面。常见形态是 assistant 行后面紧跟 user/tool_result 行,于是几乎整份
/// 都能封存。
fn scan_claude_slice(text: &str, source_key: &str) -> SliceScan {
    let complete = text.rfind('\n').map_or(0, |index| index + 1);
    let sealed_bytes = last_line_end(&text[..complete], |line| {
        analytics::parse_claude_usage_requests(line, source_key).is_empty()
    });
    let mut requests = analytics::parse_claude_usage_requests(&text[..sealed_bytes], source_key);
    let sealed_requests = requests.len();
    requests.extend(analytics::parse_claude_usage_requests(
        &text[sealed_bytes..],
        source_key,
    ));
    SliceScan {
        requests,
        sealed_bytes: sealed_bytes as u64,
        sealed_requests,
        model: String::new(),
    }
}

/// 解析一段已在内存里的文本(整份,或续扫读到的尾段)。
///
/// `model_in` 是进入这段时的跨行 `model` 状态,由上一次封存点带过来。
fn scan_slice(
    format: SourceFormat,
    text: &str,
    source_key: &str,
    model_in: &str,
) -> Option<SliceScan> {
    match format {
        // dsh 逐行解析,封存点在扫描过程中直接得到。
        SourceFormat::Dsh => {
            scan_dsh_lines(Cursor::new(text.as_bytes()), text.len(), model_in).ok()
        }
        SourceFormat::Omp => Some(scan_omp_slice(text)),
        SourceFormat::Codex => Some(scan_codex_slice(text, model_in)),
        SourceFormat::Claude => Some(scan_claude_slice(text, source_key)),
    }
}

/// 把一次全量扫描的结果配上游标。
fn full_source_scan(format: SourceFormat, path: &Path, scan: SliceScan) -> SourceScan {
    let SliceScan {
        requests,
        sealed_bytes,
        sealed_requests,
        model,
    } = scan;
    let cursor = seal_fingerprint(path, sealed_bytes).map(|fingerprint| SourceCursor {
        offset: sealed_bytes,
        fingerprint,
        rows: sealed_requests.min(i64::MAX as usize) as i64,
        model,
        format,
    });
    SourceScan {
        requests,
        base_index: 0,
        cursor,
    }
}

/// 整份扫描一个来源。返回 `None` = 读不了或解析不了,调用方跳过这个来源(库里的
/// 旧行保持不动)。
fn scan_source_full(path: &Path) -> Option<SourceScan> {
    if analytics::is_zstd_usage_log(path) {
        let file = fs::File::open(path).ok()?;
        let decoder = zstd::stream::read::Decoder::new(file).ok()?;
        let scan = scan_dsh_stream(BufReader::new(decoder), MAX_DECOMPRESSED_USAGE_LOG_BYTES)
            .ok()
            .flatten()?;
        // 压缩文件的行偏移是解压后的位置,seek 不回原文件 → 不留游标。归档的
        // `.jsonl.zstd` 也不会再增长,续扫本来就没有收益。
        return Some(SourceScan {
            requests: scan.requests,
            base_index: 0,
            cursor: None,
        });
    }

    let file = fs::File::open(path).ok()?;
    match scan_dsh_stream(BufReader::new(file), MAX_DECOMPRESSED_USAGE_LOG_BYTES) {
        Ok(Some(scan)) => return Some(full_source_scan(SourceFormat::Dsh, path, scan)),
        Ok(None) => {}
        Err(_) => return None,
    }
    if fs::metadata(path).ok()?.len() > MAX_DECOMPRESSED_USAGE_LOG_BYTES as u64 {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    // omp 的判定谓词更具体(title 槽/version:3),放在 dsh(接受任意数字
    // version 的 {type:"session"} 头)之前,避免无 title 槽的 omp 文件被 dsh 吞掉。
    let format = if analytics::is_omp_session(&content) {
        SourceFormat::Omp
    } else if analytics::is_dsh_session(&content) {
        SourceFormat::Dsh
    } else if analytics::is_codex_session(&content) {
        SourceFormat::Codex
    } else {
        SourceFormat::Claude
    };
    let scan = scan_slice(format, &content, &path.to_string_lossy(), "")?;
    Some(full_source_scan(format, path, scan))
}

/// 从游标续扫:校验封存边界没被改写,再从游标读新增尾段。
///
/// 返回 `None` = 必须全量重读,三种情形:
/// - 读不到「封存边界前那段字节」—— 文件被截断,或被更短的内容重写,游标落到了
///   文件之外。这是一次 `read_exact` 短读,不需要另外拿 stat 的 size 去比:size
///   是这一轮更早时候取的,拿它判断反而会在「stat 之后文件又长了」时误判;
/// - 封存边界前的尾部指纹失配 —— 同尺寸/更大的重写、轮转替换,size 看不出来;
/// - 尾段读失败、非 UTF-8,或超过解压安全上限。
///
/// 这里可以放心全量重读:`usage_requests` 只按 `source_path` 存明细,没有「汇总后
/// 删明细」的环节,[`write_source_scan`] 会先删掉 `base_index` 之后的旧行,全量重读
/// (`base_index == 0`)等于整份替换,是幂等的。参照实现在这种情形下把游标钉到 EOF、
/// 放弃重写区间,是因为它 30 天前的明细会被汇总后剪掉、重导会二次累加 —— 那个约束
/// 这边不存在,而钉游标要丢掉重写区间的数据,所以这边选重读。
///
/// 轮转替换不额外记 inode:Windows 没有稳定的文件 id,而「短读 + 边界指纹」已经
/// 覆盖了它 —— 换了 inode 但前缀字节一模一样时,续扫本来就是对的。
fn scan_source_incremental(path: &Path, cursor: &SourceCursor) -> Option<SourceScan> {
    if analytics::is_zstd_usage_log(path) {
        return None;
    }
    let mut file = fs::File::open(path).ok()?;
    let mut window = read_fingerprint_window(&mut file, cursor.offset).ok()?;
    if cursor_fingerprint(&window) != cursor.fingerprint {
        return None;
    }
    let mut tail = Vec::new();
    file.by_ref()
        .take(MAX_DECOMPRESSED_USAGE_LOG_BYTES as u64 + 1)
        .read_to_end(&mut tail)
        .ok()?;
    if tail.len() > MAX_DECOMPRESSED_USAGE_LOG_BYTES {
        return None;
    }
    let text = std::str::from_utf8(&tail).ok()?;
    let scan = scan_slice(cursor.format, text, &path.to_string_lossy(), &cursor.model)?;
    let sealed_bytes = scan.sealed_bytes.min(tail.len() as u64);
    roll_fingerprint_window(&mut window, &tail[..sealed_bytes as usize]);
    Some(SourceScan {
        requests: scan.requests,
        base_index: cursor.rows,
        cursor: Some(SourceCursor {
            offset: cursor.offset.saturating_add(sealed_bytes),
            fingerprint: cursor_fingerprint(&window),
            rows: cursor
                .rows
                .saturating_add(scan.sealed_requests.min(i64::MAX as usize) as i64),
            model: scan.model,
            format: cursor.format,
        }),
    })
}

fn as_sql_integer(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

/// [`UsageAgent`] → `agent` 列的字面量。与 [`parse_usage_agent`] 互为逆向。
///
/// 这两个 match 是库里 `agent` 列的唯一值域来源:写入只可能产出这四个字面量,所以读取
/// 认不出来就一定是旧行或坏行,不是新家族。
fn usage_agent_column(agent: UsageAgent) -> &'static str {
    match agent {
        UsageAgent::Codex => "codex",
        UsageAgent::Claude => "claude",
        UsageAgent::Dsh => "dsh",
        UsageAgent::Omp => "omp",
    }
}

/// 写入一次扫描的结果:记录 + 游标,同一个事务里原子提交。
///
/// 只删 `request_index >= base_index` 的旧行:续扫时已封存前缀的行原地不动,这才是
/// 省下重读的地方;全量重读(`base_index == 0`)时所有 `request_index` 都 `>= 0`,
/// 同一条语句就是整份替换,所以重写/截断后的重读天然幂等。
fn write_source_scan(
    connection: &mut Connection,
    path: &str,
    state: SourceState,
    scan: &SourceScan,
) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM usage_requests WHERE source_path = ?1 AND request_index >= ?2",
            params![path, scan.base_index.max(0)],
        )
        .map_err(|error| error.to_string())?;
    {
        let mut insert = transaction
            .prepare(
                "
                INSERT INTO usage_requests (
                    source_path,
                    request_index,
                    timestamp,
                    date,
                    agent,
                    model,
                    input_tokens,
                    output_tokens,
                    cache_creation_tokens,
                    cache_read_tokens
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ",
            )
            .map_err(|error| error.to_string())?;
        for (index, request) in scan.requests.iter().enumerate() {
            insert
                .execute(params![
                    path,
                    scan.base_index
                        .saturating_add(index.min(i64::MAX as usize) as i64),
                    request.timestamp,
                    request.date.to_string(),
                    usage_agent_column(request.agent),
                    request.model,
                    as_sql_integer(request.input_tokens),
                    as_sql_integer(request.output_tokens),
                    as_sql_integer(request.cache_creation_tokens),
                    as_sql_integer(request.cache_read_tokens),
                ])
                .map_err(|error| error.to_string())?;
        }
    }
    let cursor = scan.cursor.as_ref();
    transaction
        .execute(
            "
            INSERT INTO usage_sources (
                path,
                modified_ns,
                size,
                indexed_at,
                cursor_offset,
                cursor_fingerprint,
                cursor_rows,
                cursor_model,
                cursor_format
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(path) DO UPDATE SET
                modified_ns = excluded.modified_ns,
                size = excluded.size,
                indexed_at = excluded.indexed_at,
                cursor_offset = excluded.cursor_offset,
                cursor_fingerprint = excluded.cursor_fingerprint,
                cursor_rows = excluded.cursor_rows,
                cursor_model = excluded.cursor_model,
                cursor_format = excluded.cursor_format
            ",
            params![
                path,
                state.modified_ns,
                state.size,
                unix_millis(),
                // 不能续扫时整组写 NULL,别把上一轮的游标留成孤儿。
                cursor.map(|cursor| as_sql_integer(cursor.offset)),
                cursor.map(|cursor| cursor.fingerprint),
                cursor.map(|cursor| cursor.rows),
                cursor.map(|cursor| cursor.model.as_str()),
                cursor.map(|cursor| source_format_column(cursor.format)),
            ],
        )
        .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

fn remove_sources(connection: &mut Connection, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    for path in paths {
        transaction
            .execute(
                "DELETE FROM usage_requests WHERE source_path = ?1",
                params![path],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute("DELETE FROM usage_sources WHERE path = ?1", params![path])
            .map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}

/// 这一轮要不要重解析这个来源。
///
/// 三档:
/// - 库里没有 → 必须解析。
/// - 指纹没变 → 内容没动,跳过。
/// - 指纹变了 → 只有距上次入库超过 `REPARSE_THROTTLE` 才解析。
///
/// 为什么"变了也可能跳过":统计口径是「按天累计的 token」,晚几十秒入库对读数没有
/// 可见影响;而活跃会话每条消息都在追加,不节流就是每 500 ms 走一轮 stat + seek +
/// 读尾段 + 写事务。跳过不会丢数据 —— run_loop 的兜底扫描会再来,届时节流窗口已过。
///
/// `force = true` 时无条件解析,给显式刷新命令用（用户点了"刷新"就该立刻看到）。
fn should_reparse(
    indexed: Option<&IndexedSource>,
    current: SourceState,
    now: i64,
    force: bool,
) -> bool {
    let Some(previous) = indexed else {
        return true;
    };
    if previous.state == current {
        return false;
    }
    if force {
        return true;
    }
    now.saturating_sub(previous.indexed_at) >= REPARSE_THROTTLE.as_millis() as i64
}

pub(crate) fn refresh_index() -> Result<bool, String> {
    refresh_index_inner(false)
}

fn refresh_index_inner(force: bool) -> Result<bool, String> {
    let mut connection = open_database()?;
    let indexed = load_source_states(&connection)?;
    let mut files = HashSet::new();
    for root in analytics::usage_roots() {
        analytics::collect_jsonl_files(&root, &mut files);
    }

    let now = unix_millis();
    let mut changed = false;
    let mut seen = HashSet::new();
    for path in files {
        let canonical = path.to_string_lossy().into_owned();
        seen.insert(canonical.clone());
        let Some(state) = source_state(&path) else {
            continue;
        };
        if !should_reparse(indexed.get(&canonical), state, now, force) {
            continue;
        }
        // 有游标就先试续扫;续扫判定文件被截断/重写时回落全量重读。
        let scan = indexed
            .get(&canonical)
            .and_then(|entry| entry.cursor.as_ref())
            .and_then(|cursor| scan_source_incremental(&path, cursor))
            .or_else(|| scan_source_full(&path));
        let Some(scan) = scan else {
            continue;
        };
        write_source_scan(&mut connection, &canonical, state, &scan)?;
        changed = true;
    }

    let removed = indexed
        .keys()
        .filter(|path| !seen.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    if !removed.is_empty() {
        remove_sources(&mut connection, &removed)?;
        changed = true;
    }
    Ok(changed)
}

/// `agent` 列的字面量 → [`UsageAgent`]。与 [`replace_source`] 里写库的那个 match 互为
/// 逆向,两边必须同时改。
///
/// 认不出来回 `None`,调用方**跳过这一行**而不是归到某一族。原来这里是
/// `_ => UsageAgent::Claude`:库里只可能出现上面那四个字面量(写入侧同一个 match 产出),
/// 所以认不出来意味着降级回滚留下的旧行或库被写坏 —— 那种行记到 claude 名下,用户看到的
/// 是 claude 用量凭空变多,而且和 breakdown 四族之和对不上。宁可少算不可错算。
fn parse_usage_agent(value: &str) -> Option<UsageAgent> {
    match value {
        "codex" => Some(UsageAgent::Codex),
        "claude" => Some(UsageAgent::Claude),
        "dsh" => Some(UsageAgent::Dsh),
        "omp" => Some(UsageAgent::Omp),
        _ => None,
    }
}

pub(crate) fn load_requests(
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<UsageRequest>, String> {
    let connection = open_database()?;
    load_requests_from(&connection, from, to)
}

fn load_requests_from(
    connection: &Connection,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<UsageRequest>, String> {
    let mut statement = connection
        .prepare(
            "
            SELECT
                timestamp,
                date,
                agent,
                model,
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens
            FROM usage_requests
            WHERE date >= ?1 AND date <= ?2
            ORDER BY timestamp ASC
            ",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![from.to_string(), to.to_string()], |row| {
            let date = row
                .get::<_, String>(1)?
                .parse::<chrono::NaiveDate>()
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            let Some(agent) = parse_usage_agent(&row.get::<_, String>(2)?) else {
                return Ok(None);
            };
            Ok(Some(UsageRequest {
                timestamp: row.get(0)?,
                date,
                agent,
                model: row.get(3)?,
                input_tokens: row.get::<_, i64>(4)?.max(0) as u64,
                output_tokens: row.get::<_, i64>(5)?.max(0) as u64,
                cache_creation_tokens: row.get::<_, i64>(6)?.max(0) as u64,
                cache_read_tokens: row.get::<_, i64>(7)?.max(0) as u64,
            }))
        })
        .map_err(|error| error.to_string())?;
    let mut requests = Vec::new();
    for row in rows {
        // 认不出 agent 的行在这里被丢掉,不进任何一族的统计。
        if let Some(request) = row.map_err(|error| error.to_string())? {
            requests.push(request);
        }
    }
    Ok(requests)
}

pub(crate) fn latest_updated_at() -> Result<i64, String> {
    let connection = open_database()?;
    connection
        .query_row("SELECT MAX(indexed_at) FROM usage_sources", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .optional()
        .map(|value| value.flatten().unwrap_or(0))
        .map_err(|error| error.to_string())
}

fn emit_updated(app: &AppHandle) {
    let _ = app.emit(
        INDEX_EVENT,
        serde_json::json!({ "updatedAt": latest_updated_at().unwrap_or_default() }),
    );
}

fn sync_watched_roots(
    watcher: &mut Option<notify::RecommendedWatcher>,
    watched: &mut HashSet<PathBuf>,
) {
    let Some(watcher) = watcher.as_mut() else {
        return;
    };
    for root in analytics::usage_roots() {
        if watched.insert(root.clone()) && watcher.watch(&root, RecursiveMode::Recursive).is_err() {
            watched.remove(&root);
        }
    }
}

fn run_loop(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::RecommendedWatcher::new(tx, notify::Config::default()).ok();
    let mut watched = HashSet::new();

    loop {
        sync_watched_roots(&mut watcher, &mut watched);
        if refresh_index().unwrap_or(false) {
            emit_updated(&app);
        }

        if watcher.is_some() {
            match rx.recv_timeout(FALLBACK_SCAN_INTERVAL) {
                Ok(_) => thread::sleep(EVENT_DEBOUNCE),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => watcher = None,
            }
            while rx.try_recv().is_ok() {}
        } else {
            thread::sleep(FALLBACK_SCAN_INTERVAL);
        }
    }
}

pub(crate) fn start(app: AppHandle) {
    thread::spawn(move || run_loop(app));
}

#[tauri::command]
pub(crate) async fn refresh_usage_statistics_index(app: AppHandle) -> Result<bool, String> {
    // 显式刷新绕过节流:用户点了刷新就该立刻看到最新数字。
    let changed = tokio::task::spawn_blocking(|| refresh_index_inner(true))
        .await
        .map_err(|error| format!("refresh_usage_statistics_index join error: {error}"))??;
    if changed {
        emit_updated(&app);
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn indexed(size: i64, indexed_at: i64) -> IndexedSource {
        IndexedSource {
            state: SourceState {
                modified_ns: size * 1000,
                size,
            },
            indexed_at,
            cursor: None,
        }
    }

    /// 整份替换,不留游标。生产路径一律走 [`write_source_scan`] 带扫描结果,这个
    /// 壳子只服务于「直接造记录」的用例。
    fn replace_source(
        connection: &mut Connection,
        path: &str,
        state: SourceState,
        requests: Vec<UsageRequest>,
    ) -> Result<(), String> {
        write_source_scan(
            connection,
            path,
            state,
            &SourceScan {
                requests,
                base_index: 0,
                cursor: None,
            },
        )
    }

    fn temp_path(suffix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("aeroric-usage-{}{suffix}", Uuid::new_v4()))
    }

    /// 造一条 claude assistant 行。`id` 是 `message.id`:同一次 API 响应的多个
    /// content block 共享它,整份解析按它去重。
    fn claude_line(id: &str, uuid: &str, output_tokens: u64, minute: u32) -> String {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","timestamp":"2026-09-01T10:{minute:02}:00.000Z","message":{{"id":"{id}","model":"claude-opus-5","usage":{{"input_tokens":10,"output_tokens":{output_tokens},"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        )
    }

    /// 造一条 claude user 行 —— 不产出用量记录,是 claude 家族的封存边界。
    fn claude_user_line(uuid: &str) -> String {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","timestamp":"2026-09-01T10:30:00.000Z","message":{{"role":"user","content":"go"}}}}"#
        )
    }

    /// 扫一遍某个来源并落库,返回这一轮走的是续扫还是全量。
    fn scan_round(connection: &mut Connection, path: &Path) -> bool {
        let canonical = path.to_string_lossy().into_owned();
        let state = source_state(path).expect("source state");
        let cursor = load_source_states(connection)
            .expect("load cursors")
            .get(&canonical)
            .and_then(|entry| entry.cursor.clone());
        let incremental = cursor
            .as_ref()
            .and_then(|cursor| scan_source_incremental(path, cursor));
        let was_incremental = incremental.is_some();
        let scan = incremental
            .or_else(|| scan_source_full(path))
            .expect("scan");
        write_source_scan(connection, &canonical, state, &scan).expect("write scan");
        was_incremental
    }

    fn total_output_tokens(connection: &Connection, path: &Path) -> i64 {
        connection
            .query_row(
                "SELECT COALESCE(SUM(output_tokens), 0) FROM usage_requests WHERE source_path = ?1",
                params![path.to_string_lossy().into_owned()],
                |row| row.get(0),
            )
            .expect("sum output tokens")
    }

    fn row_count(connection: &Connection, path: &Path) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE source_path = ?1",
                params![path.to_string_lossy().into_owned()],
                |row| row.get(0),
            )
            .expect("count rows")
    }

    /// 按 `request_index` 顺序取回入库的 model 列。续扫的记录靠游标带过来的
    /// `model` 状态填这一列,丢状态时这里会看到空串。
    fn stored_models(connection: &Connection, path: &Path) -> Vec<String> {
        let mut statement = connection
            .prepare(
                "
                SELECT model FROM usage_requests
                WHERE source_path = ?1
                ORDER BY request_index
                ",
            )
            .expect("prepare models");
        let rows = statement
            .query_map(params![path.to_string_lossy().into_owned()], |row| {
                row.get::<_, String>(0)
            })
            .expect("query models");
        rows.map(|row| row.expect("model row")).collect()
    }

    fn stored_cursor(connection: &Connection, path: &Path) -> Option<SourceCursor> {
        load_source_states(connection)
            .expect("load cursors")
            .get(&path.to_string_lossy().into_owned())
            .and_then(|entry| entry.cursor.clone())
    }

    fn append(path: &Path, text: &str) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open");
        file.write_all(text.as_bytes()).expect("append");
    }

    #[test]
    fn a_new_source_is_always_parsed() {
        let state = SourceState {
            modified_ns: 1,
            size: 10,
        };
        assert!(should_reparse(None, state, 0, false));
    }

    #[test]
    fn an_unchanged_source_is_never_reparsed() {
        let previous = indexed(10, 0);
        // 指纹相同 → 跳过,连 force 也不必解析(内容确实没动)。
        assert!(!should_reparse(
            Some(&previous),
            previous.state,
            10_000_000,
            false
        ));
        assert!(!should_reparse(
            Some(&previous),
            previous.state,
            10_000_000,
            true
        ));
    }

    /// 活跃会话每条消息都在追加。节流窗口内的变化要压住,否则就是反复整文件重解析。
    #[test]
    fn a_growing_source_is_throttled_then_reparsed() {
        let previous = indexed(10, 1_000);
        let grown = SourceState {
            modified_ns: 99_000,
            size: 20,
        };
        let window = REPARSE_THROTTLE.as_millis() as i64;

        assert!(!should_reparse(
            Some(&previous),
            grown,
            1_000 + window - 1,
            false
        ));
        assert!(should_reparse(
            Some(&previous),
            grown,
            1_000 + window,
            false
        ));
    }

    /// 显式刷新不受节流影响。
    #[test]
    fn force_bypasses_the_throttle_for_changed_sources() {
        let previous = indexed(10, 1_000);
        let grown = SourceState {
            modified_ns: 99_000,
            size: 20,
        };
        assert!(should_reparse(Some(&previous), grown, 1_001, true));
    }

    #[test]
    fn source_state_changes_when_file_grows() {
        let path = temp_path(".jsonl");
        fs::write(&path, "{}\n").unwrap();
        let before = source_state(&path).unwrap();
        fs::write(&path, "{}\n{}\n").unwrap();
        let after = source_state(&path).unwrap();
        assert!(after.size > before.size);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn parses_zstd_compressed_dsh_sessions() {
        let path = temp_path(".session.jsonl.zstd");
        let content = concat!(
            r#"{"type":"session","version":0,"id":"s1","createdAt":1755100000000}"#,
            "\n",
            r#"{"type":"request/context","seq":0,"time":1755100000100,"data":{"provider":"deepseek-official","model":"deepseek-v4"}}"#,
            "\n",
            r#"{"type":"assistant/message","seq":1,"time":1755100001000,"data":{"usage":{"inputTokens":100,"outputTokens":20,"cacheReadTokens":50,"cacheWriteTokens":5,"reasoningTokens":7}}}"#,
            "\n",
        );
        let compressed = zstd::stream::encode_all(content.as_bytes(), 1).unwrap();
        fs::write(&path, compressed).unwrap();

        let requests = scan_source_full(&path).unwrap().requests;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].agent, UsageAgent::Dsh);
        assert_eq!(requests[0].input_tokens, 100);
        assert_eq!(requests[0].output_tokens, 20);
        assert_eq!(requests[0].cache_read_tokens, 50);
        assert_eq!(requests[0].cache_creation_tokens, 5);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn streamed_dsh_scanner_rejects_excessive_decompressed_data() {
        let content = concat!(
            r#"{"type":"session","version":0,"id":"s1","createdAt":1755100000000}"#,
            "\n",
            r#"{"type":"request/context","seq":0,"time":1755100000100,"data":{"provider":"deepseek-official","model":"deepseek-v4"}}"#,
            "\n",
            r#"{"type":"assistant/message","seq":1,"time":1755100001000,"data":{"usage":{"inputTokens":100,"outputTokens":20}}}"#,
            "\n",
        );
        let result = scan_dsh_stream(
            BufReader::new(std::io::Cursor::new(content.as_bytes())),
            content.len() - 1,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_corrupt_zstd_without_producing_empty_usage() {
        let path = temp_path(".session.jsonl.zstd");
        fs::write(&path, b"not-zstd").unwrap();
        assert!(scan_source_full(&path).is_none());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn corrupt_rewrite_does_not_replace_previously_indexed_requests() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let path = temp_path(".session.jsonl.zstd");
        let source_path = path.to_string_lossy().into_owned();
        let request = UsageRequest {
            timestamp: 1.0,
            date: chrono::NaiveDate::from_ymd_opt(2026, 8, 19).unwrap(),
            agent: UsageAgent::Dsh,
            model: "deepseek-v4".to_owned(),
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_tokens: 0,
            cache_read_tokens: 2,
        };
        replace_source(
            &mut connection,
            &source_path,
            SourceState {
                modified_ns: 1,
                size: 100,
            },
            vec![request],
        )
        .unwrap();

        fs::write(&path, b"partial-zstd-frame").unwrap();
        assert!(scan_source_full(&path).is_none());
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE source_path = ?1",
                params![source_path],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn replacing_a_source_does_not_duplicate_requests() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let state = SourceState {
            modified_ns: 1,
            size: 100,
        };
        let request = UsageRequest {
            timestamp: 1.0,
            date: chrono::NaiveDate::from_ymd_opt(2026, 7, 16).unwrap(),
            agent: UsageAgent::Codex,
            model: "gpt-5.5".to_owned(),
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_tokens: 0,
            cache_read_tokens: 2,
        };

        replace_source(
            &mut connection,
            "/tmp/session.jsonl",
            state,
            vec![request.clone()],
        )
        .unwrap();
        replace_source(
            &mut connection,
            "/tmp/session.jsonl",
            SourceState {
                modified_ns: 2,
                size: 120,
            },
            vec![request],
        )
        .unwrap();

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── 增量字节游标 ───────────────────────────────────────────────────────

    const DSH_HEADER: &str =
        r#"{"type":"session","version":0,"id":"s1","createdAt":1755100000000}"#;

    fn dsh_context(model: &str) -> String {
        format!(
            r#"{{"type":"request/context","seq":0,"time":1755100000100,"data":{{"provider":"p","model":"{model}"}}}}"#
        )
    }

    fn dsh_message(time_ms: i64, output_tokens: u64) -> String {
        format!(
            r#"{{"type":"assistant/message","seq":1,"time":{time_ms},"data":{{"usage":{{"inputTokens":10,"outputTokens":{output_tokens}}}}}}}"#
        )
    }

    /// 续扫只读新增尾段,已封存前缀的记录既不重复计数也不丢。
    #[test]
    fn incremental_rounds_do_not_recount_the_sealed_prefix() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let path = temp_path(".jsonl");
        fs::write(
            &path,
            format!(
                "{DSH_HEADER}\n{}\n{}\n",
                dsh_context("deepseek-v4"),
                dsh_message(1755100001000, 20)
            ),
        )
        .unwrap();

        assert!(
            !scan_round(&mut connection, &path),
            "首轮没有游标,必须是全量扫描"
        );
        assert_eq!(row_count(&connection, &path), 1);
        assert_eq!(total_output_tokens(&connection, &path), 20);
        let first = stored_cursor(&connection, &path).expect("首轮应写下游标");
        assert_eq!(first.offset, fs::metadata(&path).unwrap().len());
        assert_eq!(first.rows, 1);
        assert_eq!(first.format, SourceFormat::Dsh);
        assert_eq!(first.model, "deepseek-v4", "封存点要记住跨行 model 状态");

        append(&path, &format!("{}\n", dsh_message(1755100002000, 30)));
        assert!(scan_round(&mut connection, &path), "第二轮应走续扫");
        assert_eq!(row_count(&connection, &path), 2);
        assert_eq!(
            total_output_tokens(&connection, &path),
            50,
            "续扫把已封存前缀又算了一遍"
        );

        append(&path, &format!("{}\n", dsh_message(1755100003000, 40)));
        assert!(scan_round(&mut connection, &path), "第三轮应走续扫");
        assert_eq!(row_count(&connection, &path), 3);
        assert_eq!(total_output_tokens(&connection, &path), 90);

        // 尾段的 model 必须由游标带过来:新增的 assistant/message 前面没有
        // request/context,整份解析给它的 model 就是最后一次路由的模型。
        assert_eq!(stored_models(&connection, &path), vec!["deepseek-v4"; 3]);

        let _ = fs::remove_file(path);
    }

    /// 文件被截断/改写成更短的内容:游标超过当前文件长度,必须整份重读,
    /// 库里只剩新内容,而且重建的游标还能继续续扫。
    #[test]
    fn a_truncated_file_is_reread_in_full() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let path = temp_path(".jsonl");
        fs::write(
            &path,
            format!(
                "{DSH_HEADER}\n{}\n{}\n{}\n{}\n",
                dsh_context("deepseek-v4"),
                dsh_message(1755100001000, 20),
                dsh_message(1755100002000, 30),
                dsh_message(1755100003000, 40)
            ),
        )
        .unwrap();
        scan_round(&mut connection, &path);
        assert_eq!(row_count(&connection, &path), 3);
        let long_offset = stored_cursor(&connection, &path).expect("游标").offset;

        // 换成更短的内容:偏移落到文件之外。
        fs::write(
            &path,
            format!(
                "{DSH_HEADER}\n{}\n{}\n",
                dsh_context("deepseek-v4"),
                dsh_message(1755100009000, 7)
            ),
        )
        .unwrap();
        let short_size = fs::metadata(&path).unwrap().len();
        assert!(short_size < long_offset, "夹具要让游标越过文件末尾");

        assert!(
            !scan_round(&mut connection, &path),
            "游标越过文件末尾时不能续扫"
        );
        assert_eq!(row_count(&connection, &path), 1, "旧文件的行必须被清掉");
        assert_eq!(total_output_tokens(&connection, &path), 7);
        let rebuilt = stored_cursor(&connection, &path).expect("重读后要重建游标");
        assert_eq!(rebuilt.offset, short_size);
        assert_eq!(rebuilt.rows, 1);

        // 重建的游标必须真的可用:再追加一条要走续扫且计数正确。
        append(&path, &format!("{}\n", dsh_message(1755100010000, 11)));
        assert!(scan_round(&mut connection, &path), "重读之后应恢复续扫");
        assert_eq!(row_count(&connection, &path), 2);
        assert_eq!(total_output_tokens(&connection, &path), 18);

        let _ = fs::remove_file(path);
    }

    /// 半行不能被封存:补全之后必须被记上,而且只记一次。
    ///
    /// 这是「按行号数游标」的老 bug —— 半行先算一次,补全后又被跳过,那条记录永久丢失。
    #[test]
    fn a_half_written_line_is_counted_once_after_it_completes() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let path = temp_path(".jsonl");
        fs::write(
            &path,
            format!(
                "{DSH_HEADER}\n{}\n{}\n",
                dsh_context("deepseek-v4"),
                dsh_message(1755100001000, 20)
            ),
        )
        .unwrap();
        scan_round(&mut connection, &path);
        let sealed = stored_cursor(&connection, &path).expect("游标").offset;

        // 写了一半的 JSON:解析不出来,也不能把它算进封存偏移。
        let complete = dsh_message(1755100002000, 30);
        let (head, rest) = complete.split_at(complete.len() / 2);
        append(&path, head);
        assert!(scan_round(&mut connection, &path), "追加后应走续扫");
        assert_eq!(row_count(&connection, &path), 1, "半行不是一条记录");
        assert_eq!(total_output_tokens(&connection, &path), 20);
        assert_eq!(
            stored_cursor(&connection, &path).expect("游标").offset,
            sealed,
            "半行被封存了,补全后会被永久跳过"
        );

        // 补全这一行。
        append(&path, &format!("{rest}\n"));
        assert!(scan_round(&mut connection, &path), "补全后应走续扫");
        assert_eq!(row_count(&connection, &path), 2, "补全的行必须被记上");
        assert_eq!(total_output_tokens(&connection, &path), 50);
        assert_eq!(
            stored_cursor(&connection, &path).expect("游标").offset,
            fs::metadata(&path).unwrap().len(),
            "补全后整行都该封存"
        );

        let _ = fs::remove_file(path);
    }

    /// 完整但没有换行结尾的末行:照常入库(文件可能本来就不以换行结尾),
    /// 但不封存 —— 下一轮从封存点重读,换行落地后仍然只算一次。
    #[test]
    fn an_unterminated_complete_line_is_imported_but_not_sealed() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let path = temp_path(".jsonl");
        fs::write(
            &path,
            format!(
                "{DSH_HEADER}\n{}\n{}\n",
                dsh_context("deepseek-v4"),
                dsh_message(1755100001000, 20)
            ),
        )
        .unwrap();
        scan_round(&mut connection, &path);
        let sealed = stored_cursor(&connection, &path).expect("游标").offset;

        append(&path, &dsh_message(1755100002000, 30));
        scan_round(&mut connection, &path);
        assert_eq!(row_count(&connection, &path), 2, "完整的末行要入库");
        assert_eq!(total_output_tokens(&connection, &path), 50);
        assert_eq!(
            stored_cursor(&connection, &path).expect("游标").offset,
            sealed,
            "没有换行结尾的末行不能封存"
        );

        append(&path, "\n");
        scan_round(&mut connection, &path);
        assert_eq!(row_count(&connection, &path), 2, "换行落地后不该多出一条");
        assert_eq!(total_output_tokens(&connection, &path), 50);

        let _ = fs::remove_file(path);
    }

    /// claude 的封存点不能落在「共享 message.id 的连续多行」之中,否则同一次
    /// API 响应会被前缀和尾段各记一次。
    #[test]
    fn a_claude_dedup_group_is_never_split_by_the_cursor() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let path = temp_path(".jsonl");
        // 一次响应的两个 content block:同一个 message.id、各自的 uuid、同一份 usage。
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                claude_user_line("u-1"),
                claude_line("msg-1", "e-1", 40, 1)
            ),
        )
        .unwrap();
        scan_round(&mut connection, &path);
        assert_eq!(row_count(&connection, &path), 1);
        assert_eq!(total_output_tokens(&connection, &path), 40);

        append(&path, &format!("{}\n", claude_line("msg-1", "e-2", 55, 2)));
        scan_round(&mut connection, &path);
        assert_eq!(
            row_count(&connection, &path),
            1,
            "同一个 message.id 的两行被算成两条请求"
        );
        assert_eq!(
            total_output_tokens(&connection, &path),
            55,
            "去重应保留时间戳更晚的那条"
        );

        // 下一条 user 行给出封存边界,再来一次响应仍然只算一条。
        append(
            &path,
            &format!(
                "{}\n{}\n",
                claude_user_line("u-2"),
                claude_line("msg-2", "e-3", 7, 3)
            ),
        );
        scan_round(&mut connection, &path);
        assert_eq!(row_count(&connection, &path), 2);
        assert_eq!(total_output_tokens(&connection, &path), 62);

        let _ = fs::remove_file(path);
    }

    /// 同尺寸重写靠 size 看不出来,只有游标边界指纹能发现 —— 发现后整份重读。
    #[test]
    fn a_same_size_rewrite_is_caught_by_the_boundary_fingerprint() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let path = temp_path(".jsonl");
        let original = format!(
            "{DSH_HEADER}\n{}\n{}\n",
            dsh_context("deepseek-v4"),
            dsh_message(1755100001000, 20)
        );
        fs::write(&path, &original).unwrap();
        scan_round(&mut connection, &path);
        assert_eq!(total_output_tokens(&connection, &path), 20);

        // 只改 token 数,字节数不变。
        let rewritten = format!(
            "{DSH_HEADER}\n{}\n{}\n",
            dsh_context("deepseek-v4"),
            dsh_message(1755100001000, 99)
        );
        assert_eq!(rewritten.len(), original.len(), "夹具必须是同尺寸重写");
        fs::write(&path, &rewritten).unwrap();

        assert!(
            !scan_round(&mut connection, &path),
            "指纹失配时不能继续续扫"
        );
        assert_eq!(row_count(&connection, &path), 1);
        assert_eq!(
            total_output_tokens(&connection, &path),
            99,
            "重写后的内容没有被重读"
        );

        let _ = fs::remove_file(path);
    }

    /// codex 的封存点停在最后一条产出记录之后,之后的 `turn_context` 留在尾段;
    /// 续扫时把封存点的 model 回灌给解析器,新增的 token_count 不会丢模型。
    #[test]
    fn codex_incremental_rounds_keep_the_model_state() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let path = temp_path(".jsonl");
        let token_count = |time: &str, input: u64, output: u64| {
            format!(
                r#"{{"type":"event_msg","timestamp":"{time}","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":{output},"reasoning_output_tokens":0}}}}}}}}"#
            )
        };
        fs::write(
            &path,
            format!(
                "{}\n{}\n{}\n",
                r#"{"type":"session_meta","timestamp":"2026-09-01T10:00:00.000Z","payload":{"id":"c1"}}"#,
                r#"{"type":"turn_context","timestamp":"2026-09-01T10:00:01.000Z","payload":{"model":"gpt-5.5"}}"#,
                token_count("2026-09-01T10:00:02.000Z", 10, 20)
            ),
        )
        .unwrap();
        scan_round(&mut connection, &path);
        let cursor = stored_cursor(&connection, &path).expect("游标");
        assert_eq!(cursor.format, SourceFormat::Codex);
        assert_eq!(cursor.model, "gpt-5.5");
        assert_eq!(cursor.rows, 1);

        append(
            &path,
            &format!("{}\n", token_count("2026-09-01T10:00:03.000Z", 5, 6)),
        );
        assert!(scan_round(&mut connection, &path), "第二轮应走续扫");
        assert_eq!(row_count(&connection, &path), 2);
        assert_eq!(total_output_tokens(&connection, &path), 26);
        assert_eq!(
            stored_models(&connection, &path),
            vec!["gpt-5.5"; 2],
            "续扫的记录丢了 model:封存点的状态没有回灌给解析器"
        );

        let _ = fs::remove_file(path);
    }

    /// 存量库(没有游标列)迁移后:列补上、游标为空、历史用量一条不少。
    #[test]
    fn migrating_a_legacy_database_keeps_history_and_leaves_the_cursor_empty() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE usage_sources (
                    path TEXT PRIMARY KEY,
                    modified_ns INTEGER NOT NULL,
                    size INTEGER NOT NULL,
                    indexed_at INTEGER NOT NULL
                );
                CREATE TABLE usage_requests (
                    source_path TEXT NOT NULL,
                    request_index INTEGER NOT NULL,
                    timestamp REAL NOT NULL,
                    date TEXT NOT NULL,
                    agent TEXT NOT NULL,
                    model TEXT NOT NULL,
                    input_tokens INTEGER NOT NULL,
                    output_tokens INTEGER NOT NULL,
                    cache_creation_tokens INTEGER NOT NULL,
                    cache_read_tokens INTEGER NOT NULL,
                    PRIMARY KEY (source_path, request_index)
                );
                INSERT INTO usage_sources VALUES ('/tmp/old.jsonl', 7, 42, 100);
                INSERT INTO usage_requests VALUES
                    ('/tmp/old.jsonl', 0, 1.0, '2026-08-01', 'claude', 'claude-opus-5', 11, 22, 0, 0);
                ",
            )
            .unwrap();

        initialize_database(&connection).unwrap();

        let states = load_source_states(&connection).unwrap();
        let entry = states.get("/tmp/old.jsonl").expect("存量来源不能丢");
        assert_eq!(entry.state.size, 42);
        assert!(entry.cursor.is_none(), "存量行不该凭空得到一个游标");
        let day = chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let loaded = load_requests_from(&connection, day, day).unwrap();
        assert_eq!(loaded.len(), 1, "迁移把历史用量弄丢了");
        assert_eq!(loaded[0].output_tokens, 22);

        // 幂等:再跑一次不报错(ALTER 只在缺列时执行)。
        initialize_database(&connection).unwrap();
        assert!(load_source_states(&connection)
            .unwrap()
            .contains_key("/tmp/old.jsonl"));
    }

    // ── agent 列的读写映射 ─────────────────────────────────────────────────

    /// 下一个 [`UsageAgent`]。穷尽 `match`:新增一族时这里不编译,于是下面那条往返测试
    /// 一定会覆盖到它。
    fn next_agent(agent: UsageAgent) -> Option<UsageAgent> {
        match agent {
            UsageAgent::Codex => Some(UsageAgent::Claude),
            UsageAgent::Claude => Some(UsageAgent::Dsh),
            UsageAgent::Dsh => Some(UsageAgent::Omp),
            UsageAgent::Omp => None,
        }
    }

    fn all_agents() -> Vec<UsageAgent> {
        let mut agents = vec![UsageAgent::Codex];
        while let Some(next) = next_agent(*agents.last().expect("至少有 Codex")) {
            assert!(!agents.contains(&next), "next_agent 成环:{next:?}");
            agents.push(next);
        }
        agents
    }

    /// 写进去的每一族都要原样读回来。两个 match 反向不一致 = 用量记到别人名下。
    #[test]
    fn every_agent_survives_a_database_round_trip() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();

        let agents = all_agents();
        assert_eq!(agents.len(), 4, "清单漏了 agent");
        for (index, agent) in agents.iter().enumerate() {
            replace_source(
                &mut connection,
                &format!("/tmp/session-{index}.jsonl"),
                SourceState {
                    modified_ns: 1,
                    size: 10,
                },
                vec![UsageRequest {
                    timestamp: index as f64,
                    date: day,
                    agent: *agent,
                    model: "m".to_owned(),
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_creation_tokens: 0,
                    cache_read_tokens: 0,
                }],
            )
            .unwrap();
        }

        let loaded = load_requests_from(&connection, day, day).unwrap();
        assert_eq!(
            loaded
                .iter()
                .map(|request| request.agent)
                .collect::<Vec<_>>(),
            agents,
            "读回来的 agent 与写进去的不一致"
        );
    }

    /// 认不出的 agent 必须被跳过,而不是记到 claude 名下。
    ///
    /// 这种行只可能来自降级回滚留下的旧库或写坏的库(写入侧只产出四个已知字面量)。
    /// 原来的 `_ => UsageAgent::Claude` 会让它变成 claude 的用量:面板上 claude 凭空多出
    /// 一截,且总计与四族之和对不上。用裸 SQL 造这一行 —— 类型安全的写入路径造不出来。
    #[test]
    fn an_unrecognized_agent_is_skipped_instead_of_counted_as_claude() {
        let mut connection = Connection::open_in_memory().unwrap();
        initialize_database(&connection).unwrap();
        let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 2).unwrap();

        replace_source(
            &mut connection,
            "/tmp/known.jsonl",
            SourceState {
                modified_ns: 1,
                size: 10,
            },
            vec![UsageRequest {
                timestamp: 1.0,
                date: day,
                agent: UsageAgent::Claude,
                model: "claude-sonnet-5".to_owned(),
                input_tokens: 7,
                output_tokens: 3,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            }],
        )
        .unwrap();
        connection
            .execute(
                "
                INSERT INTO usage_requests (
                    source_path, request_index, timestamp, date, agent, model,
                    input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens
                ) VALUES ('/tmp/mystery.jsonl', 0, 2.0, ?1, 'gemini', 'gemini-3', 999, 999, 0, 0)
                ",
                params![day.to_string()],
            )
            .unwrap();

        let loaded = load_requests_from(&connection, day, day).unwrap();
        assert_eq!(loaded.len(), 1, "认不出的行不该进结果");
        assert_eq!(loaded[0].agent, UsageAgent::Claude);
        assert_eq!(loaded[0].input_tokens, 7, "claude 的 input 不该被那行污染");
        assert_eq!(loaded[0].model, "claude-sonnet-5");
    }

    #[test]
    fn the_agent_column_mapping_is_reversible() {
        for agent in all_agents() {
            assert_eq!(
                parse_usage_agent(usage_agent_column(agent)),
                Some(agent),
                "{agent:?} 的读写映射不自洽"
            );
        }
        assert_eq!(parse_usage_agent("gemini"), None);
        assert_eq!(parse_usage_agent(""), None);
    }
}
