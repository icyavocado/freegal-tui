use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use ratatui::widgets::TableState;
use tui_input::Input;

use crate::api::client::SearchKind;
use crate::api::models::{Album, Artist, Page, Song};

pub const SEARCH_SETTLE_MS: u64 = 300;
pub const SEARCH_LIMIT: u32 = 50;
pub const ROW_CAP: usize = 500;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchQuery {
    pub q: String,
    pub gen: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    Input,
    #[default]
    Results,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load {
    #[default]
    Idle,
    Dirty,
    Loading,
    Done,
    Failed(String),
}

impl Load {
    pub fn is_busy(&self) -> bool {
        matches!(self, Load::Dirty | Load::Loading)
    }
}

/// Result-page sort, cycled with `S` while results are focused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortMode {
    /// Relevance buckets (exact → starts-with → word-start → substring → rest),
    /// ranked on song title / artist name; ties keep API order.
    #[default]
    Best,
    TitleAZ,
    TitleZA,
    ArtistAZ,
}

impl SortMode {
    pub fn cycle(self) -> Self {
        match self {
            SortMode::Best => SortMode::TitleAZ,
            SortMode::TitleAZ => SortMode::TitleZA,
            SortMode::TitleZA => SortMode::ArtistAZ,
            SortMode::ArtistAZ => SortMode::Best,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::Best => "best match",
            SortMode::TitleAZ => "title A→Z",
            SortMode::TitleZA => "title Z→A",
            SortMode::ArtistAZ => "artist A→Z",
        }
    }
}

/// Lexical relevance bucket for `query` against a row. Lower wins.
/// Songs rank on the better of title / artist-name match so searching an
/// artist or album name does not dump their songs into the "rest" bucket.
pub(crate) fn lexical_rank(query: &str, row: &Row) -> u8 {
    if query.is_empty() {
        return u8::MAX;
    }
    match_rank(query, &row.title().to_lowercase())
        .min(match_rank(query, &row.artist_name().to_lowercase()))
}

pub(crate) fn match_rank(query: &str, haystack: &str) -> u8 {
    if haystack == query {
        0
    } else if haystack.starts_with(query) {
        1
    } else if haystack.split_whitespace().any(|w| w.starts_with(query)) {
        2
    } else if haystack.contains(query) {
        3
    } else {
        4
    }
}

fn sort_section(rows: &mut [Row], key: impl Fn(&Row) -> String, reverse: bool) {
    rows.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        if reverse {
            kb.cmp(&ka)
        } else {
            ka.cmp(&kb)
        }
    });
}

#[derive(Debug, Default, Clone)]
pub struct Boost {
    plays: std::collections::HashMap<(u64, u8), u32>,
    plays_by_album: std::collections::HashMap<u64, u32>,
    plays_by_artist: std::collections::HashMap<String, u32>,
    genres: std::collections::HashMap<String, u32>,
    genre_total: u32,
}

impl Boost {
    pub fn from_entries(entries: &[crate::history::Entry]) -> Self {
        let mut b = Boost::default();
        for e in entries {
            *b.plays.entry((e.id, e.provider)).or_insert(0) += e.plays.max(1);
            if let Some(id) = e.album_id {
                *b.plays_by_album.entry(id).or_insert(0) += 1;
            }
            if let Some(aid) = &e.artist_id {
                *b.plays_by_artist.entry(aid.clone()).or_insert(0) += 1;
            }
            if let Some(g) = &e.genre {
                *b.genres.entry(g.to_lowercase()).or_insert(0) += 1;
                b.genre_total += 1;
            }
        }
        b
    }

    fn score(&self, row: &Row) -> i64 {
        let Row::Song(s) = row else { return 0 };
        let plays = self
            .plays
            .get(&(s.id, s.provider.unwrap_or(1)))
            .copied()
            .unwrap_or(0) as i64;
        let play_pts = 2 * (3.min((plays + 1).max(1).ilog2() as i64));
        let album_pts = s
            .album_id
            .and_then(|id| self.plays_by_album.get(&id))
            .map(|_| 2)
            .unwrap_or(0) as i64;
        let artist_pts = s
            .artist
            .as_ref()
            .and_then(|a| self.plays_by_artist.get(&a.id))
            .map(|_| 1)
            .unwrap_or(0) as i64;
        let genre_pts = s
            .genre
            .as_ref()
            .and_then(|g| self.genres.get(&g.to_lowercase()))
            .map(|&n| if n * 2 >= self.genre_total { 1 } else { 0 })
            .unwrap_or(0) as i64;
        play_pts + album_pts + artist_pts + genre_pts
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Row {
    Song(Song),
    Artist(Artist),
    Album(Album),
}

impl Row {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Row::Song(_) => "song",
            Row::Artist(_) => "artist",
            Row::Album(_) => "album",
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Row::Song(s) => &s.title,
            Row::Artist(a) => &a.name,
            Row::Album(a) => &a.title,
        }
    }

    pub fn artist_name(&self) -> &str {
        match self {
            Row::Song(s) => s.artist.as_ref().map(|a| a.name.as_str()).unwrap_or(""),
            Row::Artist(a) => &a.name,
            Row::Album(a) => a.artist.as_ref().map(|x| x.name.as_str()).unwrap_or(""),
        }
    }

    pub fn album_label(&self) -> String {
        match self {
            Row::Song(s) => s
                .album
                .as_ref()
                .map(|a| a.title.as_str())
                .unwrap_or("")
                .to_string(),
            Row::Artist(_) => String::new(),
            Row::Album(a) => a.year.map(|y| y.to_string()).unwrap_or_default(),
        }
    }

    pub fn duration_secs(&self) -> Option<u64> {
        match self {
            Row::Song(s) => s.full_duration_sec,
            _ => None,
        }
    }

    pub fn artist_id(&self) -> Option<&str> {
        match self {
            Row::Artist(a) => Some(a.id.as_str()),
            _ => None,
        }
    }

    pub fn album(&self) -> Option<&Album> {
        match self {
            Row::Album(a) => Some(a),
            _ => None,
        }
    }

    pub fn is_song(&self) -> bool {
        matches!(self, Row::Song(_))
    }
}

#[derive(Debug, Clone)]
pub struct AlbumRequest {
    pub album: Album,
    pub gen: u64,
    pub slot: crate::app::AlbumDrillSlot,
}

#[derive(Debug, Default)]
pub struct SearchHome {
    pub input: Input,
    pub gen: u64,
    pub focus: Focus,
    pub sort: SortMode,
    pub boost: Boost,
    pub kind_state: [Load; 3],
    pub songs: Page<Song>,
    pub artists: Page<Artist>,
    pub albums: Page<Album>,
    pub table: TableState,
    pub pending_album: Option<AlbumRequest>,
}

impl SearchHome {
    pub fn on_input_changed(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        self.sort = SortMode::Best;
        self.kind_state = std::array::from_fn(|_| Load::Dirty);
        if self.input.value().trim().is_empty() {
            self.clear_results();
        }
    }

    pub fn clear_results(&mut self) {
        self.songs = Page::default();
        self.artists = Page::default();
        self.albums = Page::default();
        self.kind_state = std::array::from_fn(|_| Load::Idle);
        self.table.select(None);
    }

    pub fn mark_dispatched(&mut self, gen: u64) {
        if self.gen == gen {
            self.kind_state = std::array::from_fn(|_| Load::Loading);
        }
    }

    pub fn apply_kind(&mut self, gen: u64, kind: SearchKind, result: Load, page: AppliedPage) {
        if self.gen != gen {
            return;
        }
        self.kind_state[kind as usize] = match result {
            Load::Done => Load::Done,
            Load::Failed(msg) => Load::Failed(msg),
            _ => unreachable!("apply_kind receives terminal states only"),
        };
        match (kind, page) {
            (SearchKind::Song, AppliedPage::Songs(p)) => {
                self.songs = p;
            }
            (SearchKind::Artist, AppliedPage::Artists(p)) => {
                self.artists = p;
            }
            (SearchKind::Album, AppliedPage::Albums(p)) => {
                self.albums = p;
            }
            _ => {}
        }
        if self.table.selected().is_none() && !self.rows().is_empty() {
            self.table.select(Some(0));
        }
    }

    pub fn rows(&self) -> Vec<Row> {
        let query = self.input.value().trim().to_lowercase();
        let mut songs: Vec<Row> = self
            .songs
            .items
            .iter()
            .map(|s| Row::Song(s.clone()))
            .collect();
        let mut artists: Vec<Row> = self
            .artists
            .items
            .iter()
            .map(|a| Row::Artist(a.clone()))
            .collect();
        let mut albums: Vec<Row> = self
            .albums
            .items
            .iter()
            .map(|a| Row::Album(a.clone()))
            .collect();
        match self.sort {
            SortMode::Best => {
                for sec in [&mut songs, &mut artists, &mut albums] {
                    let mut indexed: Vec<(usize, Row)> = sec.drain(..).enumerate().collect();
                    indexed.sort_by(|(ia, ra), (ib, rb)| {
                        let (ba, bb) = (lexical_rank(&query, ra), lexical_rank(&query, rb));
                        ba.cmp(&bb)
                            .then_with(|| self.boost.score(rb).cmp(&self.boost.score(ra)))
                            .then_with(|| ia.cmp(ib))
                    });
                    sec.extend(indexed.into_iter().map(|(_, r)| r));
                }
            }
            SortMode::TitleAZ => {
                sort_section(&mut songs, |r| r.title().to_lowercase(), false);
                sort_section(&mut artists, |r| r.title().to_lowercase(), false);
                sort_section(&mut albums, |r| r.title().to_lowercase(), false);
            }
            SortMode::TitleZA => {
                sort_section(&mut songs, |r| r.title().to_lowercase(), true);
                sort_section(&mut artists, |r| r.title().to_lowercase(), true);
                sort_section(&mut albums, |r| r.title().to_lowercase(), true);
            }
            SortMode::ArtistAZ => {
                sort_section(&mut songs, |r| r.artist_name().to_lowercase(), false);
                sort_section(&mut artists, |r| r.artist_name().to_lowercase(), false);
                sort_section(&mut albums, |r| r.artist_name().to_lowercase(), false);
            }
        }
        let mut rows: Vec<Row> = Vec::with_capacity(
            self.songs.items.len() + self.artists.items.len() + self.albums.items.len(),
        );
        rows.extend(songs);
        rows.extend(artists);
        rows.extend(albums);
        rows.truncate(ROW_CAP);
        rows
    }

    pub fn any_busy(&self) -> bool {
        self.kind_state.iter().any(Load::is_busy)
    }

    pub fn first_error(&self) -> Option<&str> {
        for s in &self.kind_state {
            if let Load::Failed(msg) = s {
                return Some(msg);
            }
        }
        None
    }

    pub fn has_rows(&self) -> bool {
        !self.songs.items.is_empty()
            || !self.artists.items.is_empty()
            || !self.albums.items.is_empty()
    }

    pub fn more_available(&self) -> bool {
        let has_next = |n: &Option<String>| n.as_ref().is_some_and(|n| !n.is_empty());
        has_next(&self.songs.next) || has_next(&self.artists.next) || has_next(&self.albums.next)
    }
}

#[derive(Debug)]
pub enum AppliedPage {
    None,
    Songs(Page<Song>),
    Artists(Page<Artist>),
    Albums(Page<Album>),
}

#[derive(Debug)]
pub struct ArtistContext {
    pub artist: Artist,
    pub albums: Page<Album>,
    pub state: Load,
    pub gen: u64,
    pub table: TableState,
    pub pending_album: Option<AlbumRequest>,
}

#[derive(Debug, Clone)]
pub struct AlbumContext {
    pub album: Album,
    pub table: TableState,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum SearchView {
    Root(SearchHome),
    Artist(Box<ArtistContext>),
    Album(Box<AlbumContext>),
}

#[derive(Debug, Default)]
pub struct SearchTab {
    pub stack: Vec<SearchView>,
}

impl SearchTab {
    pub fn new() -> Self {
        Self {
            stack: vec![SearchView::Root(SearchHome::default())],
        }
    }

    pub fn top(&self) -> Option<&SearchView> {
        self.stack.last()
    }

    pub fn top_mut(&mut self) -> Option<&mut SearchView> {
        self.stack.last_mut()
    }

    pub fn pop(&mut self) -> bool {
        if self.stack.len() > 1 {
            self.stack.pop();
            true
        } else {
            false
        }
    }
}

pub fn notify_search_input(
    watch: &Option<tokio::sync::watch::Sender<SearchQuery>>,
    home: &SearchHome,
) {
    if let Some(tx) = watch {
        let _ = tx.send(SearchQuery {
            q: home.input.value().trim().to_string(),
            gen: home.gen,
        });
    }
}

pub type SentGen = Arc<AtomicU64>;

pub fn sent_gen_cell() -> SentGen {
    Arc::new(AtomicU64::new(0))
}

pub fn claim_dispatch(cell: &SentGen, gen: u64) -> bool {
    cell.fetch_max(gen, Ordering::SeqCst) < gen
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song_page() -> Page<Song> {
        serde_json::from_value(json!({
            "items": [{"songId": 1, "title": "Song A", "fullDurationSec": 201,
                        "artist": {"artistId": "x", "name": "Artist X"}}],
            "offset": 0, "limit": 50, "next": ""
        }))
        .unwrap()
    }

    fn artist_page() -> Page<Artist> {
        serde_json::from_value(json!({
            "items": [{"artistId": "UXJ0", "name": "Artist X"}],
            "offset": 0, "limit": 50
        }))
        .unwrap()
    }

    #[test]
    fn typing_marks_dirty_and_bumps_gen() {
        let mut h = SearchHome::default();
        h.input.handle(tui_input::InputRequest::InsertChar('a'));
        let g0 = h.gen;
        h.on_input_changed();
        assert_eq!(h.gen, g0 + 1);
        assert!(h.any_busy());
    }

    #[test]
    fn stale_results_are_dropped() {
        let mut h = SearchHome::default();
        h.on_input_changed();
        let stale = h.gen - 1;
        h.apply_kind(
            stale,
            SearchKind::Song,
            Load::Done,
            AppliedPage::Songs(song_page()),
        );
        assert!(h.songs.items.is_empty(), "stale gen must not land");
        h.apply_kind(
            h.gen,
            SearchKind::Song,
            Load::Done,
            AppliedPage::Songs(song_page()),
        );
        assert_eq!(h.songs.items.len(), 1);
    }

    fn boost_entry(id: u64, plays: u32, genre: &str, artist_id: &str) -> crate::history::Entry {
        crate::history::Entry {
            id,
            provider: 1,
            plays,
            genre: Some(genre.into()),
            artist_id: Some(artist_id.into()),
            ..crate::history::Entry::default()
        }
    }

    fn love_page() -> Page<Song> {
        serde_json::from_value(json!({
            "items": [
                {"songId": 1, "title": "Never Love Again", "genre": "Rock",
                 "artist": {"artistId": "a", "name": "Band A"}},
                {"songId": 2, "title": "A Love Song", "genre": "Pop",
                 "artist": {"artistId": "b", "name": "Band B"}}
            ],
            "offset": 0, "limit": 50, "next": ""
        }))
        .unwrap()
    }

    fn home_typing(query: &str, songs: Page<Song>) -> SearchHome {
        let mut h = SearchHome {
            kind_state: std::array::from_fn(|_| Load::Done),
            songs,
            ..SearchHome::default()
        };
        for c in query.chars() {
            h.input.handle(tui_input::InputRequest::InsertChar(c));
        }
        h
    }

    fn first_song_title(h: &SearchHome) -> String {
        match h.rows().first() {
            Some(Row::Song(s)) => s.title.clone(),
            other => panic!("expected song row, got {other:?}"),
        }
    }

    #[test]
    fn boost_promotes_played_song_within_same_bucket() {
        let h = home_typing("love", love_page());
        assert_eq!(first_song_title(&h), "Never Love Again");
        let mut h = h;
        h.boost = Boost::from_entries(&[boost_entry(2, 5, "Pop", "b")]);
        assert_eq!(first_song_title(&h), "A Love Song");
    }

    #[test]
    fn boost_never_crosses_lexical_buckets() {
        let songs = serde_json::from_value(json!({
            "items": [
                {"songId": 1, "title": "Love",
                 "artist": {"artistId": "a", "name": "Band A"}},
                {"songId": 2, "title": "A Love Song",
                 "artist": {"artistId": "b", "name": "Band B"}}
            ],
            "offset": 0, "limit": 50, "next": ""
        }))
        .unwrap();
        let mut h = home_typing("love", songs);
        h.boost = Boost::from_entries(&[boost_entry(2, 999, "Pop", "b")]);
        assert_eq!(
            first_song_title(&h),
            "Love",
            "exact match wins despite zero plays"
        );
    }

    #[test]
    fn genre_affinity_breaks_ties_without_direct_plays() {
        let h = home_typing("love", love_page());
        assert_eq!(first_song_title(&h), "Never Love Again");
        let mut h = h;
        h.boost = Boost::from_entries(&[
            boost_entry(99, 1, "Pop", "zz"),
            boost_entry(98, 1, "Pop", "zy"),
        ]);
        assert_eq!(first_song_title(&h), "A Love Song");
    }

    #[test]
    fn rows_ordered_and_capped() {
        let h = SearchHome {
            kind_state: std::array::from_fn(|_| Load::Done),
            songs: song_page(),
            artists: artist_page(),
            ..SearchHome::default()
        };
        let rows = h.rows();
        assert!(matches!(rows[0], Row::Song(_)));
        assert!(matches!(rows[1], Row::Artist(_)));
        assert_eq!(rows[0].artist_name(), "Artist X");
    }

    #[test]
    fn best_promotes_exact_title_match() {
        let songs = serde_json::from_value(json!({
            "items": [
                {"songId": 1, "title": "I Want It All",
                 "artist": {"artistId": "a", "name": "Queen Trib"}},
                {"songId": 2, "title": "Queen",
                 "artist": {"artistId": "b", "name": "Someone Else"}}
            ],
            "offset": 0, "limit": 50, "next": ""
        }))
        .unwrap();
        let mut h = SearchHome {
            kind_state: std::array::from_fn(|_| Load::Done),
            songs,
            ..SearchHome::default()
        };
        for c in "queen".chars() {
            h.input.handle(tui_input::InputRequest::InsertChar(c));
        }
        let rows = h.rows();
        assert!(
            matches!(&rows[0], Row::Song(s) if s.title == "Queen"),
            "exact title beat starts-with artist name"
        );
        assert!(matches!(&rows[1], Row::Song(s) if s.title == "I Want It All"));
    }

    #[test]
    fn s_cycles_reorders_rows() {
        let songs = serde_json::from_value(json!({
            "items": [
                {"songId": 1, "title": "Bbb", "artist": {"artistId": "a", "name": "Yard"}},
                {"songId": 2, "title": "Aaa", "artist": {"artistId": "b", "name": "Xero"}}
            ],
            "offset": 0, "limit": 50, "next": ""
        }))
        .unwrap();
        let mut h = SearchHome {
            kind_state: std::array::from_fn(|_| Load::Done),
            songs,
            ..SearchHome::default()
        };
        assert_eq!(h.sort, SortMode::Best);
        assert_eq!(
            h.rows()[0].title(),
            "Bbb",
            "Best with no query keeps API order"
        );
        h.sort = h.sort.cycle(); // title A→Z
        assert_eq!(h.rows()[0].title(), "Aaa");
        h.sort = h.sort.cycle(); // title Z→A
        assert_eq!(h.rows()[0].title(), "Bbb");
        h.sort = h.sort.cycle(); // artist A→Z
        assert_eq!(h.rows()[0].artist_name(), "Xero");
        h.sort = h.sort.cycle(); // wraps to Best
        assert_eq!(h.rows()[0].title(), "Bbb");
    }

    #[test]
    fn new_query_resets_sort_to_best() {
        let mut h = SearchHome::default();
        h.sort = SortMode::ArtistAZ;
        h.input.handle(tui_input::InputRequest::InsertChar('x'));
        h.on_input_changed();
        assert_eq!(h.sort, SortMode::Best);
    }

    #[test]
    fn selection_auto_set_on_first_result() {
        let mut h = SearchHome::default();
        h.apply_kind(
            h.gen,
            SearchKind::Song,
            Load::Done,
            AppliedPage::Songs(song_page()),
        );
        assert_eq!(h.table.selected(), Some(0));
    }

    #[test]
    fn esc_pop_never_crosses_root() {
        let mut t = SearchTab::new();
        assert!(!t.pop(), "root pop must be rejected");
        t.stack.push(SearchView::Album(Box::new(AlbumContext {
            album: Album::default(),
            table: TableState::default(),
        })));
        assert!(t.pop());
        assert!(matches!(t.top(), Some(SearchView::Root(_))));
    }

    #[test]
    fn dispatch_claim_is_one_shot_per_gen() {
        let cell = sent_gen_cell();
        assert!(claim_dispatch(&cell, 5));
        assert!(!claim_dispatch(&cell, 5));
        assert!(claim_dispatch(&cell, 6));
    }
}
