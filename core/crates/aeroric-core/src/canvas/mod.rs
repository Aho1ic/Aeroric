//! 无限画布一期（UI 改版 M5）：白板与节点布局的持久化。
//!
//! 存储：`<data>/canvas/boards.json`（白板列表 + 全局 revision）与 `<data>/canvas/board-<id>.json`
//! （该板节点），全部原子写；一个进程内的写操作经 [`WRITE_LOCK`] 串行。每次变更 revision + 1。
//! 区域分组按几何包含（M5 D2），这里不存成员表；会话 / 文件引用是软引用，目标消失时节点保留。

use std::fs;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::storage::atomic::{write_atomic, FileMode};

pub const MAX_NODES_PER_BOARD: usize = 2000;
pub const MAX_NOTE_BYTES: usize = 64 * 1024;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_BOARD_NAME_CHARS: usize = 80;
pub const MIN_NODE_SIZE: f64 = 40.0;
pub const MAX_NODE_SIZE: f64 = 10_000.0;
pub const MAX_COORDINATE: f64 = 1_000_000.0;

static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Session,
    Note,
    Region,
    File,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSummary {
    #[serde(flatten)]
    pub board: Board,
    pub node_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub board_id: String,
    pub kind: NodeKind,
    /// kind=session: a task (`taskId`) or an imported session (`importedId`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_id: Option<String>,
    /// kind=file: project-relative `path` inside `projectId`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub z: i64,
    pub updated_at_ms: u64,
}

/// One move / resize in a batch. Absent size keeps the current one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMove {
    pub id: String,
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CanvasError {
    #[error("{0} not found")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("limit reached: {0}")]
    Limit(String),
    #[error("{0}")]
    Io(String),
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BoardsDoc {
    revision: u64,
    boards: Vec<Board>,
}

#[derive(Default, Serialize, Deserialize)]
struct NodesDoc {
    nodes: Vec<Node>,
}

pub struct CanvasStore {
    dir: PathBuf,
}

impl CanvasStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("canvas"),
        }
    }

    // MARK: Boards

    pub fn list_boards(&self) -> Result<(Vec<BoardSummary>, u64), CanvasError> {
        let doc = self.boards_doc()?;
        let mut boards = doc
            .boards
            .into_iter()
            .map(|board| {
                let node_count = self
                    .nodes_doc(&board.id)
                    .map(|d| d.nodes.len())
                    .unwrap_or(0);
                BoardSummary { board, node_count }
            })
            .collect::<Vec<_>>();
        boards.sort_by(|a, b| {
            b.board
                .updated_at_ms
                .cmp(&a.board.updated_at_ms)
                .then(a.board.id.cmp(&b.board.id))
        });
        Ok((boards, doc.revision))
    }

    pub fn create_board(
        &self,
        name: Option<String>,
        color: Option<String>,
        now_ms: u64,
    ) -> Result<(Board, u64), CanvasError> {
        let name = validate_name(name)?;
        let color = validate_color(color)?;
        let _guard = WRITE_LOCK.lock();
        let mut doc = self.boards_doc()?;
        let board = Board {
            id: new_id()?,
            name,
            color,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        };
        doc.boards.push(board.clone());
        doc.revision += 1;
        self.write_nodes(&board.id, &NodesDoc::default())?;
        self.write_boards(&doc)?;
        Ok((board, doc.revision))
    }

    /// `name` / `color`: `None` keeps, `Some(None)` clears, `Some(Some(v))` sets.
    pub fn update_board(
        &self,
        id: &str,
        name: Option<Option<String>>,
        color: Option<Option<String>>,
        now_ms: u64,
    ) -> Result<(Board, u64), CanvasError> {
        let name = name.map(validate_name).transpose()?;
        let color = color.map(validate_color).transpose()?;
        let _guard = WRITE_LOCK.lock();
        let mut doc = self.boards_doc()?;
        let board = doc
            .boards
            .iter_mut()
            .find(|b| b.id == id)
            .ok_or_else(|| CanvasError::NotFound(format!("board {id}")))?;
        if let Some(name) = name {
            board.name = name;
        }
        if let Some(color) = color {
            board.color = color;
        }
        board.updated_at_ms = now_ms;
        let board = board.clone();
        doc.revision += 1;
        self.write_boards(&doc)?;
        Ok((board, doc.revision))
    }

    pub fn delete_board(&self, id: &str) -> Result<u64, CanvasError> {
        let _guard = WRITE_LOCK.lock();
        let mut doc = self.boards_doc()?;
        let before = doc.boards.len();
        doc.boards.retain(|b| b.id != id);
        if doc.boards.len() == before {
            return Err(CanvasError::NotFound(format!("board {id}")));
        }
        doc.revision += 1;
        self.write_boards(&doc)?;
        let _ = fs::remove_file(self.nodes_path(id));
        Ok(doc.revision)
    }

    // MARK: Nodes

    pub fn nodes(&self, board_id: &str) -> Result<(Vec<Node>, u64), CanvasError> {
        let doc = self.boards_doc()?;
        self.require_board(&doc, board_id)?;
        let mut nodes = self.nodes_doc(board_id)?.nodes;
        nodes.sort_by(|a, b| a.z.cmp(&b.z).then(a.id.cmp(&b.id)));
        Ok((nodes, doc.revision))
    }

    /// Create (empty `id`) or replace a node. Server owns id, z on create and timestamps.
    pub fn upsert_node(&self, mut node: Node, now_ms: u64) -> Result<(Node, u64), CanvasError> {
        validate_node(&node)?;
        let _guard = WRITE_LOCK.lock();
        let mut doc = self.boards_doc()?;
        self.require_board(&doc, &node.board_id)?;
        let mut nodes = self.nodes_doc(&node.board_id)?;
        node.updated_at_ms = now_ms;
        match nodes
            .nodes
            .iter_mut()
            .find(|n| !node.id.is_empty() && n.id == node.id)
        {
            Some(existing) => {
                if existing.kind != node.kind {
                    return Err(CanvasError::Invalid("a node's kind cannot change".into()));
                }
                *existing = node.clone();
            }
            None if !node.id.is_empty() => {
                return Err(CanvasError::NotFound(format!("node {}", node.id)));
            }
            None => {
                if nodes.nodes.len() >= MAX_NODES_PER_BOARD {
                    return Err(CanvasError::Limit(format!(
                        "{MAX_NODES_PER_BOARD} nodes per board"
                    )));
                }
                node.id = new_id()?;
                node.z = nodes.nodes.iter().map(|n| n.z).max().unwrap_or(0) + 1;
                nodes.nodes.push(node.clone());
            }
        }
        self.touch_board(&mut doc, &node.board_id, now_ms);
        self.write_nodes(&node.board_id, &nodes)?;
        self.write_boards(&doc)?;
        Ok((node, doc.revision))
    }

    /// All-or-nothing: one unknown id or invalid geometry fails the whole batch.
    pub fn move_nodes(
        &self,
        board_id: &str,
        moves: &[NodeMove],
        now_ms: u64,
    ) -> Result<u64, CanvasError> {
        let _guard = WRITE_LOCK.lock();
        let mut doc = self.boards_doc()?;
        self.require_board(&doc, board_id)?;
        let mut nodes = self.nodes_doc(board_id)?;
        let mut next = nodes.nodes.clone();
        for change in moves {
            let node = next
                .iter_mut()
                .find(|n| n.id == change.id)
                .ok_or_else(|| CanvasError::NotFound(format!("node {}", change.id)))?;
            node.x = change.x;
            node.y = change.y;
            if let Some(width) = change.width {
                node.width = width;
            }
            if let Some(height) = change.height {
                node.height = height;
            }
            node.updated_at_ms = now_ms;
            validate_geometry(node)?;
        }
        nodes.nodes = next;
        self.touch_board(&mut doc, board_id, now_ms);
        self.write_nodes(board_id, &nodes)?;
        self.write_boards(&doc)?;
        Ok(doc.revision)
    }

    pub fn delete_nodes(
        &self,
        board_id: &str,
        ids: &[String],
        now_ms: u64,
    ) -> Result<u64, CanvasError> {
        let _guard = WRITE_LOCK.lock();
        let mut doc = self.boards_doc()?;
        self.require_board(&doc, board_id)?;
        let mut nodes = self.nodes_doc(board_id)?;
        nodes.nodes.retain(|n| !ids.contains(&n.id));
        self.touch_board(&mut doc, board_id, now_ms);
        self.write_nodes(board_id, &nodes)?;
        self.write_boards(&doc)?;
        Ok(doc.revision)
    }

    // MARK: Files

    fn boards_path(&self) -> PathBuf {
        self.dir.join("boards.json")
    }

    fn nodes_path(&self, board_id: &str) -> PathBuf {
        self.dir.join(format!("board-{board_id}.json"))
    }

    fn boards_doc(&self) -> Result<BoardsDoc, CanvasError> {
        read_or_default(&self.boards_path())
    }

    fn nodes_doc(&self, board_id: &str) -> Result<NodesDoc, CanvasError> {
        read_or_default(&self.nodes_path(board_id))
    }

    fn write_boards(&self, doc: &BoardsDoc) -> Result<(), CanvasError> {
        write_json(&self.boards_path(), doc)
    }

    fn write_nodes(&self, board_id: &str, doc: &NodesDoc) -> Result<(), CanvasError> {
        write_json(&self.nodes_path(board_id), doc)
    }

    fn require_board(&self, doc: &BoardsDoc, id: &str) -> Result<(), CanvasError> {
        if !is_valid_id(id) || !doc.boards.iter().any(|b| b.id == id) {
            return Err(CanvasError::NotFound(format!("board {id}")));
        }
        Ok(())
    }

    fn touch_board(&self, doc: &mut BoardsDoc, id: &str, now_ms: u64) {
        if let Some(board) = doc.boards.iter_mut().find(|b| b.id == id) {
            board.updated_at_ms = now_ms;
        }
        doc.revision += 1;
    }
}

fn validate_name(name: Option<String>) -> Result<Option<String>, CanvasError> {
    let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    if name
        .as_ref()
        .is_some_and(|n| n.chars().count() > MAX_BOARD_NAME_CHARS)
    {
        return Err(CanvasError::Invalid(format!(
            "board name over {MAX_BOARD_NAME_CHARS} characters"
        )));
    }
    Ok(name)
}

fn validate_color(color: Option<String>) -> Result<Option<String>, CanvasError> {
    match color {
        Some(c)
            if c.is_empty() || c.len() > 16 || !c.chars().all(|ch| ch.is_ascii_alphanumeric()) =>
        {
            Err(CanvasError::Invalid(
                "color must be a short alphanumeric name".into(),
            ))
        }
        other => Ok(other),
    }
}

fn validate_geometry(node: &Node) -> Result<(), CanvasError> {
    let coords_ok = [node.x, node.y]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= MAX_COORDINATE);
    let size_ok = [node.width, node.height]
        .iter()
        .all(|v| v.is_finite() && (MIN_NODE_SIZE..=MAX_NODE_SIZE).contains(v));
    if coords_ok && size_ok {
        Ok(())
    } else {
        Err(CanvasError::Invalid(format!(
            "node {} has out-of-range geometry",
            node.id
        )))
    }
}

fn validate_node(node: &Node) -> Result<(), CanvasError> {
    validate_geometry(node)?;
    validate_color(node.color.clone())?;
    if !node.id.is_empty() && !is_valid_id(&node.id) {
        return Err(CanvasError::Invalid("malformed node id".into()));
    }
    if node
        .title
        .as_ref()
        .is_some_and(|t| t.chars().count() > MAX_TITLE_CHARS)
    {
        return Err(CanvasError::Invalid(format!(
            "title over {MAX_TITLE_CHARS} characters"
        )));
    }
    if node
        .content
        .as_ref()
        .is_some_and(|c| c.len() > MAX_NOTE_BYTES)
    {
        return Err(CanvasError::Invalid(format!(
            "content over {MAX_NOTE_BYTES} bytes"
        )));
    }
    let reference_ok = match node.kind {
        NodeKind::Session => node.task_id.is_some() != node.imported_id.is_some(),
        NodeKind::File => {
            node.project_id.is_some() && node.path.as_ref().is_some_and(|p| !p.is_empty())
        }
        NodeKind::Note | NodeKind::Region => true,
    };
    if !reference_ok {
        return Err(CanvasError::Invalid(format!(
            "{:?} node is missing its reference",
            node.kind
        )));
    }
    Ok(())
}

/// Ids are 32 lowercase hex characters; anything else never reaches a file name.
fn is_valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn new_id() -> Result<String, CanvasError> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| CanvasError::Io(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn read_or_default<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> Result<T, CanvasError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| CanvasError::Io(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(CanvasError::Io(e.to_string())),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), CanvasError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| CanvasError::Io(e.to_string()))?;
    write_atomic(path, &bytes, FileMode::Private).map_err(|e| CanvasError::Io(e.to_string()))
}

#[cfg(test)]
mod tests;
