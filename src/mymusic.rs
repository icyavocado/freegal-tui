use crate::api::models::{Page, PlaylistCard, Song};
use crate::search::Load;
use ratatui::widgets::TableState;

pub const MUSIC_FEED_LIMIT: u64 = 4;
pub const MUSIC_DRILL_LIMIT: u64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicFeed {
    Playlists,
    Downloaded,
    Favorites,
    Wishlist,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum MusicData {
    #[default]
    None,
    Songs(Page<Song>),
    Playlists(Page<PlaylistCard>),
}

#[derive(Debug)]
pub struct MusicSection {
    pub feed: MusicFeed,
    pub title: &'static str,
    pub state: Load,
    pub data: MusicData,
}

pub fn music_sections() -> [(&'static str, MusicFeed); 4] {
    [
        ("My Playlists", MusicFeed::Playlists),
        ("Downloaded Songs", MusicFeed::Downloaded),
        ("Favorites", MusicFeed::Favorites),
        ("Wishlist", MusicFeed::Wishlist),
    ]
}

pub fn empty_line(feed: MusicFeed) -> &'static str {
    match feed {
        MusicFeed::Playlists => "Your created playlists will appear here.",
        MusicFeed::Downloaded => "Your downloaded songs will appear here.",
        MusicFeed::Favorites => "Your Favorite Songs, Playlists, Albums will appear here.",
        MusicFeed::Wishlist => "Your Wishlist Songs will appear here.",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicDrill {
    Feed(MusicFeed),
}

#[derive(Debug)]
pub struct MusicPending {
    pub kind: MusicDrill,
    pub gen: u64,
}

#[derive(Debug, Default)]
pub enum MusicView {
    #[default]
    Root,
    Loading {
        state: Load,
    },
    Playlists {
        page: Page<PlaylistCard>,
        state: Load,
        table: TableState,
    },
    Songs {
        songs: Page<Song>,
        state: Load,
        table: TableState,
    },
}

#[derive(Debug)]
pub struct MusicTab {
    pub sections: Vec<MusicSection>,
    pub gen: u64,
    pub view: MusicView,
    pub crumb: Option<String>,
    pub pending: Option<MusicPending>,
    pub last_width: u16,
    pub root_table: TableState,
}

impl Default for MusicTab {
    fn default() -> Self {
        Self {
            sections: music_sections()
                .into_iter()
                .map(|(title, feed)| MusicSection {
                    feed,
                    title,
                    state: Load::Idle,
                    data: MusicData::default(),
                })
                .collect(),
            gen: 0,
            view: MusicView::Root,
            crumb: None,
            pending: None,
            last_width: 0,
            root_table: TableState::default(),
        }
    }
}

pub enum MusicRow<'a> {
    Head(MusicFeed, &'static str),
    Song(&'a Song),
    Playlist(&'a PlaylistCard),
}

pub fn music_rows(tab: &MusicTab) -> Vec<MusicRow<'_>> {
    let mut rows = Vec::new();
    for s in &tab.sections {
        if s.state != Load::Done {
            continue;
        }
        match &s.data {
            MusicData::Playlists(page) if !page.items.is_empty() => {
                rows.push(MusicRow::Head(s.feed, s.title));
                rows.extend(
                    page.items
                        .iter()
                        .take(MUSIC_FEED_LIMIT as usize)
                        .map(MusicRow::Playlist),
                );
            }
            MusicData::Songs(page) if !page.items.is_empty() => {
                rows.push(MusicRow::Head(s.feed, s.title));
                rows.extend(
                    page.items
                        .iter()
                        .take(MUSIC_FEED_LIMIT as usize)
                        .map(MusicRow::Song),
                );
            }
            _ => {
                rows.push(MusicRow::Head(s.feed, s.title));
            }
        }
    }
    rows
}

impl MusicTab {
    pub fn busy(&self) -> bool {
        self.sections.iter().any(|s| s.state.is_busy())
            || matches!(self.view, MusicView::Loading { .. })
    }

    pub fn mark_loading(&mut self) {
        for s in &mut self.sections {
            s.state = Load::Loading;
        }
    }

    pub fn section_mut(&mut self, feed: MusicFeed) -> Option<&mut MusicSection> {
        self.sections.iter_mut().find(|s| s.feed == feed)
    }

    pub fn section(&self, feed: MusicFeed) -> Option<&MusicSection> {
        self.sections.iter().find(|s| s.feed == feed)
    }

    pub fn nav(&mut self, dir: NavDir) {
        let len = music_rows(self).len();
        if len == 0 {
            return;
        }
        let table = &mut self.root_table;
        let next = match (table.selected(), dir) {
            (None, NavDir::Down) => 0,
            (None, NavDir::Up) => len - 1,
            (Some(i), NavDir::Down) => (i + 1).min(len - 1),
            (Some(i), NavDir::Up) => i.saturating_sub(1),
        };
        table.select(Some(next));
    }

    pub fn edge(&mut self, dir: NavDir) {
        let len = music_rows(self).len();
        if len > 0 {
            self.root_table.select(Some(match dir {
                NavDir::Up => 0,
                NavDir::Down => len - 1,
            }));
        }
    }

    pub fn selected_row(&self) -> Option<MusicRow<'_>> {
        let idx = self.root_table.selected()?;
        music_rows(self).into_iter().nth(idx)
    }

    pub fn reset(&mut self) {
        self.view = MusicView::Root;
        self.crumb = None;
        self.pending = None;
    }

    pub fn is_root(&self) -> bool {
        matches!(self.view, MusicView::Root)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NavDir {
    Up,
    Down,
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
    fn sections_in_web_order_and_headed() {
        let mut tab = MusicTab::default();
        assert!(music_rows(&tab).is_empty());
        let idx = tab
            .sections
            .iter()
            .position(|s| s.feed == MusicFeed::Playlists)
            .unwrap();
        assert_eq!(idx, 0);
        tab.sections[idx].state = Load::Done;
        tab.sections[idx].data = MusicData::Playlists(Page {
            items: (0..6)
                .map(|i| PlaylistCard {
                    id: i,
                    name: format!("p{i}"),
                    ..PlaylistCard::default()
                })
                .collect(),
            ..Page::default()
        });
        let rows = music_rows(&tab);
        assert_eq!(rows.len(), 1 + MUSIC_FEED_LIMIT as usize);
        assert!(matches!(
            rows[0],
            MusicRow::Head(MusicFeed::Playlists, "My Playlists")
        ));
    }

    #[test]
    fn empty_done_section_still_shows_heading() {
        let mut tab = MusicTab::default();
        let idx = tab
            .sections
            .iter()
            .position(|s| s.feed == MusicFeed::Wishlist)
            .unwrap();
        tab.sections[idx].state = Load::Done;
        tab.sections[idx].data = MusicData::None;
        let rows = music_rows(&tab);
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0], MusicRow::Head(MusicFeed::Wishlist, _)));
        assert_eq!(
            empty_line(MusicFeed::Wishlist),
            "Your Wishlist Songs will appear here."
        );
    }

    #[test]
    fn nav_clamps_from_empty() {
        let mut tab = MusicTab::default();
        tab.nav(NavDir::Down);
        assert_eq!(tab.root_table.selected(), None);
    }
}
