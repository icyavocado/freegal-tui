//! Download pipeline: percent-decoding of the signed `downloadUrl`, metadata-based
//! filename sanitation (255-byte cap), collision-free `(2)` suffixing, and streaming
//! download to disk via `.part` + atomic rename. Signed URLs are self-authorizing —
//! plain GET, no auth headers (specs/01-api.md).

use std::path::{Path, PathBuf};

use tokio_stream::StreamExt as _;

use crate::api::models::Song;

pub fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let mut decoded = None;
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &raw[i + 1..i + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                decoded = Some(byte);
                i += 3;
            }
        }
        match decoded {
            Some(b) => out.push(b),
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn sanitize_filename(name: &str) -> String {
    let stripped: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let trimmed = stripped.trim_matches(|c| c == '.' || c == ' ');
    let mut out = String::new();
    let mut total = 0usize;
    for c in trimmed.chars() {
        let len = c.len_utf8();
        if total + len > 255 {
            break;
        }
        total += len;
        out.push(c);
    }
    let safe = out.trim_end().to_string();
    if safe.is_empty() {
        "untitled".into()
    } else {
        safe
    }
}

pub fn filename_for(song: &Song) -> String {
    let base = match song.artist.as_ref() {
        Some(artist) if !artist.name.is_empty() => format!("{} - {}", artist.name, song.title),
        _ => song.title.clone(),
    };
    format!("{}.mp3", sanitize_filename(&base))
}

pub fn dedupe_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = name
        .strip_suffix(".mp3")
        .unwrap_or(name)
        .trim_end_matches(')');
    let (base, mut n) = match stem.rsplit_once(" (") {
        Some((b, tail)) if tail.chars().all(|c| c.is_ascii_digit()) => {
            (b, tail.parse::<u32>().unwrap_or(1) + 1)
        }
        _ => (stem, 2),
    };
    loop {
        let candidate = dir.join(format!("{base} ({n}).mp3"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

pub fn default_dir() -> Option<PathBuf> {
    if let Some(audio) = dirs::audio_dir() {
        return Some(audio.join("freegal"));
    }
    dirs::home_dir().map(|h| h.join("Music").join("freegal"))
}

pub async fn download_to_file(
    http: &reqwest::Client,
    url: &str,
    dest: PathBuf,
) -> std::io::Result<PathBuf> {
    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| std::io::Error::other(format!("download request: {e}")))?;
    if !resp.status().is_success() {
        return Err(std::io::Error::other(format!(
            "download failed: HTTP {}",
            resp.status()
        )));
    }
    let dir = dest
        .parent()
        .ok_or_else(|| std::io::Error::other("download path has no parent"))?;
    tokio::fs::create_dir_all(dir).await?;
    let part = dest.with_extension("mp3.part");
    let mut file = tokio::fs::File::create(&part).await?;
    use tokio::io::AsyncWriteExt as _;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream
        .next()
        .await
        .transpose()
        .map_err(|e| std::io::Error::other(format!("download body: {e}")))?
    {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&part, &dest).await?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(
            percent_decode("https%3A%2F%2Fx.example"),
            "https://x.example"
        );
        assert_eq!(percent_decode("bad%2"), "bad%2");
        assert_eq!(percent_decode("trailing%"), "trailing%");
        assert_eq!(percent_decode("%C3%A9"), "é");
    }

    #[test]
    fn sanitization() {
        assert_eq!(sanitize_filename("a/b\\c"), "a b c");
        assert_eq!(sanitize_filename("..hidden."), "hidden");
        assert_eq!(sanitize_filename("   "), "untitled");
        assert_eq!(sanitize_filename("t\0x\u{1}"), "t x");
        let long = "é".repeat(300);
        assert_eq!(sanitize_filename(&long).len(), 254);
    }

    #[test]
    fn filename_metadata() {
        let song = Song {
            title: "Bohemian/Rhapsody?".into(),
            artist: Some(crate::api::models::ArtistRef {
                id: "a".into(),
                name: "Queen".into(),
            }),
            ..Song::default()
        };
        assert_eq!(filename_for(&song), "Queen - Bohemian Rhapsody?.mp3");
    }

    #[tokio::test]
    async fn dedupe_suffixing() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("song.mp3");
        std::fs::File::create(&first).unwrap();
        assert_eq!(
            dedupe_path(dir.path(), "song.mp3"),
            dir.path().join("song (2).mp3")
        );
        std::fs::File::create(dir.path().join("song (2).mp3")).unwrap();
        assert_eq!(
            dedupe_path(dir.path(), "song.mp3"),
            dir.path().join("song (3).mp3")
        );
        assert_eq!(
            dedupe_path(dir.path(), "fresh.mp3"),
            dir.path().join("fresh.mp3")
        );
    }
}
