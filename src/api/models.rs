use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ArtistRef {
    #[serde(rename = "artistId")]
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub limit: u64,
    pub offset: u64,
    pub next: Option<String>,
    pub previous: Option<String>,
}

impl<T> Page<T> {
    #[allow(dead_code)]
    pub fn next_request(&self) -> Option<(String, Vec<(String, String)>)> {
        let raw = self.next.as_deref()?;
        if raw.is_empty() {
            return None;
        }
        let after_v1 = raw.split("/v1/").nth(1)?;
        let (path, query) = match after_v1.split_once('?') {
            Some((p, q)) => (p, q),
            None => (after_v1, ""),
        };
        if path.is_empty() {
            return None;
        }
        let pairs = query
            .split('&')
            .filter(|kv| !kv.is_empty())
            .map(|kv| match kv.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (kv.to_string(), String::new()),
            })
            .collect();
        Some((path.to_string(), pairs))
    }
}

fn empty_to_none(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn page_from_container<T: for<'de> Deserialize<'de>>(
    container: &Value,
) -> Result<Page<T>, String> {
    #[derive(Deserialize)]
    #[serde(default)]
    struct Raw<T> {
        items: Vec<T>,
        limit: u64,
        offset: u64,
        next: String,
        previous: String,
    }
    impl<T> Default for Raw<T> {
        fn default() -> Self {
            Self {
                items: Vec::new(),
                limit: 0,
                offset: 0,
                next: String::new(),
                previous: String::new(),
            }
        }
    }
    let raw: Raw<T> = serde_json::from_value(container.clone()).map_err(|e| e.to_string())?;
    Ok(Page {
        items: raw.items,
        limit: raw.limit,
        offset: raw.offset,
        next: empty_to_none(raw.next),
        previous: empty_to_none(raw.previous),
    })
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Song {
    #[serde(rename = "songId")]
    pub id: u64,
    #[serde(rename = "albumId")]
    pub album_id: Option<u64>,
    pub title: String,
    pub provider: Option<u8>,
    #[serde(default)]
    pub artist: Option<ArtistRef>,
    #[serde(default)]
    pub album: Option<Album>,
    pub genre: Option<String>,
    #[serde(default)]
    pub explicit: bool,
    #[serde(rename = "trackNumber")]
    pub track_number: Option<u64>,
    #[serde(rename = "sampleDurationSec")]
    pub sample_duration_sec: Option<u64>,
    #[serde(rename = "fullDurationSec")]
    pub full_duration_sec: Option<u64>,
    #[serde(default, skip_serializing)]
    pub download_status: Option<Value>,
    #[serde(default, skip_serializing)]
    pub stream_status: Option<Value>,
    #[serde(rename = "streamURL")]
    pub stream_url: Option<String>,
    #[serde(rename = "streamUrl")]
    pub stream_url_camel: Option<String>,
    #[serde(default)]
    pub sort_id: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct PlaylistCard {
    #[serde(rename = "playlistId")]
    pub id: u64,
    pub name: String,
    pub description: String,
    #[serde(rename = "songsCount")]
    pub songs_count: u64,
    #[serde(rename = "imageUrl")]
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Genre {
    #[serde(rename = "genreId")]
    pub id: Value,
    pub title: String,
    pub image: Option<String>,
}

impl Genre {
    #[allow(dead_code)]
    pub fn genre_name(&self) -> &str {
        &self.title
    }
}

impl Song {
    #[allow(dead_code)]
    pub fn stream_url(&self) -> Option<&str> {
        self.stream_url_camel
            .as_deref()
            .or(self.stream_url.as_deref())
            .filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Artist {
    #[serde(rename = "artistId")]
    pub id: String,
    pub name: String,
    pub label: Option<String>,
    #[serde(rename = "artistImage")]
    pub image: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Album {
    #[serde(rename = "albumId")]
    pub id: u64,
    pub title: String,
    pub provider: Option<u8>,
    pub artist: Option<ArtistRef>,
    pub genre: Option<String>,
    pub label: Option<String>,
    pub copyright: Option<String>,
    #[serde(rename = "copyrightDate")]
    pub year: Option<i64>,
    #[serde(rename = "imageUrl")]
    pub image_url: Option<String>,
    #[serde(default, skip_serializing)]
    pub stream_status: Option<Value>,
    #[serde(default)]
    pub songs: Vec<Song>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn playlist_card_from_probe_shape() {
        let v = json!({
            "playlistId": 397967u64, "name": "Study & Chill", "description": "",
            "songsCount": 35u64,
            "created": "2025-09-16T15:49:32+00:00",
            "imageUrl": "https://music.libraryideas.com/freegalmusic/prod/EN/assets/playlist/study.jpg",
            "streamStatus": true, "href": "https://api.freegalmusic.com/v1/playlists/songs?playlistId=397967"
        });
        let card: PlaylistCard = serde_json::from_value(v).unwrap();
        assert_eq!(card.id, 397967);
        assert_eq!(card.name, "Study & Chill");
        assert_eq!(card.songs_count, 35);
        assert!(card.image_url.is_some());
    }

    #[test]
    fn search_song_row_from_fixture() {
        let v = json!({
            "album": {
                "artist": {"artistId": "QXF1YSBUaW1leg", "name": "Aqua Timez"},
                "albumId": 9999900403297i64, "title": "Aqua Timez 20th Live -OLDROSE-",
                "provider": 1, "genre": "Rock", "copyrightDate": 2026,
                "imageUrl": "https://music.libraryideas.com/img.jpg?token=REDACTED"
            },
            "artist": {"artistId": "QXF1YSBUaW1leg", "name": "Aqua Timez"},
            "songId": 377926024, "albumId": 9999900403297i64,
            "title": "最後まで - Aqua Timez 20th Live -OLDROSE-",
            "provider": 1, "genre": "Rock",
            "sampleDurationSec": 30, "fullDurationSec": 283,
            "downloadStatus": true, "streamStatus": true, "streamURL": ""
        });
        let song: Song = serde_json::from_value(v).expect("fixture shape");
        assert_eq!(song.id, 377926024);
        assert_eq!(song.artist.as_ref().unwrap().name, "Aqua Timez");
        assert_eq!(song.album.as_ref().unwrap().year, Some(2026));
        assert_eq!(song.full_duration_sec, Some(283));
        assert_eq!(song.stream_url(), None, "empty streamURL is not a URL");
        assert_eq!(song.track_number, None, "search rows carry no trackNumber");
    }

    #[test]
    fn search_page_container_from_fixture() {
        let container = json!({
            "freegalUrl": "https://m68.freegalmusic.com/x",
            "items": [
                {"artistId": "QXF1YQ", "name": "Aqua", "label": "Do It Yourself",
                 "artistImage": "https://music.libraryideas.com/i.jpg?token=REDACTED"}
            ],
            "limit": 2, "offset": 0,
            "next": "https://api.freegalmusic.com/v1/search?type=artist&q=aqua&offset=2&limit=2&flag=1",
            "previous": ""
        });
        let page: Page<Artist> = page_from_container(&container).expect("container shape");
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name, "Aqua");
        assert_eq!(page.limit, 2);
        assert!(page.previous.is_none(), "empty-string previous → None");
        let (path, query) = page.next_request().expect("next");
        assert_eq!(path, "search");
        assert!(query.contains(&("type".to_string(), "artist".to_string())));
        assert!(query.contains(&("offset".to_string(), "2".to_string())));
        assert!(query.contains(&("flag".to_string(), "1".to_string())));
    }

    #[test]
    fn page_next_request_none_when_exhausted() {
        let page: Page<Artist> = Page {
            next: Some(String::new()),
            ..Page::default()
        };
        assert!(page.next_request().is_none());
        let page: Page<Artist> = Page::default();
        assert!(page.next_request().is_none());
    }

    #[test]
    fn album_row_from_fixture_with_year() {
        let v = json!({
            "artist": {"artistId": "QXF1YSAmIENocmlzdG9waGVyIFdhcmQgJiBEYXZpZCBUeXNvbg",
                       "name": "Aqua & Christopher Ward & David Tyson"},
            "albumId": 3850245003697i64, "title": "Aqua", "provider": 2,
            "genre": "Rock", "label": "Campus", "copyright": "(P) 1995 Campus",
            "copyrightDate": 2024, "streamStatus": true
        });
        let album: Album = serde_json::from_value(v).expect("album fixture shape");
        assert_eq!(album.title, "Aqua");
        assert_eq!(album.year, Some(2024));
        assert_eq!(album.provider, Some(2));
        assert_eq!(album.songs.len(), 0);
    }
}
