use super::chrome;
use super::render;
use crate::api::models::{Album, Artist, ArtistRef, Page, PlaylistCard, Song};
use crate::app::App;
use crate::home::{HomeFeed, HomeFeedData, HomeView};
use crate::search::{Load, SearchView};
use insta::assert_snapshot;
use ratatui::{backend::TestBackend, Terminal};

fn draw(app: &mut App, w: u16, h: u16) -> ratatui::backend::TestBackend {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("TestBackend is infallible");
    terminal
        .draw(|f| render(app, f))
        .expect("TestBackend draw is infallible");
    terminal.backend().clone()
}

fn wizard_app() -> App {
    App::new(None)
}

fn home_app() -> App {
    let mut app = App::new(None);
    app.make_home();
    app
}

#[test]
fn snapshot_login_wizard_80x24() {
    let mut app = wizard_app();
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_full_chrome_80x24() {
    let mut app = home_app();
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_no_hint_chrome_80x18() {
    let mut app = home_app();
    let backend = draw(&mut app, 80, 18);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_folded_tabs_60x14() {
    let mut app = home_app();
    let backend = draw(&mut app, 60, 14);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_help_overlay_80x24() {
    let mut app = home_app();
    app.help_open = true;
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_errors_overlay_80x24() {
    let mut app = home_app();
    app.toast_error("search failed: 61: invalid session — re-login");
    app.toast_error("download failed: HTTP 502");
    app.toast_error("playback unavailable (connection lost)");
    app.errors_open = true;
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_too_small_card_40x10() {
    let mut app = home_app();
    let backend = draw(&mut app, 40, 10);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_download_confirm_80x24() {
    let mut app = search_rows_app();
    if let Some(SearchView::Root(home)) = app.search.top_mut() {
        home.table.select(Some(0));
    }
    app.update(crate::app::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::NONE,
    )));
    assert!(app.download_confirm.is_some(), "y opened the modal");
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_nord_full_chrome_80x24() {
    let mut app = home_app();
    let backend =
        crate::ui::theme::with_palette(&crate::ui::theme::NORD, || draw(&mut app, 80, 24));
    assert_snapshot!(backend);
}

#[test]
fn snapshot_nord_download_confirm_80x24() {
    let mut app = search_rows_app();
    if let Some(SearchView::Root(home)) = app.search.top_mut() {
        home.table.select(Some(0));
    }
    app.update(crate::app::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('y'),
        crossterm::event::KeyModifiers::NONE,
    )));
    assert!(app.download_confirm.is_some(), "y opened the modal");
    let backend =
        crate::ui::theme::with_palette(&crate::ui::theme::NORD, || draw(&mut app, 80, 24));
    assert_snapshot!(backend);
}

#[test]
fn snapshot_clear_history_confirm_80x24() {
    let mut app = history_app();
    app.update(crate::app::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('D'),
        crossterm::event::KeyModifiers::NONE,
    )));
    assert!(app.clear_history_question, "D opened the confirm");
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_audio_menu_80x24() {
    let mut app = home_app();
    app.update(crate::app::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('o'),
        crossterm::event::KeyModifiers::NONE,
    )));
    assert!(app.audio_menu_open, "o opened the menu");
    app.audio_devices = vec![
        ("pipewire".into(), "System default (PipeWire)".to_string()),
        (
            "pipewire/bluez_output.68_F2_1F_3E_35_1D.1".into(),
            "Obsidian 2".to_string(),
        ),
        (
            "alsa/sysdefault:CARD=sofsoundwire".into(),
            "sof-soundwire speakers".to_string(),
        ),
    ];
    app.audio_menu_table.select(Some(1));
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_downloads_populated_80x24() {
    let mut app = home_app();
    app.update(crate::app::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('5'),
        crossterm::event::KeyModifiers::NONE,
    )));
    app.downloaded_ids = vec![1, 2];
    app.downloads = vec![
        crate::app::DownloadRow {
            song_id: 1,
            title: "Bohemian Rhapsody".into(),
            artist: "Queen".into(),
            file_name: "01 Bohemian Rhapsody.mp3".into(),
        },
        crate::app::DownloadRow {
            song_id: 2,
            title: "Some Long Song Title That Should Truncate At Thirty Chars".into(),
            artist: "Abba".into(),
            file_name: "02 Long Title.mp3".into(),
        },
    ];
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_queue_tab_80x24() {
    let mut app = home_app();
    app.update(crate::app::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('3'),
        crossterm::event::KeyModifiers::NONE,
    )));
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

fn song(id: u64, title: &str, artist: &str, album: &str, secs: Option<u64>) -> Song {
    Song {
        id,
        album_id: None,
        title: title.to_string(),
        provider: Some(1),
        artist: Some(ArtistRef {
            id: "YXJ0aXN0LTE".into(),
            name: artist.to_string(),
        }),
        album: Some(Album {
            id: 42,
            title: album.to_string(),
            provider: Some(1),
            ..Album::default()
        }),
        genre: None,
        explicit: false,
        track_number: None,
        sample_duration_sec: Some(30),
        full_duration_sec: secs,
        download_status: None,
        stream_status: None,
        stream_url: None,
        stream_url_camel: None,
        sort_id: None,
    }
}

fn artist(id: &str, name: &str) -> Artist {
    Artist {
        id: id.to_string(),
        name: name.to_string(),
        label: None,
        image: None,
    }
}

fn page<T>(items: Vec<T>) -> Page<T> {
    Page {
        items,
        limit: 50,
        offset: 0,
        next: None,
        previous: None,
    }
}

pub(crate) fn search_rows_app() -> App {
    let mut app = home_app();
    app.set_tab(crate::app::Tab::Home);
    if let Some(SearchView::Root(home)) = app.search.top_mut() {
        home.gen = 1;
        home.input = tui_input::Input::new("abba".to_string());
        home.kind_state = [Load::Done, Load::Done, Load::Done];
        home.songs = page(vec![
            song(
                1,
                "Some Long Song Title That Should Truncate At Thirty Chars",
                "Abba",
                "Arrival",
                Some(231),
            ),
            song(2, "Short Song", "Cher", "Living Proof", Some(204)),
            {
                let mut s = song(3, "Instrumental", "ABBA", "Arrival", None);
                s.explicit = true;
                s
            },
        ]);
        home.artists = page(vec![artist("UXVlZW4", "Queen")]);
        home.albums = page(vec![Album {
            id: 9,
            title: "A Night at the Opera".to_string(),
            provider: Some(1),
            artist: Some(ArtistRef {
                id: "UXVlZW4".into(),
                name: "Queen".to_string(),
            }),
            year: Some(1975),
            ..Album::default()
        }]);
    }
    app
}

#[test]
fn snapshot_search_root_rows_80x24() {
    let mut app = search_rows_app();
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_search_zero_results_80x24() {
    let mut app = search_rows_app();
    let query = "zzzz".to_string();
    if let Some(crate::search::SearchView::Root(home)) = app.search.top_mut() {
        home.input = tui_input::Input::new(query);
        home.songs = crate::api::models::Page::default();
        home.artists = crate::api::models::Page::default();
        home.albums = crate::api::models::Page::default();
        home.table.select(None);
    }
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

pub(crate) fn history_app() -> App {
    let mut app = home_app();
    app.history_path =
        std::env::temp_dir().join(format!("freegal-history-test-{}", std::process::id()));
    app.set_tab(crate::app::Tab::History);
    app.history = vec![
        crate::history::Entry {
            id: 1,
            provider: 1,
            title: "Some Long Song Title That Should Truncate At Thirty Chars".into(),
            artist_name: "Abba".into(),
            album_title: "Arrival".into(),
            genre: Some("Pop".into()),
            duration_secs: Some(231),
            played_at_unix: 999_999_999,
            ..crate::history::Entry::default()
        },
        crate::history::Entry {
            id: 2,
            provider: 1,
            title: "Short Song".into(),
            artist_name: "Cher".into(),
            album_title: "Living Proof".into(),
            duration_secs: Some(204),
            played_at_unix: 999_999_400,
            ..crate::history::Entry::default()
        },
        crate::history::Entry {
            id: 3,
            provider: 1,
            title: "Instrumental".into(),
            artist_name: "ABBA".into(),
            album_title: "Arrival".into(),
            duration_secs: Some(180),
            played_at_unix: 999_997_600,
            ..crate::history::Entry::default()
        },
    ];
    // Pinned clock: rel_time renders from app.now_unix, not the wall clock.
    app.now_unix = 999_999_999; // entry 1: 1s → just now; 2: 599s → 9m; 3: 2399s → 39m
    app.history_focus = crate::search::Focus::Results;
    app.history_table.select(Some(0));
    app
}

#[test]
fn snapshot_history_populated_80x24() {
    let mut app = history_app();
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_history_empty_80x24() {
    let mut app = home_app();
    app.history_path =
        std::env::temp_dir().join(format!("freegal-history-empty-{}", std::process::id()));
    app.update(crate::app::AppEvent::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('4'),
        crossterm::event::KeyModifiers::NONE,
    )));
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_search_artist_ctx_80x24() {
    let mut app = search_rows_app();
    if let Some(SearchView::Root(home)) = app.search.top_mut() {
        if let Some(a) = home.artists.items.first().cloned() {
            app.search
                .stack
                .push(SearchView::Artist(Box::new(crate::search::ArtistContext {
                    artist: a,
                    albums: page(vec![
                        Album {
                            id: 9,
                            title: "A Night at the Opera".to_string(),
                            provider: Some(1),
                            year: Some(1975),
                            songs: vec![song(
                                10,
                                "Bohemian Rhapsody",
                                "Queen",
                                "A Night at the Opera",
                                Some(355),
                            )],
                            ..Album::default()
                        },
                        Album {
                            id: 10,
                            title: "News of the World".to_string(),
                            provider: Some(1),
                            year: Some(1977),
                            ..Album::default()
                        },
                    ]),
                    state: Load::Done,
                    gen: 1,
                    table: Default::default(),
                    pending_album: None,
                })));
        }
    }
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_search_album_ctx_80x24() {
    let mut app = search_rows_app();
    app.search
        .stack
        .push(SearchView::Album(Box::new(crate::search::AlbumContext {
            album: Album {
                id: 9,
                title: "A Night at the Opera".to_string(),
                provider: Some(1),
                artist: Some(ArtistRef {
                    id: "UXVlZW4".into(),
                    name: "Queen".to_string(),
                }),
                year: Some(1975),
                genre: Some("Rock".to_string()),
                songs: vec![
                    song(
                        10,
                        "Death on Two Legs (Dedicated to...)",
                        "Queen",
                        "A Night at the Opera",
                        Some(223),
                    ),
                    song(
                        11,
                        "Bohemian Rhapsody",
                        "Queen",
                        "A Night at the Opera",
                        Some(355),
                    ),
                    song(
                        12,
                        "You're My Best Friend",
                        "Queen",
                        "A Night at the Opera",
                        Some(172),
                    ),
                ],
                ..Album::default()
            },
            table: Default::default(),
        })));
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn chrome_ladder_matches_spec_rungs() {
    use chrome::ChromeMode;
    let (w, h) = (80u16, 24u16);
    assert_eq!(
        chrome::layout_ladder(ratatui::layout::Rect::new(0, 0, w, h))
            .unwrap()
            .mode,
        ChromeMode::Full
    );
    assert_eq!(
        chrome::layout_ladder(ratatui::layout::Rect::new(0, 0, 80, 18))
            .unwrap()
            .mode,
        ChromeMode::NoHint
    );
    assert_eq!(
        chrome::layout_ladder(ratatui::layout::Rect::new(0, 0, 60, 14))
            .unwrap()
            .mode,
        ChromeMode::FoldedTabs
    );
    assert_eq!(
        chrome::layout_ladder(ratatui::layout::Rect::new(0, 0, 40, 10))
            .unwrap()
            .mode,
        ChromeMode::Minimal
    );
    assert!(chrome::layout_ladder(ratatui::layout::Rect::new(0, 0, 40, 9)).is_none());
}

fn discover_app() -> App {
    let mut app = home_app();
    let explicit = {
        let mut s = song(1, "Golden Hour", "Kacey Musgraves", "Golden Hour", None);
        s.explicit = true;
        s
    };
    let songs_data = HomeFeedData::Songs(page(vec![
        explicit,
        song(
            2,
            "Midnight City",
            "M83",
            "Hurry Up, We're Dreaming",
            Some(243),
        ),
    ]));
    let albums_data = HomeFeedData::Albums(page(vec![Album {
        id: 12075438,
        title: "Stages".to_string(),
        provider: Some(1),
        artist: Some(ArtistRef {
            id: "TmVpbCBEaWFtb25k".into(),
            name: "Neil Diamond".into(),
        }),
        genre: Some("Pop".into()),
        year: Some(2003),
        ..Album::default()
    }]));
    let playlists_data = HomeFeedData::Playlists(page(vec![PlaylistCard {
        id: 397967,
        name: "Study & Chill".into(),
        songs_count: 35,
        ..PlaylistCard::default()
    }]));
    for section in app.home.sections.iter_mut() {
        section.state = Load::Done;
        section.data = match section.feed {
            HomeFeed::FeaturedSongs => songs_data.clone(),
            HomeFeed::MoreForYou => albums_data.clone(),
            HomeFeed::LibraryPlaylists => playlists_data.clone(),
            _ => HomeFeedData::None,
        };
    }
    app.home.sections_table.select(Some(1));
    app.home.curated_table.select(Some(0));
    app
}

#[test]
fn snapshot_home_root_80x24() {
    let mut app = discover_app();
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_home_songs_drill_80x24() {
    let mut app = discover_app();
    app.home.view = HomeView::List {
        songs: page(vec![
            song(
                35204358,
                "Weightless",
                "Marconi Union",
                "Ambience",
                Some(481),
            ),
            song(
                35204359,
                "Teardrop",
                "Massive Attack",
                "Mezzanine",
                Some(330),
            ),
        ]),
        state: Load::Done,
        table: {
            let mut t = ratatui::widgets::TableState::default();
            t.select(Some(0));
            t
        },
    };
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_home_too_small_40x10() {
    let mut app = discover_app();
    let backend = draw(&mut app, 40, 10);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_my_music_root_80x24() {
    let mut app = home_app();
    app.set_tab(crate::app::Tab::MyMusic);
    for (feed, data) in [
        (
            crate::mymusic::MusicFeed::Playlists,
            crate::mymusic::MusicData::Playlists(page(vec![PlaylistCard {
                id: 397967,
                name: "Road Trip 2026".into(),
                description: "Highway tracks".into(),
                songs_count: 24,
                ..PlaylistCard::default()
            }])),
        ),
        (
            crate::mymusic::MusicFeed::Downloaded,
            crate::mymusic::MusicData::Songs(page(vec![song(
                35204358,
                "Weightless",
                "Marconi Union",
                "Ambience",
                Some(481),
            )])),
        ),
    ] {
        let idx = app
            .music
            .sections
            .iter()
            .position(|s| s.feed == feed)
            .unwrap();
        app.music.sections[idx].state = Load::Done;
        app.music.sections[idx].data = data;
    }
    for s in &mut app.music.sections {
        if s.state == Load::Idle {
            s.state = Load::Done;
        }
    }
    app.music.root_table.select(Some(1));
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_my_music_wishlist_drill_80x24() {
    let mut app = home_app();
    app.set_tab(crate::app::Tab::MyMusic);
    app.music.view = crate::mymusic::MusicView::Songs {
        songs: page(vec![song(
            120001,
            "Save for Later",
            "Later Band",
            "Wishlist Vol. 1",
            Some(214),
        )]),
        state: Load::Done,
        table: {
            let mut t = ratatui::widgets::TableState::default();
            t.select(Some(0));
            t
        },
    };
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

#[test]
fn snapshot_genres_list_80x24() {
    let mut app = discover_app();
    app.home.view = HomeView::Genres {
        page: crate::api::models::Page {
            items: vec![
                genre("Pop"),
                genre("Rock"),
                genre("Jazz"),
                genre("Country"),
                genre("Hip-hop/Rap"),
                genre("Classical"),
            ],
            ..crate::api::models::Page::default()
        },
        state: Load::Done,
        table: {
            let mut t = ratatui::widgets::TableState::default();
            t.select(Some(0));
            t
        },
    };
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}

fn genre(title: &str) -> crate::api::models::Genre {
    crate::api::models::Genre {
        title: title.into(),
        ..crate::api::models::Genre::default()
    }
}

#[test]
fn snapshot_genre_detail_80x24() {
    let mut app = discover_app();
    app.home.view = HomeView::Detail(Box::new(crate::home::GenreDetail {
        genre: "pop".into(),
        sub: None,
        songs: page(vec![song(
            900,
            "Blinding Lights",
            "The Weeknd",
            "After Hours",
            Some(200),
        )]),
        albums: crate::api::models::Page::default(),
        playlists: page(vec![PlaylistCard {
            id: 5,
            name: "Heart Beats".into(),
            ..PlaylistCard::default()
        }]),
        artists: crate::api::models::Page::default(),
        subgenres: crate::api::models::Page {
            items: vec![genre("Synth-pop"), genre("Acoustic Pop")],
            ..crate::api::models::Page::default()
        },
        state: Load::Done,
        table: {
            let mut t = ratatui::widgets::TableState::default();
            t.select(Some(0));
            t
        },
    }));
    let backend = draw(&mut app, 80, 24);
    assert_snapshot!(backend);
}
