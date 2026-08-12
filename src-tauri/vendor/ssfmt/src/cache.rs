//! Format code caching.

use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Mutex;

use crate::ast::NumberFormat;
use crate::error::ParseError;

/// Global cache for parsed format codes.
static CACHE: Mutex<Option<LruCache<String, NumberFormat>>> = Mutex::new(None);

const CACHE_SIZE: usize = 100;

/// Get or parse a format code, using the cache.
pub fn get_or_parse(format_code: &str) -> Result<NumberFormat, ParseError> {
    let mut cache_guard = CACHE.lock().unwrap();

    let cache =
        cache_guard.get_or_insert_with(|| LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap()));

    if let Some(fmt) = cache.get(format_code) {
        return Ok(fmt.clone());
    }

    let fmt = NumberFormat::parse(format_code)?;
    cache.put(format_code.to_string(), fmt.clone());
    Ok(fmt)
}
