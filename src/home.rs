use crate::api::models::{Album, Artist, Genre, Page, PlaylistCard, Song};
use crate::search::{AlbumContext, AlbumRequest, Load};
use ratatui::widgets::TableState;

pub const FEED_LIMIT: u64 = 8;
pub const DRILL_LIMIT: u64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeFeed {
    FeaturedSongs,
    MoreForYou,
    LibraryPlaylists,
    RecentlyPlayed,
    RecommendedPlaylists,
    FeaturedPlaylists,
    FeaturedArtists,
    NewAlbums,
    TrendingPlaylists,
    TopSongs,
    FeaturedAlbums,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum HomeFeedData {
    #[default]
    None,
    Songs(Page<Song>),
    Albums(Page<Album>),
    Playlists(Page<PlaylistCard>),
    Artists(Page<Artist>),
}

pub struct Section {
    pub feed: HomeFeed,
    pub title: &'static str,
    pub state: Load,
    pub data: HomeFeedData,
}

pub fn section_defs() -> [(&'static str, HomeFeed); 9] {
    [
        ("Featured Songs", HomeFeed::FeaturedSongs),
        ("More of what you'd like", HomeFeed::MoreForYou),
        ("Library Playlists", HomeFeed::LibraryPlaylists),
        ("Recently Played", HomeFeed::RecentlyPlayed),
        ("Recommended Playlists", HomeFeed::RecommendedPlaylists),
        ("Featured Playlists", HomeFeed::FeaturedPlaylists),
        ("Artists You Might Like", HomeFeed::FeaturedArtists),
        ("New Albums", HomeFeed::NewAlbums),
        ("Trending Playlists", HomeFeed::TrendingPlaylists),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickLink {
    Feed(HomeFeed),
    Genres,
}

pub fn quick_links() -> [(&'static str, QuickLink); 6] {
    [
        ("Artists", QuickLink::Feed(HomeFeed::FeaturedArtists)),
        ("Trending", QuickLink::Feed(HomeFeed::TopSongs)),
        ("New", QuickLink::Feed(HomeFeed::NewAlbums)),
        ("Albums", QuickLink::Feed(HomeFeed::FeaturedAlbums)),
        ("Playlists", QuickLink::Feed(HomeFeed::FeaturedPlaylists)),
        ("Genres", QuickLink::Genres),
    ]
}

pub const GENRE_SONGS: u64 = 15;
pub const GENRE_ALBUMS: u64 = 8;
pub const GENRE_PLAYLISTS: u64 = 8;
pub const GENRE_ARTISTS: u64 = 12;

#[derive(Debug, Clone)]
pub struct GenreDetail {
    pub genre: String,
    pub sub: Option<String>,
    pub songs: Page<Song>,
    pub albums: Page<Album>,
    pub playlists: Page<PlaylistCard>,
    pub artists: Page<Artist>,
    pub subgenres: Page<Genre>,
    pub state: Load,
    pub table: TableState,
}

const SECTION_ITEMS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeFocus {
    #[default]
    Sections,
    Links,
    Curated,
}

pub const SIDEBAR_MIN_WIDTH: u16 = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum DrillKind {
    Playlist(u64, String),
    Feed(HomeFeed),
    Artist(String),
    Genres,
    Genre(String, Option<String>),
}

pub struct PendingDrill {
    pub kind: DrillKind,
    pub gen: u64,
}

#[derive(Debug, Clone, Default)]
pub enum HomeView {
    #[default]
    Root,
    Loading {
        state: Load,
    },
    Genres {
        page: Page<Genre>,
        state: Load,
        table: TableState,
    },
    Detail(Box<GenreDetail>),
    Album(Box<AlbumContext>),
    List {
        songs: Page<Song>,
        state: Load,
        table: TableState,
    },
    Albums {
        albums: Page<Album>,
        state: Load,
        table: TableState,
    },
    Playlists {
        playlists: Page<PlaylistCard>,
        state: Load,
        table: TableState,
    },
    Artists {
        artists: Page<Artist>,
        state: Load,
        table: TableState,
    },
}

pub struct HomeTab {
    pub sections: Vec<Section>,
    pub gen: u64,
    pub genre_cache: Option<Page<Genre>>,
    pub view: HomeView,
    pub back: Vec<HomeView>,
    pub crumbs: Vec<String>,
    pub focus: HomeFocus,
    pub last_width: u16,
    pub link_sel: usize,
    pub sections_table: TableState,
    pub curated_table: TableState,
    pub pending: Option<PendingDrill>,
    pub pending_album: Option<AlbumRequest>,
}

impl Default for HomeTab {
    fn default() -> Self {
        Self {
            sections: section_defs()
                .into_iter()
                .map(|(title, feed)| Section {
                    feed,
                    title,
                    state: Load::Idle,
                    data: HomeFeedData::default(),
                })
                .collect(),
            gen: 0,
            genre_cache: None,
            view: HomeView::Root,
            back: Vec::new(),
            crumbs: vec!["Home".to_string()],
            focus: HomeFocus::default(),
            last_width: 0,
            link_sel: 0,
            sections_table: TableState::default(),
            curated_table: TableState::default(),
            pending: None,
            pending_album: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NavDir {
    Up,
    Down,
}

#[derive(Clone)]
pub enum HomeRow<'a> {
    Head(HomeFeed, &'static str),
    Song(&'a Song),
    Album(&'a Album),
    Playlist(&'a PlaylistCard),
    Artist(&'a Artist),
}

impl HomeTab {
    pub fn is_root(&self) -> bool {
        matches!(self.view, HomeView::Root)
    }

    pub fn section_mut(&mut self, feed: HomeFeed) -> Option<&mut Section> {
        self.sections.iter_mut().find(|s| s.feed == feed)
    }

    pub fn curated(&self) -> &Section {
        self.sections
            .iter()
            .find(|s| s.feed == HomeFeed::LibraryPlaylists)
            .expect("library playlists section always present")
    }

    pub fn curated_len(&self) -> usize {
        match &self.curated().data {
            HomeFeedData::Playlists(p) => p.items.len(),
            _ => 0,
        }
    }

    pub fn curated_ok(&self) -> bool {
        self.curated().state == Load::Done
    }

    pub fn busy(&self) -> bool {
        self.sections.iter().any(|s| s.state.is_busy())
    }

    pub fn mark_loading(&mut self) {
        for s in &mut self.sections {
            s.state = Load::Loading;
        }
    }

    pub fn home_rows(&self) -> Vec<HomeRow<'_>> {
        let mut rows = Vec::new();
        for s in &self.sections {
            if s.state != Load::Done {
                continue;
            }
            match &s.data {
                HomeFeedData::Songs(page) if !page.items.is_empty() => {
                    rows.push(HomeRow::Head(s.feed, s.title));
                    rows.extend(page.items.iter().take(SECTION_ITEMS).map(HomeRow::Song));
                }
                HomeFeedData::Albums(page) if !page.items.is_empty() => {
                    rows.push(HomeRow::Head(s.feed, s.title));
                    rows.extend(page.items.iter().take(SECTION_ITEMS).map(HomeRow::Album));
                }
                HomeFeedData::Playlists(page) if !page.items.is_empty() => {
                    rows.push(HomeRow::Head(s.feed, s.title));
                    rows.extend(page.items.iter().take(SECTION_ITEMS).map(HomeRow::Playlist));
                }
                HomeFeedData::Artists(page) if !page.items.is_empty() => {
                    rows.push(HomeRow::Head(s.feed, s.title));
                    rows.extend(page.items.iter().take(SECTION_ITEMS).map(HomeRow::Artist));
                }
                _ => {}
            }
        }
        rows
    }

    pub fn nav_sections(&mut self, dir: NavDir) {
        let len = self.home_rows().len();
        nav_list(&mut self.sections_table, len, dir);
    }

    pub fn nav_links(&mut self, dir: i32) {
        let len = quick_links().len();
        if len == 0 {
            return;
        }
        self.link_sel = (self.link_sel as i32 + dir).rem_euclid(len as i32) as usize;
    }

    pub fn selected_link(&self) -> Option<QuickLink> {
        quick_links().get(self.link_sel).map(|(_, link)| *link)
    }

    pub fn nav_curated(&mut self, dir: NavDir) {
        let len = self.curated_len();
        nav_list(&mut self.curated_table, len, dir);
    }

    pub fn selected_row(&self) -> Option<HomeRow<'_>> {
        let idx = self.sections_table.selected()?;
        self.home_rows().get(idx).cloned()
    }

    pub fn selected_playlist(&self) -> Option<&PlaylistCard> {
        let idx = self.curated_table.selected()?;
        match &self.curated().data {
            HomeFeedData::Playlists(p) => p.items.get(idx),
            _ => None,
        }
    }

    pub fn enter_level(&mut self, label: String) {
        if !self.is_root() {
            self.back.push(self.view.clone());
        }
        self.crumbs.push(label);
    }

    pub fn pop_level(&mut self) {
        self.pending = None;
        self.pending_album = None;
        if self.crumbs.len() > 1 {
            self.crumbs.pop();
            self.view = self.back.pop().unwrap_or(HomeView::Root);
        } else {
            self.view = HomeView::Root;
        }
    }

    pub fn pop_to_crumb(&mut self, k: usize) {
        while self.crumbs.len() > k + 1 {
            self.pending = None;
            self.pending_album = None;
            self.crumbs.pop();
            self.view = self.back.pop().unwrap_or(HomeView::Root);
        }
    }
}

pub fn feed_label(feed: HomeFeed) -> &'static str {
    match feed {
        HomeFeed::FeaturedSongs => "Featured Songs",
        HomeFeed::MoreForYou => "More of what you'd like",
        HomeFeed::LibraryPlaylists => "Library Playlists",
        HomeFeed::RecentlyPlayed => "Recently Played",
        HomeFeed::RecommendedPlaylists => "Recommended Playlists",
        HomeFeed::FeaturedPlaylists => "Featured Playlists",
        HomeFeed::FeaturedArtists => "Featured Artists",
        HomeFeed::NewAlbums => "New Albums",
        HomeFeed::TrendingPlaylists => "Trending Playlists",
        HomeFeed::TopSongs => "Trending Songs",
        HomeFeed::FeaturedAlbums => "Featured Albums",
    }
}

pub fn detail_rows_len(d: &GenreDetail) -> usize {
    let mut n = 0usize;
    if !d.subgenres.items.is_empty() && d.sub.is_none() {
        n += 1 + d.subgenres.items.len().min(6);
    }
    if !d.songs.items.is_empty() {
        n += 1 + d.songs.items.len();
    }
    if !d.playlists.items.is_empty() {
        n += 1 + d.playlists.items.len();
    }
    if !d.albums.items.is_empty() {
        n += 1 + d.albums.items.len();
    }
    if !d.artists.items.is_empty() {
        n += 1 + d.artists.items.len();
    }
    n
}

pub enum DetailRow<'a> {
    Head(&'static str),
    Song(&'a Song),
    Album(&'a Album),
    Playlist(&'a PlaylistCard),
    Artist(&'a Artist),
    Sub(&'a Genre),
}

impl HomeTab {
    pub fn detail_rows(&self) -> Vec<DetailRow<'_>> {
        let HomeView::Detail(d) = &self.view else {
            return Vec::new();
        };
        HomeTab::detail_rows_for(d)
    }

    pub fn detail_rows_for(d: &GenreDetail) -> Vec<DetailRow<'_>> {
        let mut rows = Vec::new();
        if !d.subgenres.items.is_empty() && d.sub.is_none() {
            rows.push(DetailRow::Head("Subgenres"));
            rows.extend(d.subgenres.items.iter().take(6).map(DetailRow::Sub));
        }
        if !d.songs.items.is_empty() {
            rows.push(DetailRow::Head("Songs"));
            rows.extend(d.songs.items.iter().map(DetailRow::Song));
        }
        if !d.playlists.items.is_empty() {
            rows.push(DetailRow::Head("Playlists"));
            rows.extend(d.playlists.items.iter().map(DetailRow::Playlist));
        }
        if !d.albums.items.is_empty() {
            rows.push(DetailRow::Head("Albums"));
            rows.extend(d.albums.items.iter().map(DetailRow::Album));
        }
        if !d.artists.items.is_empty() {
            rows.push(DetailRow::Head("Artists"));
            rows.extend(d.artists.items.iter().map(DetailRow::Artist));
        }
        rows
    }
}

fn nav_list(table: &mut TableState, len: usize, dir: NavDir) {
    if len == 0 {
        return;
    }
    let current = table.selected();
    let next = match (current, dir) {
        (None, NavDir::Down) => 0,
        (None, NavDir::Up) => len - 1,
        (Some(i), NavDir::Down) => (i + 1).min(len - 1),
        (Some(i), NavDir::Up) => i.saturating_sub(1),
    };
    table.select(Some(next));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: u64, title: &str) -> Song {
        Song {
            id,
            title: title.to_string(),
            ..Song::default()
        }
    }

    #[test]
    fn sections_render_in_order_with_item_caps() {
        let mut home = HomeTab::default();
        assert!(home.home_rows().is_empty());
        let songs_idx = home
            .sections
            .iter()
            .position(|s| s.feed == HomeFeed::FeaturedSongs)
            .unwrap();
        home.sections[songs_idx].state = Load::Done;
        home.sections[songs_idx].data = HomeFeedData::Songs(Page {
            items: (0..10).map(|i| song(i, "s")).collect(),
            ..Page::default()
        });
        let rows = home.home_rows();
        assert_eq!(rows.len(), 1 + SECTION_ITEMS);
        assert!(matches!(
            rows[0],
            HomeRow::Head(HomeFeed::FeaturedSongs, "Featured Songs")
        ));
        for r in rows[1..].iter().enumerate() {
            let (i, row) = r;
            assert!(matches!(row, HomeRow::Song(s) if s.id == i as u64));
        }
    }

    #[test]
    fn nav_clamps_and_nothing_skipped() {
        let mut home = HomeTab::default();
        assert_eq!(home.sections_table.selected(), None);
        let idx = home
            .sections
            .iter()
            .position(|s| s.feed == HomeFeed::FeaturedSongs)
            .unwrap();
        home.sections[idx].state = Load::Done;
        home.sections[idx].data = HomeFeedData::Songs(Page {
            items: (0..3).map(|i| song(i, "s")).collect(),
            ..Page::default()
        });
        home.nav_sections(NavDir::Down);
        assert_eq!(home.sections_table.selected(), Some(0));
        home.nav_sections(NavDir::Up);
        assert_eq!(home.sections_table.selected(), Some(0));
        let total = 1 + 3;
        for _ in 0..(total * 2) {
            home.nav_sections(NavDir::Down);
        }
        assert_eq!(home.sections_table.selected(), Some(total - 1));
        home.nav_sections(NavDir::Up);
        assert_eq!(home.sections_table.selected(), Some(total - 2));
        for _ in 0..total {
            home.nav_sections(NavDir::Up);
        }
        assert_eq!(home.sections_table.selected(), Some(0));
        assert!(matches!(
            home.selected_row(),
            Some(HomeRow::Head(HomeFeed::FeaturedSongs, _))
        ));
    }

    #[test]
    fn breadcrumbs_enter_pop_and_jump() {
        let mut home = HomeTab::default();
        home.enter_level("Trending Songs".into());
        assert_eq!(home.crumbs, vec!["Home", "Trending Songs"]);
        assert!(home.back.is_empty());
        home.view = HomeView::List {
            songs: Page::default(),
            state: Load::Done,
            table: TableState::default(),
        };
        home.enter_level("Featured Artists".into());
        assert_eq!(
            home.crumbs,
            vec!["Home", "Trending Songs", "Featured Artists"]
        );
        assert_eq!(home.back.len(), 1);
        home.view = HomeView::Loading {
            state: Load::Loading,
        };
        home.pop_to_crumb(1);
        assert!(matches!(home.view, HomeView::List { .. }));
        assert_eq!(home.crumbs, vec!["Home", "Trending Songs"]);
        assert!(home.back.is_empty());
        home.pop_level();
        assert!(matches!(home.view, HomeView::Root));
        assert_eq!(home.crumbs, vec!["Home"]);
    }

    #[test]
    fn links_wrap_and_select() {
        let mut home = HomeTab::default();
        assert_eq!(
            home.selected_link(),
            Some(QuickLink::Feed(HomeFeed::FeaturedArtists))
        );
        home.nav_links(-1);
        assert_eq!(home.selected_link(), Some(QuickLink::Genres));
        for _ in 0..quick_links().len() + 1 {
            home.nav_links(1);
        }
        assert_eq!(
            home.selected_link(),
            Some(QuickLink::Feed(HomeFeed::FeaturedArtists))
        );
        home.nav_links(2);
        assert_eq!(
            home.selected_link(),
            Some(QuickLink::Feed(HomeFeed::NewAlbums))
        );
    }

    #[test]
    fn curated_reads_library_playlist_section() {
        let home = HomeTab::default();
        assert_eq!(home.curated_len(), 0);
        let idx = home
            .sections
            .iter()
            .position(|s| s.feed == HomeFeed::LibraryPlaylists)
            .unwrap();
        let mut home = HomeTab::default();
        home.sections[idx].state = Load::Done;
        home.sections[idx].data = HomeFeedData::Playlists(Page {
            items: vec![PlaylistCard {
                id: 1,
                name: "Demo Mix".into(),
                ..PlaylistCard::default()
            }],
            ..Page::default()
        });
        assert_eq!(home.curated_len(), 1);
        assert!(home.curated_ok());
    }

    #[test]
    fn failed_sections_show_no_rows_but_mark_busy_false() {
        let mut home = HomeTab::default();
        let idx = home
            .sections
            .iter()
            .position(|s| s.feed == HomeFeed::NewAlbums)
            .unwrap();
        home.sections[idx].state = Load::Failed("boom".into());
        let idx = home
            .sections
            .iter()
            .position(|s| s.feed == HomeFeed::RecentlyPlayed)
            .unwrap();
        home.sections[idx].state = Load::Loading;
        assert!(home.busy());
        assert!(home.home_rows().is_empty());
    }
}
