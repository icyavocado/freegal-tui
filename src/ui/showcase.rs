#[cfg(test)]
mod showcase {
    use crate::api::creds;
    use crate::api::task as api_task;
    use crate::app::ApiCommand;
    use crate::app::{App, AppEvent, ClickZone, RowArea};
    use crate::history;
    use crate::player::PlayerEvent;
    use crate::search::Load;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    struct Frame {
        name: String,
        title: String,
        text: String,
        zones: Vec<(u16, u16, u16, u16, String)>,
    }

    fn key_press(code: ratatui::crossterm::event::KeyCode) -> AppEvent {
        use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
        AppEvent::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        })
    }

    fn area_name(area: &RowArea) -> &'static str {
        match area {
            RowArea::SearchRoot => "searchroot",
            RowArea::SearchArtist => "searchartist",
            RowArea::SearchAlbum => "searchalbum",
            RowArea::HomeSections => "sections",
            RowArea::HomeCurated => "curated",
            RowArea::HomeList => "list",
            RowArea::HomeAlbum => "album",
            RowArea::HomeAlbums => "albums",
            RowArea::HomePlaylists => "playlists",
            RowArea::HomeArtists => "artists",
            RowArea::HomeGenres => "genres",
            RowArea::HomeGenreDetail => "genredetail",
            RowArea::MusicRoot => "musicroot",
            RowArea::MusicList => "musiclist",
            RowArea::MusicPlaylists => "musicplaylists",
            RowArea::Queue => "queue",
            RowArea::History => "history",
        }
    }

    fn shoot(app: &mut App, name: &str, title: &str, rows_map: &[(&str, &str)]) -> Frame {
        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        terminal.draw(|f| crate::ui::render(app, f)).unwrap();
        let buffer = terminal.backend().buffer();
        let width = buffer.area.width as usize;
        let mut text = String::new();
        for y in 0..buffer.area.height as usize {
            let mut row = String::new();
            for x in 0..width {
                row.push_str(buffer[(x as u16, y as u16)].symbol());
            }
            text.push_str(row.trim_end());
            text.push('\n');
        }
        let zones = app
            .click_zones
            .iter()
            .map(|(rect, zone)| {
                let tag = match zone {
                    ClickZone::Tab(i) => format!("TAB:{i}"),
                    ClickZone::Chip(i) => format!("CHIP:{i}"),
                    ClickZone::HomeCrumb(i) => format!("CRUMBH:{i}"),
                    ClickZone::MusicCrumb(i) => format!("CRUMBM:{i}"),
                    ClickZone::SearchInput => "SEARCHSTRIP".into(),
                    ClickZone::HistoryInput => "HISTFILTER".into(),
                    ClickZone::JumpSearch => "JUMPSTRIP".into(),
                    ClickZone::Progress => "PROGRESS".into(),
                    ClickZone::Rows(area, _) => {
                        let key = area_name(area);
                        match rows_map.iter().find(|(k, _)| *k == key) {
                            Some((_, t)) => format!("ROW:{t}"),
                            None => "ROWS:dbl".into(),
                        }
                    }
                };
                (rect.x, rect.y, rect.width, rect.height, tag)
            })
            .collect();
        Frame {
            name: name.into(),
            title: title.into(),
            text,
            zones,
        }
    }

    fn busy(app: &App) -> bool {
        app.home.busy()
            || app.music.busy()
            || app.home.pending.is_some()
            || app.music.pending.is_some()
            || app.home.pending_album.is_some()
    }

    fn pump(
        app: &mut App,
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
        runtime: &Arc<tokio::runtime::Runtime>,
        min_idle: Duration,
    ) {
        runtime.block_on(async {
            let start = Instant::now();
            let mut idle_since = Instant::now();
            loop {
                if start.elapsed() > Duration::from_secs(120) {
                    break;
                }
                if !busy(app)
                    && start.elapsed() > min_idle
                    && idle_since.elapsed() > Duration::from_millis(250)
                {
                    break;
                }
                match tokio::time::timeout(Duration::from_millis(80), rx.recv()).await {
                    Ok(Some(event)) => {
                        app.update(event);
                        idle_since = Instant::now();
                    }
                    Ok(None) => break,
                    Err(_) => {}
                }
            }
        });
        app.tick();
    }

    fn quiesce_after_keys(
        app: &mut App,
        keys: &[ratatui::crossterm::event::KeyCode],
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
        runtime: &Arc<tokio::runtime::Runtime>,
    ) {
        for k in keys {
            app.update(key_press(*k));
        }
        pump(app, rx, runtime, Duration::from_millis(0));
    }

    #[test]
    #[ignore]
    fn showcase_capture() {
        let out_dir = std::env::var("FREEGAL_SHOWCASE_DIR")
            .unwrap_or_else(|_| "/tmp/opencode/showcase".to_string());
        std::fs::create_dir_all(&out_dir).unwrap();

        let creds_path = creds::creds_path().unwrap();
        let creds = creds::load(&creds_path).unwrap().unwrap_or_default();
        let wizard = "00_login";

        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap(),
        );
        let (api_cmd_tx, api_cmd_rx) = tokio::sync::mpsc::channel::<ApiCommand>(16);
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
        runtime.block_on(async {
            tokio::spawn(api_task::run(
                api_cmd_rx,
                event_tx.clone(),
                creds.clone(),
                Some(creds_path),
            ));
        });

        let mut app = App::new(Some(&creds));
        app.history_path = history::history_path();
        app.history = history::load(&app.history_path);
        app.connect_api(api_cmd_tx.clone());
        if app.restoring {
            let _ = api_cmd_tx.try_send(ApiCommand::Restore);
        }

        let mut frames = Vec::new();
        let mut wizard_app = App::new(None);
        frames.push(shoot(&mut wizard_app, "00_login", "Login", &[]));

        pump(&mut app, &mut event_rx, &runtime, Duration::from_secs(3));
        assert!(
            app.home.sections.iter().any(|s| s.state == Load::Done),
            "home feeds never finished"
        );
        let _ = wizard;

        let featured = app
            .home
            .sections
            .iter()
            .find(|s| matches!(s.data, crate::home::HomeFeedData::Songs(_)))
            .map(|s| match &s.data {
                crate::home::HomeFeedData::Songs(p) => p.items[0].clone(),
                _ => unreachable!(),
            })
            .expect("featured songs feed");
        let songs: Vec<crate::api::models::Song> = app
            .home
            .sections
            .iter()
            .find_map(|s| match &s.data {
                crate::home::HomeFeedData::Songs(p) => Some(p.items.clone()),
                _ => None,
            })
            .unwrap_or_default();
        app.queue.play_now(featured.clone());
        for extra in songs.iter().skip(1).take(2) {
            app.queue.append(extra.clone());
        }
        app.update(AppEvent::Player(PlayerEvent::Status {
            time: Some(4.0),
            duration: featured.full_duration_sec.map(|d| d as f64),
            paused: Some(false),
        }));
        frames.push(shoot(
            &mut app,
            "01_home",
            "Home",
            &[
                ("curated", "08_home_library_playlist"),
                ("sections", "02_home_songs"),
            ],
        ));

        quiesce_after_keys(
            &mut app,
            &[ratatui::crossterm::event::KeyCode::Char('3')],
            &mut event_rx,
            &runtime,
        );
        frames.push(shoot(&mut app, "13_queue", "Queue", &[]));
        quiesce_after_keys(
            &mut app,
            &[ratatui::crossterm::event::KeyCode::Char('2')],
            &mut event_rx,
            &runtime,
        );
        frames.push(shoot(
            &mut app,
            "11_mymusic",
            "My Music",
            &[("musicroot", "12_mymusic_wishlist")],
        ));
        app.open_music_drill(crate::mymusic::MusicFeed::Wishlist);
        pump(&mut app, &mut event_rx, &runtime, Duration::from_millis(0));
        frames.push(shoot(
            &mut app,
            "12_mymusic_wishlist",
            "My Music › Wishlist",
            &[],
        ));
        quiesce_after_keys(
            &mut app,
            &[ratatui::crossterm::event::KeyCode::Char('4')],
            &mut event_rx,
            &runtime,
        );
        frames.push(shoot(&mut app, "14_history", "History", &[]));
        quiesce_after_keys(
            &mut app,
            &[ratatui::crossterm::event::KeyCode::Char('5')],
            &mut event_rx,
            &runtime,
        );
        frames.push(shoot(&mut app, "15_downloads", "Downloads", &[]));

        quiesce_after_keys(
            &mut app,
            &[ratatui::crossterm::event::KeyCode::Char('1')],
            &mut event_rx,
            &runtime,
        );
        if app.home.curated_len() > 0 {
            app.home.curated_table.select(Some(0));
        }
        let library_playlist = app.home.selected_playlist().cloned();
        let drill = [
            (
                crate::home::HomeFeed::FeaturedSongs,
                ("02_home_songs", "Featured Songs", &[][..]),
            ),
            (
                crate::home::HomeFeed::TopSongs,
                ("03_home_trending", "Trending Songs", &[][..]),
            ),
            (
                crate::home::HomeFeed::FeaturedArtists,
                (
                    "04_home_artists",
                    "Artists",
                    &[("artists", "19_home_artist_albums")][..],
                ),
            ),
            (
                crate::home::HomeFeed::NewAlbums,
                (
                    "05_home_new_albums",
                    "New Albums",
                    &[("albums", "20_home_new_album")][..],
                ),
            ),
            (
                crate::home::HomeFeed::FeaturedAlbums,
                (
                    "06_home_albums",
                    "Featured Albums",
                    &[("albums", "18_home_featured_album")][..],
                ),
            ),
            (
                crate::home::HomeFeed::FeaturedPlaylists,
                (
                    "07_home_playlists",
                    "Featured Playlists",
                    &[("playlists", "21_home_playlist")][..],
                ),
            ),
        ];
        for (feed, (name, title, rows_map)) in drill {
            if app
                .home
                .sections
                .iter()
                .find(|s| s.feed == feed)
                .map(|s| s.state == Load::Done && matches!(s.data, crate::home::HomeFeedData::None))
                .unwrap_or(false)
            {
                continue;
            }
            app.open_feed_drill(feed);
            pump(&mut app, &mut event_rx, &runtime, Duration::from_millis(0));
            if matches!(app.home.view, crate::home::HomeView::Loading { .. }) {
                continue;
            }
            frames.push(shoot(&mut app, name, title, rows_map));
            let deeper = match feed {
                crate::home::HomeFeed::FeaturedArtists => {
                    Some(("19_home_artist_albums", "Artist Albums"))
                }
                crate::home::HomeFeed::NewAlbums => Some(("20_home_new_album", "New Album")),
                crate::home::HomeFeed::FeaturedAlbums => {
                    Some(("18_home_featured_album", "Featured Album"))
                }
                crate::home::HomeFeed::FeaturedPlaylists => {
                    Some(("21_home_playlist", "Playlist Songs"))
                }
                _ => None,
            };
            if let Some((drill_name, drill_title)) = deeper {
                let populated = match &mut app.home.view {
                    crate::home::HomeView::Artists { artists, table, .. } => {
                        let ok = !artists.items.is_empty();
                        if ok {
                            table.select(Some(0));
                        }
                        ok
                    }
                    crate::home::HomeView::Albums { albums, table, .. } => {
                        let ok = !albums.items.is_empty();
                        if ok {
                            table.select(Some(0));
                        }
                        ok
                    }
                    crate::home::HomeView::Playlists {
                        playlists, table, ..
                    } => {
                        let ok = !playlists.items.is_empty();
                        if ok {
                            table.select(Some(0));
                        }
                        ok
                    }
                    _ => false,
                };
                if populated {
                    app.update(key_press(ratatui::crossterm::event::KeyCode::Enter));
                    pump(&mut app, &mut event_rx, &runtime, Duration::from_millis(0));
                    let shaped = match feed {
                        crate::home::HomeFeed::FeaturedArtists => {
                            matches!(app.home.view, crate::home::HomeView::Albums { .. })
                        }
                        crate::home::HomeFeed::FeaturedPlaylists => {
                            matches!(app.home.view, crate::home::HomeView::List { .. })
                        }
                        _ => matches!(app.home.view, crate::home::HomeView::Album(_)),
                    };
                    if shaped {
                        frames.push(shoot(&mut app, drill_name, drill_title, &[]));
                    }
                    app.update(key_press(ratatui::crossterm::event::KeyCode::Esc));
                    pump(&mut app, &mut event_rx, &runtime, Duration::from_millis(0));
                }
            }
            app.update(key_press(ratatui::crossterm::event::KeyCode::Esc));
            pump(&mut app, &mut event_rx, &runtime, Duration::from_millis(0));
            assert!(
                app.home.is_root(),
                "drill {name} left {name} view open — Esc failed"
            );
        }
        if let Some(p) = library_playlist {
            app.open_home_list(crate::home::DrillKind::Playlist(p.id, p.name.clone()));
            pump(&mut app, &mut event_rx, &runtime, Duration::from_millis(0));
            if !matches!(app.home.view, crate::home::HomeView::Loading { .. }) {
                frames.push(shoot(
                    &mut app,
                    "08_home_library_playlist",
                    "Library Playlist",
                    &[],
                ));
            }
            app.home.pop_level();
            std::thread::sleep(Duration::from_millis(60));
        }

        app.open_genres_list();
        pump(&mut app, &mut event_rx, &runtime, Duration::from_millis(0));
        frames.push(shoot(
            &mut app,
            "09_genres",
            "Genres",
            &[
                ("genres", "10_genre_pop"),
                ("curated", "08_home_library_playlist"),
            ],
        ));
        app.open_genre_detail("Pop".into(), None);
        pump(&mut app, &mut event_rx, &runtime, Duration::from_millis(0));
        frames.push(shoot(&mut app, "10_genre_pop", "Genre: Pop", &[]));
        app.home.pop_level();

        app.update(key_press(ratatui::crossterm::event::KeyCode::Char('?')));
        frames.push(shoot(&mut app, "16_help", "Help", &[]));
        app.update(key_press(ratatui::crossterm::event::KeyCode::Esc));

        app.update(AppEvent::Player(PlayerEvent::AudioDevices {
            devices: vec![
                ("auto".into(), "Auto (recommended)".into()),
                ("pulse".into(), "PulseAudio output".into()),
                ("alsa/default".into(), "ALSA default device".into()),
            ],
        }));
        app.update(key_press(ratatui::crossterm::event::KeyCode::Char('o')));
        frames.push(shoot(&mut app, "17_audio", "Audio output", &[]));

        for frame in &frames {
            std::fs::write(format!("{out_dir}/{}.txt", frame.name), &frame.text).unwrap();
        }
        let manifest = json!({
            "size": [100usize, 32usize],
            "screens": frames.iter().map(|f| json!({
                "name": f.name,
                "title": f.title,
                "text": f.text,
                "zones": f.zones.iter().map(|(x, y, w, h, tag)| json!({
                    "x": x, "y": y, "w": w, "h": h, "tag": tag,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        std::fs::write(
            format!("{out_dir}/manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        for f in &frames {
            println!("captured {}: {} zones", f.name, f.zones.len());
        }
    }
}
