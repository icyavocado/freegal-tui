use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CACHE_TTL: u64 = 1800;
pub const CACHE_CAP: usize = 64;

#[derive(Serialize, Deserialize)]
struct Entry {
    data: Value,
    inserted: u64,
}

#[derive(Serialize, Deserialize)]
pub struct RequestCache {
    ttl: u64,
    cap: usize,
    entries: HashMap<String, Entry>,
    order: VecDeque<String>,
}

impl Default for RequestCache {
    fn default() -> Self {
        Self::new(CACHE_TTL, CACHE_CAP)
    }
}

impl RequestCache {
    pub fn new(ttl: u64, cap: usize) -> Self {
        Self {
            ttl,
            cap,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn cache_path() -> Option<PathBuf> {
        dirs::state_dir().map(|d| d.join("freegal").join("cache.json"))
    }

    pub fn get(&mut self, path: &str, pairs: &[(&str, &str)], now: u64) -> Option<Value> {
        let key = cache_key(path, pairs);
        let entry = self.entries.get(&key)?;
        if now.saturating_sub(entry.inserted) >= self.ttl {
            self.entries.remove(&key);
            return None;
        }
        Some(entry.data.clone())
    }

    pub fn put(&mut self, path: &str, pairs: &[(&str, &str)], data: Value, now: u64) {
        if data.is_null() {
            return;
        }
        let key = cache_key(path, pairs);
        if !self.entries.contains_key(&key) {
            self.order.push_back(key.clone());
        }
        self.entries.insert(
            key,
            Entry {
                data,
                inserted: now,
            },
        );
        while self.entries.len() > self.cap {
            let Some(oldest) = self.order.pop_front() else {
                self.entries.clear();
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    pub fn save(&self, path: &Path) {
        let Some(dir) = path.parent() else { return };
        if !dir.exists() && fs::create_dir_all(dir).is_err() {
            tracing::warn!("cache: cannot create {}", dir.display());
            return;
        }
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        let tmp = path.with_extension("json.tmp");
        let Ok(mut f) = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
        else {
            tracing::warn!("cache: cannot write {}", tmp.display());
            return;
        };
        if serde_json::to_writer(&mut f, self).is_ok() {
            let _ = f.flush();
            if fs::rename(&tmp, path).is_err() {
                tracing::warn!("cache: cannot replace {}", path.display());
            }
        } else {
            tracing::warn!("cache: serialize failed — not written");
        }
    }

    pub fn load(path: &Path, now: u64) -> Self {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => return Self::default(),
        };
        let mut cache: Self = match serde_json::from_slice(&bytes) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("cache: corrupt {} ({e}) — starting fresh", path.display());
                return Self::default();
            }
        };
        cache
            .entries
            .retain(|_, e| now.saturating_sub(e.inserted) < cache.ttl);
        cache.order.retain(|k| cache.entries.contains_key(k));
        while cache.entries.len() > cache.cap {
            let Some(oldest) = cache.order.pop_front() else {
                cache.entries.clear();
                break;
            };
            cache.entries.remove(&oldest);
        }
        cache
    }
}

pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn cache_key(path: &str, pairs: &[(&str, &str)]) -> String {
    let mut parts = vec![escape(path)];
    let mut sorted: Vec<(&str, &str)> = pairs.to_vec();
    sorted.sort();
    for (k, v) in sorted {
        parts.push(format!("{}={}", escape(k), escape(v)));
    }
    parts.join("&")
}

fn escape(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace(' ', "%20")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn key_is_order_independent_and_collision_safe() {
        let a = cache_key(
            "search",
            &[("q", "abba"), ("type", "song"), ("limit", "50")],
        );
        let b = cache_key(
            "search",
            &[("limit", "50"), ("type", "song"), ("q", "abba")],
        );
        assert_eq!(a, b);
        let space = cache_key("search", &[("q", "a b")]);
        let amp = cache_key("search", &[("q", "a&b")]);
        let eq = cache_key("search", &[("q", "a=b")]);
        assert_ne!(space, amp);
        assert_ne!(space, eq);
        assert_ne!(amp, eq);
        let genre = cache_key("genre/song", &[("genreName", "R&B")]);
        let genre_other = cache_key("genre/song", &[("genreName", "R"), ("x", "B")]);
        assert_ne!(genre, genre_other);
    }

    #[test]
    fn ttl_expiry_needs_fresh_clock() {
        let mut cache = RequestCache::new(100, 8);
        cache.put(
            "search",
            &[("q", "pop")],
            serde_json::json!({"items": []}),
            0,
        );
        assert!(cache.get("search", &[("q", "pop")], 1).is_some());
        assert!(cache.get("search", &[("q", "pop")], 101).is_none());
    }

    #[test]
    fn evicts_oldest_beyond_cap() {
        let mut cache = RequestCache::new(100, 2);
        cache.put("search", &[("q", "a")], serde_json::json!(1), 0);
        cache.put("search", &[("q", "b")], serde_json::json!(2), 0);
        cache.put("search", &[("q", "c")], serde_json::json!(3), 0);
        assert!(cache.get("search", &[("q", "a")], 0).is_none());
        assert!(cache.get("search", &[("q", "c")], 0).is_some());
    }

    #[test]
    fn null_is_never_stored_and_reput_refreshes() {
        let mut cache = RequestCache::new(100, 8);
        cache.put("search", &[("q", "a")], Value::Null, 0);
        assert!(cache.get("search", &[("q", "a")], 0).is_none());
        cache.put(
            "search",
            &[("q", "a")],
            serde_json::json!({"stale": true}),
            0,
        );
        cache.put(
            "search",
            &[("q", "a")],
            serde_json::json!({"stale": false}),
            0,
        );
        let fresh = cache.get("search", &[("q", "a")], 0).unwrap();
        assert_eq!(fresh, serde_json::json!({"stale": false}));
        cache.put("f", &[], serde_json::json!({"x": 1}), 0);
        assert!(cache.get("f", &[], 0).is_some());
    }

    #[test]
    fn roundtrip_load_drops_expired_and_respects_cap() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("cache.json");

        let mut warm = RequestCache::new(100, 2);
        warm.put("feeds", &[("k", "old")], serde_json::json!(1), 0);
        warm.put("feeds", &[("k", "fresh")], serde_json::json!(2), 900);
        warm.save(&path);

        let mut loaded = RequestCache::load(&path, 999);
        assert!(loaded.get("feeds", &[("k", "fresh")], 999).is_some());
        assert!(loaded.get("feeds", &[("k", "old")], 999).is_none());
    }

    #[test]
    fn load_missing_or_corrupt_starts_fresh_and_empty_file_roundtrips() {
        let dir = tempdir().expect("tempdir");
        let missing = dir.path().join("missing.json");
        assert_eq!(RequestCache::load(&missing, 10).entries.len(), 0);

        let corrupt = dir.path().join("corrupt.json");
        std::fs::write(&corrupt, b"not json at all").expect("write");
        assert_eq!(RequestCache::load(&corrupt, 10).entries.len(), 0);

        let empty = dir.path().join("empty.json");
        RequestCache::new(60, 4).save(&empty);
        assert!(empty.exists());
        assert_eq!(RequestCache::load(&empty, 10).entries.len(), 0);

        let mut loaded = RequestCache::load(&empty, 10);
        loaded.put("search", &[("q", "x")], serde_json::json!({"i": [1]}), 10);
        assert!(loaded.get("search", &[("q", "x")], 10).is_some());
    }

    #[test]
    fn clear_wipes_entries_and_order() {
        let mut cache = RequestCache::new(100, 8);
        cache.put("search", &[("q", "a")], serde_json::json!(1), 0);
        cache.clear();
        assert!(cache.get("search", &[("q", "a")], 0).is_none());
    }
}
