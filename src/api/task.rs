use std::path::PathBuf;

use tokio::sync::mpsc::{Receiver, UnboundedSender};

use crate::api::client::ApiClient;
use crate::api::creds::StoredCreds;
use crate::app::{ApiCommand, ApiEvent, AppEvent};

async fn claim_file(
    client: &mut ApiClient,
    song: &crate::api::models::Song,
    redownload: bool,
) -> crate::api::ApiResult<crate::app::DownloadOutcome> {
    use crate::api::{download, ApiError};

    let provider = song.provider.unwrap_or(1);
    let url = if redownload {
        client.redownload_song(song.id, provider).await
    } else {
        client.download_song(song.id, provider).await
    }?;
    let name = download::filename_for(song);
    let Some(dir) = download::default_dir() else {
        return Err(ApiError::Network("no audio directory found".into()));
    };
    let dest = download::dedupe_path(&dir, &name);
    let path = client
        .download_to_file(&url, dest)
        .await
        .map_err(|e| ApiError::Network(format!("{e}")))?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let details = client.user_details().await.ok();
    Ok(crate::app::DownloadOutcome {
        song_id: song.id,
        title: song.title.clone(),
        artist: song
            .artist
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_default(),
        file_name,
        details,
    })
}

pub async fn run(
    mut rx: Receiver<ApiCommand>,
    events: UnboundedSender<AppEvent>,
    creds: StoredCreds,
    creds_path: Option<PathBuf>,
) {
    let subdomain = creds.subdomain.clone();
    let subdomain = if subdomain.trim().is_empty() {
        String::new()
    } else {
        subdomain
    };
    let mut client = match ApiClient::new(subdomain, creds, creds_path) {
        Ok(c) => c,
        Err(e) => {
            let _ = events.send(AppEvent::Api(ApiEvent::Restored { result: Err(e) }));
            while rx.recv().await.is_some() {}
            return;
        }
    };

    while let Some(command) = rx.recv().await {
        let _ = events.send(AppEvent::Api(ApiEvent::NetBusy));
        let event = match command {
            ApiCommand::ResolveLibrary { subdomain } => {
                let result = client.resolve_library(&subdomain).await;
                AppEvent::Api(ApiEvent::LibraryResolved { result })
            }
            ApiCommand::SetLibraryId { id } => {
                client.set_library_id(id);
                AppEvent::Api(ApiEvent::LibraryIdSet { result: Ok(()) })
            }
            ApiCommand::Restore => {
                let result = silent_restore(&mut client).await;
                match &result {
                    Ok(details) => {
                        tracing::info!(
                            "silent restore ok — patron {:?}, downloads {}/{}",
                            details.patron_id,
                            details.available_download.unwrap_or(0),
                            details.download_limit.unwrap_or(0)
                        );
                    }
                    Err(e) => {
                        tracing::warn!("silent restore failed: {e:#}");
                    }
                }
                AppEvent::Api(ApiEvent::Restored { result })
            }
            ApiCommand::DownloadSong { song, redownload } => {
                let result = claim_file(&mut client, &song, redownload).await;
                AppEvent::Api(ApiEvent::DownloadComplete { result })
            }
            ApiCommand::GetDownloadedIds => {
                let result = client.downloaded_song_ids().await;
                AppEvent::Api(ApiEvent::DownloadedIds { result })
            }
            ApiCommand::Login { card, pin } => {
                let result = match client.login_with(card, pin).await {
                    Ok(()) => client.user_details().await,
                    Err(e) => Err(e),
                };
                AppEvent::Api(ApiEvent::LoginResult { result })
            }
            ApiCommand::Search {
                q,
                kind,
                gen,
                offset,
                limit,
                fresh,
            } => {
                let result = client
                    .search(&q, kind, offset as u64, limit as u64, fresh)
                    .await;
                AppEvent::Api(ApiEvent::SearchResults { gen, kind, result })
            }
            ApiCommand::ArtistAlbums { artist_id, gen } => {
                let result = client.artist_albums(&artist_id, 0, 50).await;
                AppEvent::Api(ApiEvent::ArtistAlbums {
                    artist_id,
                    gen,
                    result,
                })
            }
            ApiCommand::AlbumSongs { album, gen } => {
                let result = client.album_songs(&album).await;
                AppEvent::Api(ApiEvent::AlbumSongs {
                    album_id: album.id,
                    gen,
                    result,
                })
            }
            ApiCommand::HomeFeeds { gen, fresh } => {
                let defs = crate::home::section_defs();
                let feeds: Vec<crate::home::HomeFeed> = defs.iter().map(|(_, f)| *f).collect();
                let last = feeds.len() - 1;
                let mut ret = AppEvent::Api(ApiEvent::NetIdle);
                for (i, feed) in feeds.iter().enumerate() {
                    let result = client
                        .home_feed(*feed, crate::home::FEED_LIMIT, fresh)
                        .await;
                    match &result {
                        Ok(_) => tracing::info!("api: home feed {feed:?} — ready"),
                        Err(e) => tracing::warn!("api: home feed {feed:?} failed: {e:#}"),
                    }
                    let event = AppEvent::Api(ApiEvent::HomeFeed {
                        feed: *feed,
                        gen,
                        result,
                        section: true,
                    });
                    if i == last {
                        ret = event;
                    } else {
                        let _ = events.send(event);
                    }
                }
                ret
            }
            ApiCommand::HomeFeed { feed, gen } => {
                tracing::info!("api: home feed drill {feed:?}");
                let result = client
                    .home_feed(feed, crate::home::DRILL_LIMIT, false)
                    .await;
                AppEvent::Api(ApiEvent::HomeFeed {
                    feed,
                    gen,
                    result,
                    section: false,
                })
            }
            ApiCommand::MyMusic { gen, fresh } => {
                let defs = crate::mymusic::music_sections();
                let feeds: Vec<crate::mymusic::MusicFeed> = defs.iter().map(|(_, f)| *f).collect();
                let last = feeds.len() - 1;
                let mut ret = AppEvent::Api(ApiEvent::NetIdle);
                for (i, feed) in feeds.iter().enumerate() {
                    let result = match feed {
                        crate::mymusic::MusicFeed::Playlists => client
                            .user_playlists(crate::mymusic::MUSIC_FEED_LIMIT, fresh)
                            .await
                            .map(crate::mymusic::MusicData::Playlists),
                        crate::mymusic::MusicFeed::Downloaded => client
                            .downloaded_songs(crate::mymusic::MUSIC_FEED_LIMIT, fresh)
                            .await
                            .map(crate::mymusic::MusicData::Songs),
                        crate::mymusic::MusicFeed::Favorites => client
                            .wishlist_songs("favorite", crate::mymusic::MUSIC_FEED_LIMIT, fresh)
                            .await
                            .map(crate::mymusic::MusicData::Songs),
                        crate::mymusic::MusicFeed::Wishlist => client
                            .wishlist_songs("wishlist", crate::mymusic::MUSIC_FEED_LIMIT, fresh)
                            .await
                            .map(crate::mymusic::MusicData::Songs),
                    };
                    match &result {
                        Ok(_) => tracing::info!("api: music feed {feed:?} — ready"),
                        Err(e) => tracing::warn!("api: music feed {feed:?} failed: {e:#}"),
                    }
                    let event = AppEvent::Api(ApiEvent::MyMusicFeed {
                        feed: *feed,
                        gen,
                        result,
                        section: true,
                    });
                    if i == last {
                        ret = event;
                    } else {
                        let _ = events.send(event);
                    }
                }
                ret
            }
            ApiCommand::MyMusicFeed { feed, gen } => {
                tracing::info!("api: music feed drill {feed:?}");
                let result = match feed {
                    crate::mymusic::MusicFeed::Playlists => client
                        .user_playlists(crate::mymusic::MUSIC_DRILL_LIMIT, false)
                        .await
                        .map(crate::mymusic::MusicData::Playlists),
                    crate::mymusic::MusicFeed::Downloaded => client
                        .downloaded_songs(crate::mymusic::MUSIC_DRILL_LIMIT, false)
                        .await
                        .map(crate::mymusic::MusicData::Songs),
                    crate::mymusic::MusicFeed::Favorites => client
                        .wishlist_songs("favorite", crate::mymusic::MUSIC_DRILL_LIMIT, false)
                        .await
                        .map(crate::mymusic::MusicData::Songs),
                    crate::mymusic::MusicFeed::Wishlist => client
                        .wishlist_songs("wishlist", crate::mymusic::MUSIC_DRILL_LIMIT, false)
                        .await
                        .map(crate::mymusic::MusicData::Songs),
                };
                AppEvent::Api(ApiEvent::MyMusicFeed {
                    feed,
                    gen,
                    result,
                    section: false,
                })
            }
            ApiCommand::Genres { gen, fresh } => {
                tracing::info!("api: genre list");
                let result = client.genres(fresh).await;
                AppEvent::Api(ApiEvent::Genres { gen, result })
            }
            ApiCommand::GenreDetail { genre, sub, gen } => {
                tracing::info!("api: genre detail {genre} sub {sub:?}");
                let songs = client
                    .genre_songs(&genre, sub.as_deref(), crate::home::GENRE_SONGS)
                    .await;
                let albums = client
                    .genre_albums(&genre, sub.as_deref(), crate::home::GENRE_ALBUMS)
                    .await;
                let playlists = client
                    .genre_playlists(&genre, sub.as_deref(), crate::home::GENRE_PLAYLISTS)
                    .await;
                let artists = client
                    .genre_artists(&genre, sub.as_deref(), crate::home::GENRE_ARTISTS)
                    .await;
                let subs = if sub.is_none() {
                    client.subgenres(&genre).await.unwrap_or_default()
                } else {
                    Default::default()
                };
                let songs = match songs {
                    Ok(page) => page,
                    Err(e) => {
                        tracing::warn!("api: genre detail songs failed: {e:#}");
                        Default::default()
                    }
                };
                let albums = match albums {
                    Ok(page) => page,
                    Err(e) => {
                        tracing::warn!("api: genre detail albums failed: {e:#}");
                        Default::default()
                    }
                };
                let playlists = match playlists {
                    Ok(page) => page,
                    Err(e) => {
                        tracing::warn!("api: genre detail playlists failed: {e:#}");
                        Default::default()
                    }
                };
                let artists = match artists {
                    Ok(page) => page,
                    Err(e) => {
                        tracing::warn!("api: genre detail artists failed: {e:#}");
                        Default::default()
                    }
                };
                let detail = Ok(Box::new(crate::home::GenreDetail {
                    genre: genre.clone(),
                    sub: sub.clone(),
                    songs,
                    albums,
                    playlists,
                    artists,
                    subgenres: subs,
                    state: crate::search::Load::Done,
                    table: ratatui::widgets::TableState::default(),
                }));
                tracing::info!("api: genre detail — ready (tolerant per-section)");
                AppEvent::Api(ApiEvent::GenreDetail {
                    genre,
                    sub,
                    gen,
                    result: detail,
                })
            }
            ApiCommand::PlaylistSongs { playlist_id, gen } => {
                tracing::info!("api: playlist songs — id {playlist_id}");
                let result = client.playlist_songs(playlist_id).await;
                AppEvent::Api(ApiEvent::PlaylistSongs {
                    playlist_id,
                    gen,
                    result,
                })
            }
            ApiCommand::StreamSong { song, intent } => {
                tracing::info!(
                    "api: stream ticket request — song {}, intent {intent:?}",
                    song.id
                );
                let result = client
                    .check_streaming_availability(song.id, song.provider.unwrap_or(1))
                    .await;
                match &result {
                    Ok(ticket) => tracing::info!(
                        "api: ticket ok — url[:70]={}",
                        ticket.stream_url.chars().take(70).collect::<String>()
                    ),
                    Err(e) => tracing::warn!("api: ticket failed: {e:#}"),
                }
                AppEvent::Api(ApiEvent::StreamReady {
                    song,
                    intent,
                    result,
                })
            }
            ApiCommand::RecordStreamTime {
                song_id,
                provider,
                secs,
            } => {
                let result = client.record_stream_time(song_id, provider, secs).await;
                AppEvent::Api(ApiEvent::StreamTimeRecorded { result })
            }
        };
        if events.send(event).is_err() {
            break;
        }
        let _ = events.send(AppEvent::Api(ApiEvent::NetIdle));
    }
}

pub async fn run_search_debounce(
    mut rx: tokio::sync::watch::Receiver<crate::search::SearchQuery>,
    cmd_tx: tokio::sync::mpsc::Sender<ApiCommand>,
    events: UnboundedSender<AppEvent>,
    sent_gen: crate::search::SentGen,
) {
    use crate::api::client::SearchKind;
    use crate::search::SEARCH_LIMIT;

    loop {
        if rx.changed().await.is_err() {
            return;
        }
        let settled = loop {
            let query = rx.borrow_and_update().clone();
            match tokio::time::timeout(
                std::time::Duration::from_millis(crate::search::SEARCH_SETTLE_MS),
                rx.changed(),
            )
            .await
            {
                Ok(Ok(())) => continue,
                Ok(Err(_)) => return,
                Err(_elapsed) => break query,
            }
        };
        if settled.q.is_empty() {
            continue;
        }
        if !crate::search::claim_dispatch(&sent_gen, settled.gen) {
            continue;
        }
        if events
            .send(AppEvent::Api(ApiEvent::SearchDispatched {
                gen: settled.gen,
            }))
            .is_err()
        {
            return;
        }
        for kind in [SearchKind::Song, SearchKind::Artist, SearchKind::Album] {
            let command = ApiCommand::Search {
                q: settled.q.clone(),
                kind,
                gen: settled.gen,
                offset: 0,
                limit: SEARCH_LIMIT,
                fresh: false,
            };
            if cmd_tx.send(command).await.is_err() {
                return;
            }
        }
    }
}

async fn silent_restore(
    client: &mut ApiClient,
) -> crate::api::ApiResult<crate::api::client::UserDetails> {
    client.ensure_bootstrap().await?;
    client.ensure_session().await?;
    client.user_details().await
}
