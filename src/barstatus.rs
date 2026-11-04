use std::sync::Mutex;

static LAST_KEY: Mutex<String> = Mutex::new(String::new());

pub fn write(app: &crate::app::App) {
    let (title, artist, playing) = match app.queue.current() {
        Some((_, item)) => (
            item.song.title.clone(),
            item.song.artist.as_ref().map(|a| a.name.clone()).unwrap_or_default(),
            app.player_status.paused != Some(true),
        ),
        None => (String::new(), String::new(), false),
    };
    let key = format!("{title}\u{1}{artist}\u{1}{playing}");
    {
        let mut last = LAST_KEY.lock().unwrap();
        if *last == key {
            return;
        }
        *last = key;
    }
    persist(&title, &artist, playing);
}

pub fn clear() {
    *LAST_KEY.lock().unwrap() = String::new();
    persist("", "", false);
}

pub fn body(title: &str, artist: &str, playing: bool) -> String {
    serde_json::to_string(&serde_json::json!({
        "title": title,
        "artist": artist,
        "playing": playing,
    }))
    .unwrap_or_else(|_| "{}".into())
}

fn persist(title: &str, artist: &str, playing: bool) {
    let Some(dir) = dirs::state_dir().map(|d| d.join("freegal")) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let tmp = dir.join("bar.json.tmp");
    let path = dir.join("bar.json");
    if std::fs::write(&tmp, body(title, artist, playing)).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_is_minimal_json() {
        let v: serde_json::Value =
            serde_json::from_str(&body("Housewife", "Tate McRae", true)).unwrap();
        assert_eq!(v["title"], "Housewife");
        assert_eq!(v["artist"], "Tate McRae");
        assert_eq!(v["playing"], true);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body("", "", false)).unwrap()["playing"],
            false
        );
    }
}