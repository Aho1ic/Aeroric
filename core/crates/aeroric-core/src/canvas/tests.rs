use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "aeroric-canvas-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn store(&self) -> CanvasStore {
        CanvasStore::new(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn note(board: &str, x: f64) -> Node {
    Node {
        id: String::new(),
        board_id: board.to_string(),
        kind: NodeKind::Note,
        task_id: None,
        imported_id: None,
        project_id: None,
        path: None,
        title: None,
        content: Some("想法".into()),
        color: Some("yellow".into()),
        x,
        y: 0.0,
        width: 200.0,
        height: 120.0,
        z: 0,
        updated_at_ms: 0,
    }
}

#[test]
fn boards_are_created_renamed_listed_and_deleted() {
    let scratch = Scratch::new("boards");
    let store = scratch.store();
    let (board, r1) = store.create_board(Some(" 规划 ".into()), None, 10).unwrap();
    assert_eq!(board.name.as_deref(), Some("规划"), "names are trimmed");
    assert_eq!(board.id.len(), 32);
    let (renamed, r2) = store
        .update_board(
            &board.id,
            Some(Some("路线图".into())),
            Some(Some("blue".into())),
            20,
        )
        .unwrap();
    assert!(r2 > r1, "revision is monotonic");
    assert_eq!(renamed.name.as_deref(), Some("路线图"));
    assert_eq!(renamed.color.as_deref(), Some("blue"));
    let (cleared, _) = store.update_board(&board.id, Some(None), None, 30).unwrap();
    assert_eq!(cleared.name, None);
    assert_eq!(
        cleared.color.as_deref(),
        Some("blue"),
        "an absent field keeps its value"
    );

    let (list, _) = store.list_boards().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].node_count, 0);
    store.delete_board(&board.id).unwrap();
    assert!(store.list_boards().unwrap().0.is_empty());
    assert!(!scratch
        .0
        .join("canvas")
        .join(format!("board-{}.json", board.id))
        .exists());
    assert_eq!(
        store.delete_board(&board.id),
        Err(CanvasError::NotFound(format!("board {}", board.id)))
    );
}

#[test]
fn nodes_round_trip_and_survive_a_restart() {
    let scratch = Scratch::new("nodes");
    let store = scratch.store();
    let (board, _) = store.create_board(None, None, 1).unwrap();
    let (first, _) = store.upsert_node(note(&board.id, 0.0), 2).unwrap();
    let (second, _) = store.upsert_node(note(&board.id, 300.0), 3).unwrap();
    assert_eq!((first.z, second.z), (1, 2), "new nodes stack on top");
    assert_eq!(first.updated_at_ms, 2);

    let mut edited = first.clone();
    edited.content = Some("改过".into());
    store.upsert_node(edited, 4).unwrap();

    let restarted = CanvasStore::new(&scratch.0);
    let (nodes, _) = restarted.nodes(&board.id).unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].content.as_deref(), Some("改过"));
    assert_eq!(restarted.list_boards().unwrap().0[0].node_count, 2);
}

#[test]
fn moves_are_all_or_nothing() {
    let scratch = Scratch::new("moves");
    let store = scratch.store();
    let (board, _) = store.create_board(None, None, 1).unwrap();
    let (a, _) = store.upsert_node(note(&board.id, 0.0), 2).unwrap();
    let (b, _) = store.upsert_node(note(&board.id, 300.0), 3).unwrap();

    let bad = [
        NodeMove {
            id: a.id.clone(),
            x: 50.0,
            y: 50.0,
            width: None,
            height: None,
        },
        NodeMove {
            id: "f".repeat(32),
            x: 0.0,
            y: 0.0,
            width: None,
            height: None,
        },
    ];
    assert!(matches!(
        store.move_nodes(&board.id, &bad, 4),
        Err(CanvasError::NotFound(_))
    ));
    assert_eq!(
        store.nodes(&board.id).unwrap().0[0].x,
        0.0,
        "a failed batch writes nothing"
    );

    let too_small = [NodeMove {
        id: a.id.clone(),
        x: 0.0,
        y: 0.0,
        width: Some(1.0),
        height: None,
    }];
    assert!(matches!(
        store.move_nodes(&board.id, &too_small, 4),
        Err(CanvasError::Invalid(_))
    ));

    let good = [
        NodeMove {
            id: a.id.clone(),
            x: 40.0,
            y: 8.0,
            width: Some(240.0),
            height: None,
        },
        NodeMove {
            id: b.id.clone(),
            x: 400.0,
            y: 16.0,
            width: None,
            height: None,
        },
    ];
    store.move_nodes(&board.id, &good, 5).unwrap();
    let (nodes, _) = store.nodes(&board.id).unwrap();
    assert_eq!(
        (nodes[0].x, nodes[0].width, nodes[0].height),
        (40.0, 240.0, 120.0)
    );
    assert_eq!(nodes[1].x, 400.0);

    store
        .delete_nodes(&board.id, std::slice::from_ref(&a.id), 6)
        .unwrap();
    assert_eq!(store.nodes(&board.id).unwrap().0.len(), 1);
}

#[test]
fn validation_rejects_bad_input() {
    let scratch = Scratch::new("validate");
    let store = scratch.store();
    let (board, _) = store.create_board(None, None, 1).unwrap();

    let mut nan = note(&board.id, f64::NAN);
    assert!(matches!(
        store.upsert_node(nan.clone(), 2),
        Err(CanvasError::Invalid(_))
    ));
    nan.x = 0.0;
    nan.content = Some("x".repeat(MAX_NOTE_BYTES + 1));
    assert!(matches!(
        store.upsert_node(nan, 2),
        Err(CanvasError::Invalid(_))
    ));

    let mut session = note(&board.id, 0.0);
    session.kind = NodeKind::Session;
    assert!(
        matches!(
            store.upsert_node(session.clone(), 2),
            Err(CanvasError::Invalid(_))
        ),
        "needs a reference"
    );
    session.task_id = Some("t1".into());
    session.imported_id = Some("imp_1".into());
    assert!(
        matches!(
            store.upsert_node(session.clone(), 2),
            Err(CanvasError::Invalid(_))
        ),
        "exactly one reference"
    );
    session.imported_id = None;
    let (saved, _) = store.upsert_node(session, 2).unwrap();

    let mut retyped = saved.clone();
    retyped.kind = NodeKind::Note;
    assert!(matches!(
        store.upsert_node(retyped, 3),
        Err(CanvasError::Invalid(_))
    ));

    let mut unknown = note(&board.id, 0.0);
    unknown.id = "a".repeat(32);
    assert!(matches!(
        store.upsert_node(unknown, 3),
        Err(CanvasError::NotFound(_))
    ));
    let mut traversal = note(&board.id, 0.0);
    traversal.board_id = "../../etc".into();
    assert!(
        matches!(
            store.upsert_node(traversal, 3),
            Err(CanvasError::NotFound(_))
        ),
        "ids never reach file names"
    );
    assert!(matches!(
        store.create_board(Some("x".repeat(81)), None, 1),
        Err(CanvasError::Invalid(_))
    ));
    assert!(matches!(
        store.create_board(None, Some("not a color!".into()), 1),
        Err(CanvasError::Invalid(_))
    ));
}

#[test]
fn a_board_holds_at_most_the_node_limit() {
    let scratch = Scratch::new("limit");
    let store = scratch.store();
    let (board, _) = store.create_board(None, None, 1).unwrap();
    let mut doc = NodesDoc::default();
    for i in 0..MAX_NODES_PER_BOARD {
        let mut n = note(&board.id, i as f64);
        n.id = format!("{i:032x}");
        doc.nodes.push(n);
    }
    store.write_nodes(&board.id, &doc).unwrap();
    assert!(matches!(
        store.upsert_node(note(&board.id, 0.0), 2),
        Err(CanvasError::Limit(_))
    ));
}
