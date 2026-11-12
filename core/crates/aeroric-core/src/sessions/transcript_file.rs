//! 2026-11-12: one way to read an agent's session file, compressed or not.
//!
//! Codex compresses rollouts it no longer writes to `rollout-….jsonl.zst`
//! and deletes the `.jsonl` (seen 2026-11-11 21:27 on the owner's Mac).
//! Task records keep the path Codex gave when the session ran, so a reader
//! that opened that path found nothing and the session showed empty. Every
//! reader goes through here: the path as recorded, else its `.zst` beside
//! it, decompressed in memory. Nothing on disk is changed.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const ZSTD_SUFFIX: &str = ".zst";

/// The file holding the session at `path`: the path itself, or the
/// compressed copy beside it.
pub fn resolve(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let mut compressed = path.as_os_str().to_owned();
    compressed.push(ZSTD_SUFFIX);
    let compressed = PathBuf::from(compressed);
    compressed.is_file().then_some(compressed)
}

fn compressed(path: &Path) -> bool {
    path.to_string_lossy().ends_with(ZSTD_SUFFIX)
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("没有会话文件：{}", path.display()),
    )
}

/// The session's lines, decompressed when need be.
pub fn open(path: &Path) -> io::Result<Box<dyn BufRead + Send>> {
    let file_path = resolve(path).ok_or_else(|| not_found(path))?;
    let file = File::open(&file_path)?;
    Ok(if compressed(&file_path) {
        Box::new(BufReader::new(zstd::stream::read::Decoder::new(file)?))
    } else {
        Box::new(BufReader::new(file))
    })
}

pub fn read_to_string(path: &Path) -> io::Result<String> {
    let mut text = String::new();
    open(path)?.read_to_string(&mut text)?;
    Ok(text)
}

/// Something to read the session from at any offset.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// The session for reading at byte offsets, and its length. A compressed
/// file is decompressed into memory first (it is a finished session).
pub fn seekable(path: &Path) -> io::Result<(Box<dyn ReadSeek>, u64)> {
    let file_path = resolve(path).ok_or_else(|| not_found(path))?;
    if compressed(&file_path) {
        let mut bytes = Vec::new();
        open(path)?.read_to_end(&mut bytes)?;
        let len = bytes.len() as u64;
        return Ok((Box::new(io::Cursor::new(bytes)), len));
    }
    let file = File::open(&file_path)?;
    let len = file.metadata()?.len();
    Ok((Box::new(file), len))
}

/// The last `max_bytes` of the session (the first line may be cut). A
/// compressed file is a finished session: it is read whole, then cut.
pub fn tail(path: &Path, max_bytes: u64) -> io::Result<Vec<u8>> {
    let file_path = resolve(path).ok_or_else(|| not_found(path))?;
    let mut bytes = Vec::new();
    if compressed(&file_path) {
        open(path)?.read_to_end(&mut bytes)?;
        let keep = bytes.len().saturating_sub(max_bytes as usize);
        bytes.drain(..keep);
    } else {
        let mut file = File::open(&file_path)?;
        let len = file.metadata()?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(max_bytes)))?;
        file.read_to_end(&mut bytes)?;
    }
    Ok(bytes)
}

/// Copies the session to `to` as a plain file, decompressing it when need
/// be: the copy is a session another harness carries on and appends to.
pub fn copy_plain(from: &Path, to: &Path) -> io::Result<u64> {
    let mut reader = open(from)?;
    io::copy(&mut reader, &mut File::create(to)?)
}

/// A file name that is a session log: `….jsonl`, or Codex's `….jsonl.zst`.
pub fn is_session_log(name: &str) -> bool {
    name.ends_with(".jsonl") || name.ends_with(".jsonl.zst")
}

/// `….jsonl` for a compressed name, so callers that parse names (the
/// rollout's time, its id) see the name Codex first gave.
pub fn plain_name(name: &str) -> &str {
    name.strip_suffix(ZSTD_SUFFIX).unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aeroric-transcript-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_compressed_session_reads_like_the_plain_one() {
        let dir = dir();
        let text = "{\"a\":1}\n{\"b\":2}\n{\"c\":3}\n";
        let plain = dir.join("rollout-1.jsonl");
        std::fs::write(&plain, text).unwrap();
        assert_eq!(read_to_string(&plain).unwrap(), text);
        assert_eq!(tail(&plain, 8).unwrap(), b"{\"c\":3}\n");

        // Codex compressed it and removed the original; the record still
        // names the `.jsonl`.
        let gone = dir.join("rollout-2.jsonl");
        std::fs::write(
            dir.join("rollout-2.jsonl.zst"),
            zstd::stream::encode_all(text.as_bytes(), 3).unwrap(),
        )
        .unwrap();
        assert_eq!(resolve(&gone), Some(dir.join("rollout-2.jsonl.zst")));
        assert_eq!(read_to_string(&gone).unwrap(), text);
        assert_eq!(
            read_to_string(&dir.join("rollout-2.jsonl.zst")).unwrap(),
            text
        );
        assert_eq!(tail(&gone, 8).unwrap(), b"{\"c\":3}\n");
        assert_eq!(open(&gone).unwrap().lines().count(), 3);
        let copy = dir.join("copy.jsonl");
        copy_plain(&gone, &copy).unwrap();
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), text);

        let missing = dir.join("rollout-3.jsonl");
        assert_eq!(resolve(&missing), None);
        assert_eq!(
            read_to_string(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn session_log_names() {
        assert!(is_session_log("rollout-x.jsonl"));
        assert!(is_session_log("rollout-x.jsonl.zst"));
        assert!(!is_session_log("rollout-x.json"));
        assert_eq!(plain_name("rollout-x.jsonl.zst"), "rollout-x.jsonl");
        assert_eq!(plain_name("rollout-x.jsonl"), "rollout-x.jsonl");
    }
}
