//! Persisted play history (`~/.config/freegal/history.json`).
//!
//! Newest first. Recorded when a stream ticket loads into mpv; a failed
//! load is un-recorded. Keep entries small — display fields only, no
//! signed/timestamped album art URLs (those expire server-side).

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const HISTORY_CAP: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Entry {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub provider: u8,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist_name: String,
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default)]
    pub album_title: String,
    #[serde(default)]
    pub album_id: Option<u64>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub duration_secs: Option<u64>,
    #[serde(default)]
    pub stream_url: String,
    #[serde(default)]
    pub played_at_unix: i64,
    #[serde(default)]
    pub plays: u32,
}

pub fn history_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("freegal")
        .join("history.json")
}

/// Newest first on read; unreadable or missing starts fresh.
pub fn load(path: &Path) -> Vec<Entry> {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(
                    "history: unparsable {} ({e}) — starting fresh",
                    path.display()
                );
                Vec::new()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            tracing::warn!(
                "history: unreadable {} ({e}) — starting fresh",
                path.display()
            );
            Vec::new()
        }
    }
}

/// Atomic tmp-file swap, 0600 file in a 0700 directory (creds.toml pattern).
pub fn save(path: &Path, entries: &[Entry]) {
    let Some(dir) = path.parent() else { return };
    if !dir.exists() && fs::create_dir_all(dir).is_err() {
        tracing::warn!("history: cannot create {}", dir.display());
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
        tracing::warn!("history: cannot write {}", tmp.display());
        return;
    };
    if serde_json::to_writer(&mut f, entries).is_ok() {
        let _ = f.flush();
        if fs::rename(&tmp, path).is_err() {
            tracing::warn!("history: cannot replace {}", path.display());
        }
    } else {
        tracing::warn!("history: serialize failed — not written");
    }
}

/// Record a play: dedupe key (id, provider); a replay refreshes the
/// timestamp and moves the entry to the top. The cap evicts the oldest —
/// a refresh must never evict.
pub fn record(entries: &mut Vec<Entry>, entry: Entry) {
    match entries
        .iter()
        .position(|e| e.id == entry.id && e.provider == entry.provider)
    {
        Some(i) => {
            let mut hit = entries.remove(i);
            hit.played_at_unix = entry.played_at_unix;
            if let Some(inc) = hit.plays.checked_add(1) {
                hit.plays = inc.max(1);
            }
            entries.insert(0, hit);
        }
        None => entries.insert(0, entry),
    }
    entries.truncate(HISTORY_CAP);
}

pub fn remove(entries: &mut Vec<Entry>, id: u64, provider: u8) -> bool {
    let before = entries.len();
    entries.retain(|e| !(e.id == id && e.provider == provider));
    before != entries.len()
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// "just now" / "5m" / "2h" / "3d" — pure for pinned-clock tests;
/// future timestamps (clock skew) clamp to "just now".
pub fn rel_time(now: i64, then: i64) -> String {
    let delta = (now - then).max(0);
    if delta < 60 {
        "just now".into()
    } else if delta < 3600 {
        format!("{}m", delta / 60)
    } else if delta < 86_400 {
        format!("{}h", delta / 3600)
    } else {
        format!("{}d", delta / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, title: &str, played_at: i64) -> Entry {
        Entry {
            id,
            provider: 1,
            title: title.into(),
            played_at_unix: played_at,
            ..Entry::default()
        }
    }

    #[test]
    fn record_refreshes_dedupe_to_top() {
        let mut h = vec![entry(1, "a", 100), entry(2, "b", 90)];
        h[0].plays = 3;
        record(&mut h, entry(1, "a", 200));
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].id, 1);
        assert_eq!(h[0].played_at_unix, 200, "timestamp refreshed");
        assert_eq!(h[0].plays, 4, "replay bumps count");
        assert_eq!(h[1].id, 2);
        let mut fresh = entry(9, "n", 300);
        fresh.plays = 1;
        record(&mut h, fresh);
        assert_eq!(h[0].plays, 1, "fresh entry keeps caller-provided count");
    }

    #[test]
    fn cap_evicts_oldest_and_refresh_never_evicts() {
        let mut h: Vec<Entry> = (0..HISTORY_CAP)
            .map(|i| entry(i as u64, "t", i as i64))
            .collect();
        h.reverse(); // newest (199) first
        assert_eq!(h.len(), HISTORY_CAP);
        // 201st distinct evicts exactly the oldest.
        record(&mut h, entry(1000, "new", 9000));
        assert_eq!(h.len(), HISTORY_CAP);
        assert_eq!(h[0].id, 1000);
        assert!(!h.iter().any(|e| e.id == 0), "oldest evicted");
        // Replay of the oldest does NOT evict anyone.
        let mut h2: Vec<Entry> = (0..HISTORY_CAP)
            .map(|i| entry(i as u64, "t", i as i64))
            .collect();
        h2.reverse();
        record(&mut h2, entry(0, "old replay", 8000));
        assert_eq!(h2.len(), HISTORY_CAP);
        assert_eq!(h2[0].id, 0);
        assert!(h2.iter().any(|e| e.id == 1));
    }

    #[test]
    fn rel_time_boundaries() {
        let now = 1_000_000;
        assert_eq!(rel_time(now, now), "just now");
        assert_eq!(rel_time(now, now - 59), "just now");
        assert_eq!(rel_time(now, now - 61), "1m");
        assert_eq!(rel_time(now, now - 3599), "59m");
        assert_eq!(rel_time(now, now - 3600), "1h");
        assert_eq!(rel_time(now, now - 86_399), "23h");
        assert_eq!(rel_time(now, now - 86_400), "1d");
        assert_eq!(rel_time(now, now + 500), "just now", "future clamps");
    }

    #[test]
    fn roundtrip_and_defaults_survive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        let h = vec![entry(7, "x", 42)];
        save(&path, &h);
        assert_eq!(load(&path), h);
    }

    #[test]
    fn corrupt_file_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        std::fs::write(&path, b"not json{").unwrap();
        assert!(load(&path).is_empty(), "no panic, fresh start");
    }
}
