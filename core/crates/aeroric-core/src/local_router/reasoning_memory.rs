//! R7: upstreams that refused thinking replayed from another model. A 400 on
//! a request carrying it is retried once without it; when that succeeds the
//! target is remembered here and later requests drop it before sending.
//! In memory only (R7-Q2): a host restart, or editing the connection, tries
//! again.

use std::collections::HashSet;

use parking_lot::Mutex;

static STRIPPED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub fn remember(target_id: &str) {
    STRIPPED
        .lock()
        .get_or_insert_with(HashSet::new)
        .insert(target_id.to_string());
}

pub fn is_stripped(target_id: &str) -> bool {
    STRIPPED
        .lock()
        .as_ref()
        .is_some_and(|set| set.contains(target_id))
}

pub fn forget(target_id: &str) {
    if let Some(set) = STRIPPED.lock().as_mut() {
        set.remove(target_id);
    }
}

/// Remembered target ids, sorted.
pub fn report() -> Vec<String> {
    let mut ids: Vec<String> = STRIPPED
        .lock()
        .as_ref()
        .map(|set| set.iter().cloned().collect())
        .unwrap_or_default();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_until_forgotten() {
        assert!(!is_stripped("rm-a"));
        remember("rm-a");
        assert!(is_stripped("rm-a"));
        assert!(report().contains(&"rm-a".to_string()));
        forget("rm-a");
        assert!(!is_stripped("rm-a"));
    }
}
