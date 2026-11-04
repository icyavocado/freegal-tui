use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::TableState;
use tui_input::backend::crossterm::EventHandler;
use tui_input::{Input, InputRequest};

use crate::api::client::{sanitize_subdomain, ContainerKind, Library, SearchKind, UserDetails};
use crate::api::creds::StoredCreds;
use crate::api::models::{Album, Page};
use crate::api::{ApiResult, StreamTime};
use crate::player::PlayerEvent;
use crate::search::{
    claim_dispatch, notify_search_input, AlbumContext, AlbumRequest, AppliedPage, ArtistContext,
    Focus, Load, SearchView, SEARCH_LIMIT,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Home,
    MyMusic,
    Queue,
    History,
    Downloads,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Self::Home,
        Self::MyMusic,
        Self::Queue,
        Self::History,
        Self::Downloads,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Home => 0,
            Self::MyMusic => 1,
            Self::Queue => 2,
            Self::History => 3,
            Self::Downloads => 4,
        }
    }

    pub const fn from_index(mut i: usize) -> Self {
        let last = Self::ALL.len() - 1;
        if i > last {
            i = last;
        }
        Self::ALL[i]
    }

    pub const fn next(self) -> Self {
        Self::from_index((self.index() + 1) % Self::ALL.len())
    }

    pub const fn previous(self) -> Self {
        Self::from_index((self.index() + Self::ALL.len() - 1) % Self::ALL.len())
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::MyMusic => "My Music",
            Self::Queue => "Queue",
            Self::History => "History",
            Self::Downloads => "Downloads",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WizardStep {
    #[default]
    Subdomain,
    LibraryConfirm,
    Card,
    Pin,
}

#[derive(Debug, Default)]
pub struct LibraryIdFallback {
    pub input: Input,
}

#[derive(Debug, Default)]
pub struct LoginWizard {
    pub step: WizardStep,
    pub subdomain_input: Input,
    pub card_input: Input,
    pub pin_input: Input,
    pub preview: Option<String>,
    pub library: Option<Library>,
    pub busy: bool,
    pub error: Option<String>,
    pub fallback: Option<LibraryIdFallback>,
}

impl LoginWizard {
    pub fn new(prefill: Option<String>, creds: Option<&StoredCreds>) -> Self {
        // A repeat visit to the wizard (e.g. creds on file but subdomain was
        // missing) must not demand the card/PIN again: prefill both from the
        // stored credentials so Enter walks straight through.
        let (card, pin) = creds
            .filter(|c| c.has_card_and_pin())
            .map(|c| (c.card_number.clone(), c.pin.clone()))
            .unwrap_or_default();
        Self {
            subdomain_input: Input::new(prefill.unwrap_or_default()),
            card_input: Input::new(card),
            pin_input: Input::new(pin),
            step: WizardStep::Subdomain,
            ..Self::default()
        }
    }

    fn refresh_preview(&mut self) {
        self.preview = sanitize_subdomain(self.subdomain_input.value())
            .map(|s| format!("→ {s}.freegalmusic.com"));
    }
}

#[derive(Debug)]
pub enum Screen {
    Login(Box<LoginWizard>),
    Home,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum ApiCommand {
    ResolveLibrary {
        subdomain: String,
    },
    SetLibraryId {
        id: i64,
    },
    Restore,
    Login {
        card: String,
        pin: String,
    },
    Search {
        q: String,
        kind: crate::api::client::SearchKind,
        gen: u64,
        offset: u32,
        limit: u32,
        fresh: bool,
    },
    ArtistAlbums {
        artist_id: String,
        gen: u64,
    },
    AlbumSongs {
        album: crate::api::models::Album,
        gen: u64,
    },
    StreamSong {
        song: crate::api::models::Song,
        intent: StreamIntent,
    },
    RecordStreamTime {
        song_id: u64,
        provider: u8,
        secs: f64,
    },
    DownloadSong {
        song: crate::api::models::Song,
        redownload: bool,
    },
    GetDownloadedIds,
    HomeFeeds {
        gen: u64,
        fresh: bool,
    },
    HomeFeed {
        feed: crate::home::HomeFeed,
        gen: u64,
    },
    MyMusic {
        gen: u64,
        fresh: bool,
    },
    MyMusicFeed {
        feed: crate::mymusic::MusicFeed,
        gen: u64,
    },
    Genres {
        gen: u64,
        fresh: bool,
    },
    GenreDetail {
        genre: String,
        sub: Option<String>,
        gen: u64,
    },
    PlaylistSongs {
        playlist_id: u64,
        gen: u64,
    },
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum AppEvent {
    Key(KeyEvent),
    Paste(String),
    Mouse(MouseEvent),
    Api(ApiEvent),
    Player(PlayerEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowArea {
    SearchRoot,
    SearchArtist,
    SearchAlbum,
    HomeSections,
    HomeCurated,
    HomeList,
    HomeAlbum,
    HomeAlbums,
    HomePlaylists,
    HomeArtists,
    HomeGenres,
    HomeGenreDetail,
    MusicRoot,
    MusicList,
    MusicPlaylists,
    Queue,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickZone {
    Tab(usize),
    SearchInput,
    HistoryInput,
    Progress,
    Rows(RowArea, bool),
    Chip(usize),
    JumpSearch,
    HomeCrumb(usize),
    MusicCrumb(usize),
}

pub const MOUSE_DOUBLE_MS: u128 = 400;

fn key_press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ApiEvent {
    LibraryResolved {
        result: ApiResult<Library>,
    },
    LibraryIdSet {
        result: ApiResult<()>,
    },
    Restored {
        result: ApiResult<UserDetails>,
    },
    LoginResult {
        result: ApiResult<UserDetails>,
    },
    SearchDispatched {
        gen: u64,
    },
    SearchResults {
        gen: u64,
        kind: crate::api::client::SearchKind,
        result: ApiResult<crate::api::client::ContainerKind>,
    },
    ArtistAlbums {
        artist_id: String,
        gen: u64,
        result: ApiResult<crate::api::models::Page<crate::api::models::Album>>,
    },
    AlbumSongs {
        album_id: u64,
        gen: u64,
        result: ApiResult<crate::api::models::Album>,
    },
    StreamReady {
        song: crate::api::models::Song,
        intent: StreamIntent,
        result: ApiResult<crate::api::client::StreamTicket>,
    },
    StreamTimeRecorded {
        result: ApiResult<Option<StreamTime>>,
    },
    NetBusy,
    NetIdle,
    DownloadComplete {
        result: ApiResult<DownloadOutcome>,
    },
    DownloadedIds {
        result: ApiResult<Vec<u64>>,
    },
    HomeFeed {
        feed: crate::home::HomeFeed,
        gen: u64,
        result: ApiResult<crate::home::HomeFeedData>,
        section: bool,
    },
    MyMusicFeed {
        feed: crate::mymusic::MusicFeed,
        gen: u64,
        result: ApiResult<crate::mymusic::MusicData>,
        section: bool,
    },
    Genres {
        gen: u64,
        result: ApiResult<crate::api::models::Page<crate::api::models::Genre>>,
    },
    GenreDetail {
        genre: String,
        sub: Option<String>,
        gen: u64,
        result: ApiResult<Box<crate::home::GenreDetail>>,
    },
    PlaylistSongs {
        playlist_id: u64,
        gen: u64,
        result: ApiResult<crate::api::models::Page<crate::api::models::Song>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamIntent {
    PlayNow,
    Append,
    PlayNext,
    Reload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub level: ToastLevel,
    pub text: String,
    pub expires: Instant,
}

pub const TOAST_TTL: Duration = Duration::from_secs(4);
const TICKET_TTL_SECS: i64 = 1800;
pub const TOAST_MAX: usize = 3;

#[derive(Debug, Default, Clone)]
pub struct Toasts {
    pub items: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, now: Instant, level: ToastLevel, text: impl Into<String>) {
        self.items.push(Toast {
            level,
            text: text.into(),
            expires: now + TOAST_TTL,
        });
        while self.items.len() > TOAST_MAX {
            self.items.remove(0);
        }
    }

    pub fn expire(&mut self, now: Instant) {
        self.items.retain(|t| t.expires > now);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

pub struct App {
    screen: Screen,
    tab: Tab,
    should_quit: bool,
    pub stored_subdomain: Option<String>,
    pub library_name: Option<String>,
    pub stream_time_remaining: Option<StreamTime>,
    pub downloads_used: Option<i64>,
    pub downloads_limit: Option<i64>,
    pub toasts: Toasts,
    api_tx: Option<tokio::sync::mpsc::Sender<ApiCommand>>,
    pub restoring: bool,
    pub search: crate::search::SearchTab,
    search_watch: Option<tokio::sync::watch::Sender<crate::search::SearchQuery>>,
    sent_gen: Option<crate::search::SentGen>,
    nav_gen: u64,
    pub queue: crate::queue::Queue,
    pub player_status: crate::player::PlayerStatus,
    pub player_degraded: Option<String>,
    player_started: bool,
    pub volume: u8,
    pub download_confirm: Option<crate::api::models::Song>,
    pub downloads: Vec<DownloadRow>,
    pub downloaded_ids: Vec<u64>,
    pub queue_table: ratatui::widgets::TableState,
    player_tx: Option<tokio::sync::mpsc::Sender<crate::player::PlayerCommand>>,
    pending_stream_secs: f64,
    last_seen_time: Option<f64>,
    pub help_open: bool,
    pub errors_open: bool,
    pub errors_scroll: usize,
    pub errors: Vec<ErrorEntry>,
    pub audio_menu_open: bool,
    pub audio_devices: Vec<(String, String)>,
    pub audio_menu_table: ratatui::widgets::TableState,
    pub history: Vec<crate::history::Entry>,
    pub history_path: std::path::PathBuf,
    pub history_input: Input,
    pub history_focus: crate::search::Focus,
    pub history_table: ratatui::widgets::TableState,
    pub now_unix: i64,
    pub clear_history_question: bool,
    pub net_active: bool,
    pub click_zones: Vec<(Rect, ClickZone)>,
    last_click: Option<(Instant, u16, u16)>,
    last_recorded: Option<(u64, u8)>,
    pub loop_one: bool,
    stream_cache: std::collections::HashMap<(u64, u8), (String, i64)>,
    pub home: crate::home::HomeTab,
    pub music: crate::mymusic::MusicTab,
}

impl Default for App {
    fn default() -> Self {
        Self::new(None)
    }
}

impl App {
    pub fn new(creds_on_file: Option<&StoredCreds>) -> Self {
        let stored = creds_on_file
            .map(|c| c.subdomain.clone())
            .filter(|s| !s.is_empty());
        let has_creds = creds_on_file
            .map(|c| c.has_card_and_pin() && !c.subdomain.is_empty())
            .unwrap_or(false);
        let wizard = LoginWizard::new(stored.clone(), creds_on_file);
        Self {
            screen: Screen::Login(Box::new(wizard)),
            tab: Tab::Home,
            should_quit: false,
            stored_subdomain: stored,
            library_name: creds_on_file.and_then(|c| c.library_name.clone()),
            stream_time_remaining: None,
            downloads_used: None,
            downloads_limit: None,
            toasts: Toasts::default(),
            api_tx: None,
            restoring: has_creds,
            search: crate::search::SearchTab::new(),
            search_watch: None,
            sent_gen: None,
            nav_gen: 0,
            queue: crate::queue::Queue::default(),
            player_status: crate::player::PlayerStatus::default(),
            player_degraded: None,
            player_started: false,
            volume: 80,
            download_confirm: None,
            help_open: false,
            errors_open: false,
            errors_scroll: 0,
            errors: Vec::new(),
            audio_menu_open: false,
            audio_devices: Vec::new(),
            audio_menu_table: ratatui::widgets::TableState::default(),
            history: Vec::new(),
            history_path: crate::history::history_path(),
            history_input: Input::default(),
            history_focus: crate::search::Focus::Results,
            history_table: ratatui::widgets::TableState::default(),
            now_unix: crate::history::now_unix(),
            clear_history_question: false,
            net_active: false,
            click_zones: Vec::new(),
            last_click: None,
            last_recorded: None,
            loop_one: false,
            stream_cache: std::collections::HashMap::new(),
            downloads: Vec::new(),
            downloaded_ids: Vec::new(),
            queue_table: ratatui::widgets::TableState::default(),
            player_tx: None,
            pending_stream_secs: 0.0,
            last_seen_time: None,
            home: crate::home::HomeTab::default(),
            music: crate::mymusic::MusicTab::default(),
        }
    }

    pub fn connect_player(&mut self, tx: tokio::sync::mpsc::Sender<crate::player::PlayerCommand>) {
        self.player_tx = Some(tx);
    }

    fn send_player(&mut self, command: crate::player::PlayerCommand, silent: bool) {
        let label = crate::player::cmd_label(&command);
        let Some(tx) = &self.player_tx else {
            tracing::warn!("player: no channel — dropped {label}");
            return;
        };
        if tx.try_send(command).is_err() {
            tracing::warn!("player: channel full — dropped {label}");
            if !silent {
                self.toast_error("player busy — command dropped");
            }
        } else if label != "GetStatus" {
            tracing::info!("player: sent {label}");
        }
    }

    pub fn connect_api(&mut self, tx: tokio::sync::mpsc::Sender<ApiCommand>) {
        self.api_tx = Some(tx);
    }

    pub fn connect_search_debounce(
        &mut self,
        tx: tokio::sync::watch::Sender<crate::search::SearchQuery>,
        sent_gen: crate::search::SentGen,
    ) {
        self.search_watch = Some(tx);
        self.sent_gen = Some(sent_gen);
    }

    fn send_api(&mut self, command: ApiCommand) {
        let label = format!("{command:?}");
        if let Some(tx) = &self.api_tx {
            if let Err(e) = tx.try_send(command) {
                tracing::warn!("api: dropped {label} ({e})");
                self.toast_error(format!("API busy — command dropped ({e})"));
            }
        }
    }

    pub fn update(&mut self, event: AppEvent) {
        match event {
            AppEvent::Key(key) => self.handle_key(key),
            AppEvent::Paste(text) => self.handle_paste(text),
            AppEvent::Mouse(me) => self.handle_mouse(me),
            AppEvent::Api(event) => self.handle_api(event),
            AppEvent::Player(event) => self.handle_player_event(event),
        }
    }

    pub fn tick(&mut self) {
        self.toasts.expire(Instant::now());
        self.now_unix = crate::history::now_unix();
        if self.player_started && self.player_degraded.is_none() {
            self.send_player(crate::player::PlayerCommand::GetStatus, true);
        }
        let playing = self.player_degraded.is_none()
            && self.player_status.paused == Some(false)
            && self.queue.current().is_some();
        let Some(t) = self.player_status.time.filter(|_| playing) else {
            self.last_seen_time = None;
            return;
        };
        if let Some(prev) = self.last_seen_time {
            let delta = t - prev;
            if delta > 0.0 && delta < 5.0 {
                self.pending_stream_secs += delta;
            }
        }
        self.last_seen_time = Some(t);
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    #[cfg(test)]
    pub fn make_home(&mut self) {
        self.screen = Screen::Home;
    }

    #[cfg(test)]
    pub(crate) fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    fn apply_chars(input: &mut Input, text: &str) {
        for c in text.chars() {
            input.handle(InputRequest::InsertChar(c));
        }
    }

    fn handle_paste(&mut self, text: String) {
        let sanitized: String = text.lines().collect::<Vec<_>>().join(" ");
        let sanitized: String = sanitized
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        if sanitized.trim().is_empty() {
            return;
        }
        let Screen::Login(w) = &mut self.screen else {
            return;
        };
        if w.busy {
            return;
        }
        if let Some(fallback) = w.fallback.as_mut() {
            Self::apply_chars(&mut fallback.input, &sanitized);
            return;
        }
        match w.step {
            WizardStep::Subdomain => {
                w.subdomain_input.reset();
                Self::apply_chars(&mut w.subdomain_input, &sanitized);
                w.refresh_preview();
            }
            WizardStep::Card => Self::apply_chars(&mut w.card_input, &sanitized),
            WizardStep::Pin => Self::apply_chars(&mut w.pin_input, &sanitized),
            WizardStep::LibraryConfirm => {}
        }
    }

    fn route_to_input(key: KeyEvent, input: &mut Input) -> bool {
        input
            .handle_event(&crossterm::event::Event::Key(key))
            .is_some()
    }

    fn handle_mouse(&mut self, me: MouseEvent) {
        let overlay = self.help_open
            || self.errors_open
            || self.audio_menu_open
            || self.download_confirm.is_some()
            || self.clear_history_question;
        if overlay {
            return;
        }
        match me.kind {
            MouseEventKind::ScrollDown => self.handle_key(key_press(KeyCode::Down)),
            MouseEventKind::ScrollUp => self.handle_key(key_press(KeyCode::Up)),
            MouseEventKind::Down(MouseButton::Left) => self.mouse_click(me.row, me.column),
            _ => {}
        }
    }

    fn mouse_click(&mut self, row: u16, col: u16) {
        let now = Instant::now();
        let double = self.last_click.take().is_some_and(|(at, r, c)| {
            now.duration_since(at).as_millis() <= MOUSE_DOUBLE_MS && r == row && c == col
        });
        self.last_click = Some((now, row, col));
        let Some((rect, zone)) = self
            .click_zones
            .iter()
            .copied()
            .find(|(rect, _)| rect.contains(Position { x: col, y: row }))
        else {
            return;
        };
        match zone {
            ClickZone::Tab(i) => {
                let tab = Tab::ALL[i];
                if self.tab != tab {
                    self.tab = tab;
                    if tab == Tab::Downloads {
                        self.send_api(ApiCommand::GetDownloadedIds);
                    }
                    if tab == Tab::Home {
                        self.ensure_home_fetch();
                    }
                    if tab == Tab::MyMusic {
                        self.ensure_music_fetch();
                    }
                }
            }
            ClickZone::SearchInput => {
                if let Some(crate::search::SearchView::Root(h)) = self.search.top_mut() {
                    h.focus = crate::search::Focus::Input;
                }
            }
            ClickZone::JumpSearch => {
                self.tab = crate::app::Tab::Home;
                if let Some(crate::search::SearchView::Root(h)) = self.search.top_mut() {
                    h.focus = crate::search::Focus::Input;
                }
                self.ensure_home_fetch();
            }
            ClickZone::Chip(idx) => {
                self.home.link_sel = idx;
                self.home.focus = crate::home::HomeFocus::Links;
                if double {
                    if let Some(link) = self.home.selected_link() {
                        self.open_quick_link(link);
                    }
                }
            }
            ClickZone::HomeCrumb(k) => self.home.pop_to_crumb(k),
            ClickZone::MusicCrumb(k) => {
                if k == 0 {
                    self.music.reset();
                }
            }
            ClickZone::HistoryInput => self.history_focus = crate::search::Focus::Input,
            ClickZone::Progress => {
                let Some(duration) = self.player_status.duration.filter(|d| *d > 0.0) else {
                    return;
                };
                let frac = f64::from(col.saturating_sub(rect.x) + 1) / f64::from(rect.width);
                let secs = (frac * duration).clamp(0.0, duration);
                self.send_player(crate::player::PlayerCommand::SeekTo { secs }, false);
            }
            ClickZone::Rows(area, header) => {
                if self.mouse_select_row(area, rect, header, row) && double {
                    self.handle_key(key_press(KeyCode::Enter));
                }
            }
        }
    }

    fn mouse_select_row(&mut self, area: RowArea, rect: Rect, header: bool, row: u16) -> bool {
        let pad = u16::from(header);
        match area {
            RowArea::SearchRoot => {
                let Some(crate::search::SearchView::Root(h)) = self.search.top_mut() else {
                    return false;
                };
                h.focus = crate::search::Focus::Results;
                let idx = h
                    .table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                let len = h.rows().len();
                if idx >= len {
                    return false;
                }
                h.table.select(Some(idx));
                true
            }
            RowArea::SearchArtist => {
                let Some(crate::search::SearchView::Artist(ctx)) = self.search.top_mut() else {
                    return false;
                };
                let idx = ctx
                    .table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= ctx.albums.items.len() {
                    return false;
                }
                ctx.table.select(Some(idx));
                true
            }
            RowArea::SearchAlbum | RowArea::HomeAlbum => {
                let Some(ctx) = self.album_ctx_mut(area) else {
                    return false;
                };
                let idx = ctx
                    .table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= ctx.album.songs.len() {
                    return false;
                }
                ctx.table.select(Some(idx));
                true
            }
            RowArea::HomeList => {
                let crate::home::HomeView::List { songs, table, .. } = &mut self.home.view else {
                    return false;
                };
                let idx = table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= songs.items.len() {
                    return false;
                }
                table.select(Some(idx));
                true
            }
            RowArea::HomeAlbums => {
                let crate::home::HomeView::Albums { albums, table, .. } = &mut self.home.view
                else {
                    return false;
                };
                let idx = table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= albums.items.len() {
                    return false;
                }
                table.select(Some(idx));
                true
            }
            RowArea::HomePlaylists => {
                let crate::home::HomeView::Playlists {
                    playlists, table, ..
                } = &mut self.home.view
                else {
                    return false;
                };
                let idx = table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= playlists.items.len() {
                    return false;
                }
                table.select(Some(idx));
                true
            }
            RowArea::HomeArtists => {
                let crate::home::HomeView::Artists { artists, table, .. } = &mut self.home.view
                else {
                    return false;
                };
                let idx = table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= artists.items.len() {
                    return false;
                }
                table.select(Some(idx));
                true
            }
            RowArea::HomeSections => {
                let idx = self
                    .home
                    .sections_table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= self.home.home_rows().len() {
                    return false;
                }
                self.home.sections_table.select(Some(idx));
                true
            }
            RowArea::HomeCurated => {
                let idx = self
                    .home
                    .curated_table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= self.home.curated_len() {
                    return false;
                }
                self.home.curated_table.select(Some(idx));
                true
            }
            RowArea::HomeGenres => {
                let crate::home::HomeView::Genres { page, table, .. } = &mut self.home.view else {
                    return false;
                };
                let idx = table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= page.items.len() {
                    return false;
                }
                table.select(Some(idx));
                true
            }
            RowArea::HomeGenreDetail => {
                let len = match &self.home.view {
                    crate::home::HomeView::Detail(d) => crate::home::detail_rows_len(d),
                    _ => return false,
                };
                let crate::home::HomeView::Detail(d) = &mut self.home.view else {
                    return false;
                };
                let idx = d
                    .table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= len {
                    return false;
                }
                d.table.select(Some(idx));
                true
            }
            RowArea::MusicRoot => {
                let idx = self
                    .music
                    .root_table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= crate::mymusic::music_rows(&self.music).len() {
                    return false;
                }
                self.music.root_table.select(Some(idx));
                true
            }
            RowArea::MusicList => {
                let crate::mymusic::MusicView::Songs { songs, table, .. } = &mut self.music.view
                else {
                    return false;
                };
                let idx = table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= songs.items.len() {
                    return false;
                }
                table.select(Some(idx));
                true
            }
            RowArea::MusicPlaylists => {
                let crate::mymusic::MusicView::Playlists { page, table, .. } = &mut self.music.view
                else {
                    return false;
                };
                let idx = table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= page.items.len() {
                    return false;
                }
                table.select(Some(idx));
                true
            }
            RowArea::Queue => {
                let idx = self
                    .queue_table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= self.queue.len() {
                    return false;
                }
                self.queue_table.select(Some(idx));
                true
            }
            RowArea::History => {
                let idx = self
                    .history_table
                    .offset()
                    .saturating_add(row.saturating_sub(rect.y).saturating_sub(pad) as usize);
                if idx >= self.history_filtered().len() {
                    return false;
                }
                self.history_table.select(Some(idx));
                true
            }
        }
    }

    fn album_ctx_mut(&mut self, area: RowArea) -> Option<&mut crate::search::AlbumContext> {
        match area {
            RowArea::SearchAlbum => match self.search.top_mut() {
                Some(crate::search::SearchView::Album(ctx)) => Some(ctx),
                _ => None,
            },
            RowArea::HomeAlbum => match &mut self.home.view {
                crate::home::HomeView::Album(ctx) => Some(ctx),
                _ => None,
            },
            _ => None,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q')) {
            self.should_quit = true;
            return;
        }
        if self.help_open {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.help_open = false;
            }
            return;
        }
        if self.errors_open {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char('q') => {
                    self.errors_open = false;
                    self.errors_scroll = 0;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.errors_scroll = self.errors_scroll.saturating_add(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.errors_scroll = self.errors_scroll.saturating_sub(1);
                }
                KeyCode::Char('g') => self.errors_scroll = usize::MAX,
                KeyCode::Char('G') => self.errors_scroll = 0,
                _ => {}
            }
            return;
        }
        if self.audio_menu_open {
            self.handle_audio_menu_key(key);
            return;
        }
        if self.download_confirm.is_some() {
            self.handle_download_confirm_key(key);
            return;
        }
        if self.clear_history_question {
            self.handle_clear_history_key(key);
            return;
        }
        match &mut self.screen {
            Screen::Login(_) => self.handle_login_key(key),
            Screen::Home => self.handle_home_key(key),
        }
    }

    fn handle_login_key(&mut self, key: KeyEvent) {
        let Screen::Login(w) = &mut self.screen else {
            return;
        };
        if let Some(fallback) = w.fallback.as_mut() {
            match key.code {
                KeyCode::Esc | KeyCode::Tab => {
                    w.fallback = None;
                }
                KeyCode::Enter => {
                    let text = fallback.input.value().trim().to_string();
                    match text.parse::<i64>() {
                        Ok(id) => {
                            w.busy = true;
                            self.send_api(ApiCommand::SetLibraryId { id });
                        }
                        Err(_) => w.error = Some("library id must be a number".into()),
                    }
                }
                KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete => {
                    let _ = fallback
                        .input
                        .handle_event(&crossterm::event::Event::Key(key));
                }
                _ => {}
            }
            return;
        }
        if w.busy {
            return;
        }
        let editing_step = matches!(
            w.step,
            WizardStep::Subdomain | WizardStep::Card | WizardStep::Pin
        );
        if editing_step {
            let consumed = match w.step {
                WizardStep::Subdomain => Self::route_to_input(key, &mut w.subdomain_input),
                WizardStep::Card => Self::route_to_input(key, &mut w.card_input),
                WizardStep::Pin => Self::route_to_input(key, &mut w.pin_input),
                _ => false,
            };
            if consumed {
                if w.step == WizardStep::Subdomain {
                    w.refresh_preview();
                }
                return;
            }
        }
        match w.step {
            WizardStep::Subdomain => match key.code {
                KeyCode::Enter => {
                    let value = w.subdomain_input.value();
                    match sanitize_subdomain(value) {
                        None => w.error = Some("enter a subdomain like demo, or a full URL".into()),
                        Some(sub) => {
                            w.subdomain_input = Input::new(sub.clone());
                            w.refresh_preview();
                            w.busy = true;
                            w.error = None;
                            self.send_api(ApiCommand::ResolveLibrary { subdomain: sub });
                        }
                    }
                }
                KeyCode::Esc => self.should_quit = true,
                _ => {}
            },
            WizardStep::LibraryConfirm => match key.code {
                KeyCode::Enter => {
                    w.step = WizardStep::Card;
                    w.error = None;
                }
                KeyCode::Esc => {
                    w.step = WizardStep::Subdomain;
                    w.library = None;
                }
                _ => {}
            },
            WizardStep::Card => match key.code {
                KeyCode::Enter => {
                    if w.card_input.value().trim().is_empty() {
                        w.error = Some("enter your library card number".into());
                    } else {
                        w.step = WizardStep::Pin;
                        w.error = None;
                    }
                }
                KeyCode::Esc => w.step = WizardStep::LibraryConfirm,
                _ => {}
            },
            WizardStep::Pin => {
                let card = w.card_input.value().trim().to_string();
                let pin = w.pin_input.value().trim().to_string();
                match key.code {
                    KeyCode::Enter => {
                        if pin.is_empty() {
                            w.error = Some("enter your PIN".into());
                        } else {
                            w.busy = true;
                            w.error = None;
                            self.send_api(ApiCommand::Login { card, pin });
                        }
                    }
                    KeyCode::Esc => w.step = WizardStep::Card,
                    _ => {}
                }
            }
        }
    }

    fn handle_home_key(&mut self, key: KeyEvent) {
        let typing = self.is_typing();
        match key.code {
            KeyCode::Tab => {
                self.tab = self.tab.next();
                self.ensure_tab_fetch();
            }
            KeyCode::BackTab => {
                self.tab = self.tab.previous();
                self.ensure_tab_fetch();
            }
            KeyCode::Char(c @ '1'..='5') if !typing => {
                self.tab = Tab::ALL[(c as u8 - b'1') as usize];
                match self.tab {
                    Tab::Downloads => self.send_api(ApiCommand::GetDownloadedIds),
                    Tab::Home => self.ensure_home_fetch(),
                    Tab::MyMusic => self.ensure_music_fetch(),
                    _ => {}
                }
            }
            KeyCode::Char(' ' | 'l' | 'n' | 'p' | ',' | '.' | '-' | '+' | '=') if !typing => {
                self.handle_player_keys(key);
            }
            KeyCode::Char('/') if !typing => {
                self.tab = match self.tab {
                    Tab::History => Tab::History,
                    _ => Tab::Home,
                };
                if self.tab == Tab::Home {
                    if let Some(SearchView::Root(h)) = self.search.top_mut() {
                        h.focus = Focus::Input;
                    }
                    self.ensure_home_fetch();
                } else {
                    self.history_focus = Focus::Input;
                }
            }
            KeyCode::Char(c @ ('s' | 'm' | 'q' | 'd' | 'h')) if !typing => {
                self.tab = match c {
                    's' => Tab::Home,
                    'm' => Tab::MyMusic,
                    'q' => Tab::Queue,
                    'h' => Tab::History,
                    _ => Tab::Downloads,
                };
                if c == 's' {
                    self.ensure_home_fetch();
                    if let Some(SearchView::Root(h)) = self.search.top_mut() {
                        h.focus = Focus::Input;
                    }
                } else if self.tab == Tab::Downloads {
                    self.send_api(ApiCommand::GetDownloadedIds);
                } else if self.tab == Tab::MyMusic {
                    self.ensure_music_fetch();
                }
            }
            _ if self.tab == Tab::Home => {
                if !matches!(self.search.top(), Some(SearchView::Root(_))) {
                    self.handle_search_key(key);
                } else if let Some(SearchView::Root(h)) = self.search.top() {
                    if h.focus == Focus::Input || !h.input.value().trim().is_empty() {
                        self.handle_search_root(key);
                    } else {
                        self.handle_home_content(key);
                    }
                } else {
                    self.handle_home_content(key);
                }
            }
            _ if self.tab == Tab::History => self.handle_history_key(key),
            _ if self.tab == Tab::Queue => self.handle_queue_key(key),
            _ if self.tab == Tab::MyMusic => self.handle_music_key(key),
            _ => self.handle_player_keys(key),
        }
    }

    /// Letters and editing keys go into the focused text box instead of
    /// switching tabs, both on the search box and the history filter.
    fn is_typing(&self) -> bool {
        match self.tab {
            Tab::Home => matches!(
                self.search.top(),
                Some(SearchView::Root(h)) if h.focus == Focus::Input
            ),
            Tab::History => self.history_focus == Focus::Input,
            _ => false,
        }
    }

    fn handle_home_content(&mut self, key: KeyEvent) {
        if self.home.is_root() {
            self.handle_home_root(key);
        } else if matches!(self.home.view, crate::home::HomeView::Album(_)) {
            self.handle_home_album(key);
        } else {
            self.handle_home_drill(key);
        }
    }

    fn handle_home_root(&mut self, key: KeyEvent) {
        use crate::home::{HomeFocus, NavDir};
        if self.home.focus == HomeFocus::Links {
            match key.code {
                KeyCode::Left => self.home.nav_links(-1),
                KeyCode::Right => self.home.nav_links(1),
                KeyCode::Char('g') => self.home.link_sel = 0,
                KeyCode::Char('G') => {
                    self.home.link_sel = crate::home::quick_links().len().saturating_sub(1);
                }
                KeyCode::Up | KeyCode::Down | KeyCode::Char('k') | KeyCode::Char('j') => {
                    self.home.focus = HomeFocus::Sections;
                    self.home.sections_table.select(Some(0));
                }
                KeyCode::Enter => {
                    if let Some(link) = self.home.selected_link() {
                        self.open_quick_link(link);
                    }
                }
                KeyCode::Char('R') => self.refetch_home(true),
                _ => self.handle_player_keys(key),
            }
            return;
        }
        match key.code {
            KeyCode::Left | KeyCode::Right
                if self.home.last_width >= crate::home::SIDEBAR_MIN_WIDTH =>
            {
                self.home.focus = match self.home.focus {
                    HomeFocus::Sections => HomeFocus::Curated,
                    HomeFocus::Curated => HomeFocus::Sections,
                    HomeFocus::Links => HomeFocus::Sections,
                };
            }
            KeyCode::Up | KeyCode::Char('k') => self.nav_home(NavDir::Up),
            KeyCode::Down | KeyCode::Char('j') => self.nav_home(NavDir::Down),
            KeyCode::Char('g') => self.home_edge(NavDir::Up),
            KeyCode::Char('G') => self.home_edge(NavDir::Down),
            KeyCode::Enter => self.activate_home(),
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.queue_home_song(StreamIntent::Append);
            }
            KeyCode::Char('y') => self.download_home_song(),
            KeyCode::Char('R') => self.refetch_home(true),
            _ => self.handle_player_keys(key),
        }
    }

    fn nav_home(&mut self, dir: crate::home::NavDir) {
        match self.home.focus {
            crate::home::HomeFocus::Curated => self.home.nav_curated(dir),
            crate::home::HomeFocus::Links => self.home.nav_links(match dir {
                crate::home::NavDir::Up => -1,
                crate::home::NavDir::Down => 1,
            }),
            crate::home::HomeFocus::Sections => {
                if dir == crate::home::NavDir::Up
                    && self.home.last_width < crate::home::SIDEBAR_MIN_WIDTH
                    && matches!(self.home.sections_table.selected(), None | Some(0))
                {
                    self.home.focus = crate::home::HomeFocus::Links;
                    return;
                }
                self.home.nav_sections(dir);
            }
        }
    }

    fn home_edge(&mut self, dir: crate::home::NavDir) {
        use crate::home::{HomeFocus, NavDir};
        match self.home.focus {
            HomeFocus::Links => {
                self.home.link_sel = if dir == NavDir::Up {
                    0
                } else {
                    crate::home::quick_links().len() - 1
                };
            }
            HomeFocus::Curated => {
                let len = self.home.curated_len();
                let idx = match dir {
                    NavDir::Up => 0,
                    NavDir::Down => len.saturating_sub(1),
                };
                if len > 0 {
                    self.home.curated_table.select(Some(idx));
                }
            }
            HomeFocus::Sections => {
                let rows = self.home.home_rows().len();
                if rows > 0 {
                    self.home.sections_table.select(Some(rows - 1));
                }
            }
        }
    }

    fn activate_home(&mut self) {
        use crate::home::{DrillKind, HomeFocus, HomeRow};
        if self.home.focus == HomeFocus::Curated {
            if let Some(p) = self.home.selected_playlist() {
                self.open_home_list(DrillKind::Playlist(p.id, p.name.clone()));
            }
            return;
        }
        match self.home.selected_row() {
            Some(HomeRow::Head(feed, _)) => self.open_feed_drill(feed),
            Some(HomeRow::Song(s)) => {
                self.stream_song(s.clone(), StreamIntent::PlayNow);
            }
            Some(HomeRow::Album(a)) => {
                self.issue_album_songs(a.clone(), AlbumDrillSlot::Home);
            }
            Some(HomeRow::Playlist(p)) => {
                self.open_home_list(DrillKind::Playlist(p.id, p.name.clone()));
            }
            Some(HomeRow::Artist(a)) => self.open_artist_drill(a.id.clone(), a.name.clone()),
            _ => {}
        }
    }

    fn open_quick_link(&mut self, link: crate::home::QuickLink) {
        match link {
            crate::home::QuickLink::Genres => self.open_genres_list(),
            crate::home::QuickLink::Feed(feed) => self.open_feed_drill(feed),
        }
    }

    pub(crate) fn open_feed_drill(&mut self, feed: crate::home::HomeFeed) {
        self.nav_gen += 1;
        let gen = self.nav_gen;
        self.home
            .enter_level(crate::home::feed_label(feed).to_string());
        self.home.pending = Some(crate::home::PendingDrill {
            kind: crate::home::DrillKind::Feed(feed),
            gen,
        });
        self.home.view = crate::home::HomeView::Loading {
            state: Load::Loading,
        };
        self.send_api(ApiCommand::HomeFeed { feed, gen });
    }

    pub(crate) fn open_artist_drill(&mut self, artist_id: String, name: String) {
        self.nav_gen += 1;
        let gen = self.nav_gen;
        self.home.enter_level(name);
        self.home.pending = Some(crate::home::PendingDrill {
            kind: crate::home::DrillKind::Artist(artist_id.clone()),
            gen,
        });
        self.home.view = crate::home::HomeView::Loading {
            state: Load::Loading,
        };
        self.send_api(ApiCommand::ArtistAlbums { artist_id, gen });
    }

    fn fill_home_feed_drill(&mut self, result: ApiResult<crate::home::HomeFeedData>) {
        match result {
            Ok(ref data) => {
                let mut table = ratatui::widgets::TableState::default();
                let count = match data {
                    crate::home::HomeFeedData::Songs(p) => p.items.len(),
                    crate::home::HomeFeedData::Albums(p) => p.items.len(),
                    crate::home::HomeFeedData::Playlists(p) => p.items.len(),
                    crate::home::HomeFeedData::Artists(p) => p.items.len(),
                    crate::home::HomeFeedData::None => 0,
                };
                if count > 0 {
                    table.select(Some(0));
                }
                self.home.view = match data {
                    crate::home::HomeFeedData::Songs(songs) => crate::home::HomeView::List {
                        songs: songs.clone(),
                        state: Load::Done,
                        table,
                    },
                    crate::home::HomeFeedData::Albums(albums) => crate::home::HomeView::Albums {
                        albums: albums.clone(),
                        state: Load::Done,
                        table,
                    },
                    crate::home::HomeFeedData::Playlists(playlists) => {
                        crate::home::HomeView::Playlists {
                            playlists: playlists.clone(),
                            state: Load::Done,
                            table,
                        }
                    }
                    crate::home::HomeFeedData::Artists(artists) => crate::home::HomeView::Artists {
                        artists: artists.clone(),
                        state: Load::Done,
                        table,
                    },
                    crate::home::HomeFeedData::None => crate::home::HomeView::Root,
                };
            }
            Err(e) => {
                self.toast_error(format!("home: {e}"));
                self.home.pop_level();
            }
        }
    }

    pub(crate) fn open_home_list(&mut self, kind: crate::home::DrillKind) {
        self.nav_gen += 1;
        let gen = self.nav_gen;
        assert!(matches!(kind, crate::home::DrillKind::Playlist(_, _)));
        let name = match &kind {
            crate::home::DrillKind::Playlist(_, name) => name.clone(),
            _ => unreachable!("open_home_list only opens playlist drills"),
        };
        self.home.enter_level(name);
        let command = match kind {
            crate::home::DrillKind::Playlist(id, _) => ApiCommand::PlaylistSongs {
                playlist_id: id,
                gen,
            },
            _ => unreachable!("open_home_list only opens playlist drills"),
        };
        self.home.pending = Some(crate::home::PendingDrill { kind, gen });
        self.home.view = crate::home::HomeView::List {
            songs: crate::api::models::Page::default(),
            state: Load::Loading,
            table: ratatui::widgets::TableState::default(),
        };
        self.send_api(command);
    }

    fn fill_music_drill(&mut self, result: ApiResult<crate::mymusic::MusicData>) {
        match result {
            Ok(crate::mymusic::MusicData::Playlists(page)) => {
                let mut table = ratatui::widgets::TableState::default();
                if !page.items.is_empty() {
                    table.select(Some(0));
                }
                self.music.view = crate::mymusic::MusicView::Playlists {
                    page,
                    state: Load::Done,
                    table,
                };
            }
            Ok(crate::mymusic::MusicData::Songs(songs)) => {
                let mut table = ratatui::widgets::TableState::default();
                if !songs.items.is_empty() {
                    table.select(Some(0));
                }
                self.music.view = crate::mymusic::MusicView::Songs {
                    songs,
                    state: Load::Done,
                    table,
                };
            }
            Ok(crate::mymusic::MusicData::None) => {
                self.music.crumb = None;
                self.music.view = crate::mymusic::MusicView::Root;
            }
            Err(e) => {
                self.toast_error(format!("music: {e}"));
                self.music.crumb = None;
                self.music.view = crate::mymusic::MusicView::Root;
            }
        }
    }

    fn queue_home_song(&mut self, intent: StreamIntent) {
        if let Some(crate::home::HomeRow::Song(s)) = self.home.selected_row() {
            self.stream_song(s.clone(), intent);
        }
    }

    fn download_home_song(&mut self) {
        if let Some(crate::home::HomeRow::Song(s)) = self.home.selected_row() {
            self.download_confirm = Some(s.clone());
        }
    }

    fn play_now_enqueue(
        &mut self,
        song: crate::api::models::Song,
        rest: Vec<crate::api::models::Song>,
    ) {
        self.queue.play_now(song.clone());
        self.queue.enqueue_rest(rest);
        self.stream_song(song, StreamIntent::PlayNow);
    }

    fn stream_song(&mut self, song: crate::api::models::Song, intent: StreamIntent) {
        let key = (song.id, song.provider.unwrap_or(1));
        if let Some((url, at)) = self.stream_cache.get(&key).cloned() {
            if self.now_unix.saturating_sub(at) < TICKET_TTL_SECS {
                tracing::info!(
                    "stream cache hit — song {}, url[:48]={}",
                    key.0,
                    url.chars().take(48).collect::<String>()
                );
                self.apply_stream_ticket(&song, intent, &url);
                return;
            }
            self.stream_cache.remove(&key);
        }
        self.send_api(ApiCommand::StreamSong { song, intent });
    }

    fn apply_stream_ticket(
        &mut self,
        song: &crate::api::models::Song,
        intent: StreamIntent,
        stream_url: &str,
    ) {
        let title = player_title(song);
        tracing::info!(
            "stream ready — intent {intent:?}, url[:70]={}",
            stream_url.chars().take(70).collect::<String>()
        );
        match intent {
            StreamIntent::PlayNow => {
                let already_current = self.queue.current().is_some_and(|(_, item)| {
                    item.song.id == song.id && item.song.provider == song.provider
                });
                if !already_current {
                    self.queue.play_now(song.clone());
                }
                self.flush_stream_time();
                self.pending_stream_secs = 0.0;
                self.last_seen_time = None;
                self.send_player(
                    crate::player::PlayerCommand::Load {
                        url: stream_url.to_string(),
                        title,
                    },
                    false,
                );
                self.record_history_from(song, stream_url);
                self.toasts.push(
                    Instant::now(),
                    ToastLevel::Success,
                    format!("playing {}", song.title),
                );
            }
            StreamIntent::Append => {
                self.queue.append(song.clone());
                if self.queue.current().is_none() {
                    let last = self.queue.items.len() - 1;
                    self.queue.play_at(last);
                    self.send_player(
                        crate::player::PlayerCommand::Load {
                            url: stream_url.to_string(),
                            title,
                        },
                        false,
                    );
                    self.record_history_from(song, stream_url);
                }
                self.toasts.push(
                    Instant::now(),
                    ToastLevel::Info,
                    format!("appended {}", song.title),
                );
            }
            StreamIntent::PlayNext => {
                self.queue.play_next(song.clone());
                if self.queue.current().is_none() {
                    self.queue.play_at(0);
                    self.send_player(
                        crate::player::PlayerCommand::Load {
                            url: stream_url.to_string(),
                            title,
                        },
                        false,
                    );
                    self.record_history_from(song, stream_url);
                } else {
                    self.toasts.push(
                        Instant::now(),
                        ToastLevel::Info,
                        format!("queued next {}", song.title),
                    );
                }
            }
            StreamIntent::Reload => {
                self.send_player(
                    crate::player::PlayerCommand::Load {
                        url: stream_url.to_string(),
                        title,
                    },
                    false,
                );
                self.record_history_from(song, stream_url);
            }
        }
    }

    fn handle_home_album(&mut self, key: KeyEvent) {
        let action = {
            let crate::home::HomeView::Album(ctx) = &mut self.home.view else {
                return;
            };
            match key.code {
                KeyCode::Esc | KeyCode::Backspace => Some(AlbumAction::Pop),
                KeyCode::Up | KeyCode::Char('k') => {
                    ctx.table.select_previous();
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    ctx.table.select_next();
                    None
                }
                KeyCode::Char('g') => {
                    ctx.table.select_first();
                    None
                }
                KeyCode::Char('G') => {
                    ctx.table.select_last();
                    None
                }
                KeyCode::Enter | KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Char('y') => {
                    let selected = ctx.table.selected().unwrap_or(0);
                    ctx.album.songs.get(selected).map(|s| match key.code {
                        KeyCode::Char('y') => AlbumAction::Download(s.clone()),
                        KeyCode::Char('a') | KeyCode::Char('A') => {
                            AlbumAction::Play(s.clone(), StreamIntent::Append)
                        }
                        _ => AlbumAction::PlayAlbum {
                            song: s.clone(),
                            rest: ctx.album.songs.get(selected + 1..).unwrap_or(&[]).to_vec(),
                        },
                    })
                }
                _ => None,
            }
        };
        match action {
            Some(AlbumAction::Pop) => self.home.pop_level(),
            Some(AlbumAction::Play(song, intent)) => self.stream_song(song, intent),
            Some(AlbumAction::PlayAlbum { song, rest }) => self.play_now_enqueue(song, rest),
            Some(AlbumAction::Download(song)) => self.download_confirm = Some(song),
            _ => {}
        }
    }

    fn handle_home_drill(&mut self, key: KeyEvent) {
        use crate::home::{DrillKind, HomeView};
        let action =
            {
                let nav = |table: &mut ratatui::widgets::TableState, code: &KeyCode| match code {
                    KeyCode::Up | KeyCode::Char('k') => table.select_previous(),
                    KeyCode::Down | KeyCode::Char('j') => table.select_next(),
                    KeyCode::Char('g') => table.select_first(),
                    KeyCode::Char('G') => table.select_last(),
                    _ => {}
                };
                match &mut self.home.view {
                    HomeView::Loading { .. } => match key.code {
                        KeyCode::Esc | KeyCode::Backspace => Some(HomeDrillAction::Pop),
                        _ => None,
                    },
                    HomeView::List { songs, table, .. } => match key.code {
                        KeyCode::Esc | KeyCode::Backspace => Some(HomeDrillAction::Pop),
                        KeyCode::Up | KeyCode::Char('k') => {
                            table.select_previous();
                            None
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            table.select_next();
                            None
                        }
                        KeyCode::Char('g') => {
                            table.select_first();
                            None
                        }
                        KeyCode::Char('G') => {
                            table.select_last();
                            None
                        }
                        KeyCode::Enter
                        | KeyCode::Char('a')
                        | KeyCode::Char('A')
                        | KeyCode::Char('y') => {
                            let selected = table.selected().unwrap_or(0);
                            songs.items.get(selected).map(|s| match key.code {
                                KeyCode::Char('y') => HomeDrillAction::Download(s.clone()),
                                KeyCode::Char('a') | KeyCode::Char('A') => {
                                    HomeDrillAction::Play(s.clone(), StreamIntent::Append)
                                }
                                _ => HomeDrillAction::PlayAlbum {
                                    song: s.clone(),
                                    rest: songs.items.get(selected + 1..).unwrap_or(&[]).to_vec(),
                                },
                            })
                        }
                        _ => None,
                    },
                    HomeView::Albums { albums, table, .. } => {
                        nav(table, &key.code);
                        match key.code {
                            KeyCode::Esc | KeyCode::Backspace => Some(HomeDrillAction::Pop),
                            KeyCode::Enter => {
                                let selected = table.selected().unwrap_or(0);
                                albums
                                    .items
                                    .get(selected)
                                    .map(|a| HomeDrillAction::OpenAlbum(a.clone()))
                            }
                            _ => None,
                        }
                    }
                    HomeView::Playlists {
                        playlists, table, ..
                    } => {
                        nav(table, &key.code);
                        match key.code {
                            KeyCode::Esc | KeyCode::Backspace => Some(HomeDrillAction::Pop),
                            KeyCode::Enter => {
                                let selected = table.selected().unwrap_or(0);
                                playlists.items.get(selected).map(|p| {
                                    HomeDrillAction::OpenPlaylist {
                                        playlist_id: p.id,
                                        name: p.name.clone(),
                                    }
                                })
                            }
                            _ => None,
                        }
                    }
                    HomeView::Artists { artists, table, .. } => {
                        nav(table, &key.code);
                        match key.code {
                            KeyCode::Esc | KeyCode::Backspace => Some(HomeDrillAction::Pop),
                            KeyCode::Enter => {
                                let selected = table.selected().unwrap_or(0);
                                artists.items.get(selected).map(|a| {
                                    HomeDrillAction::OpenArtist(a.id.clone(), a.name.clone())
                                })
                            }
                            _ => None,
                        }
                    }
                    HomeView::Genres { table, .. } => {
                        nav(table, &key.code);
                        match key.code {
                            KeyCode::Esc | KeyCode::Backspace => Some(HomeDrillAction::Pop),
                            KeyCode::Char('R') => Some(HomeDrillAction::RefetchGenres),
                            KeyCode::Enter => {
                                let selected = table.selected().unwrap_or(0);
                                self.home
                                    .genre_cache
                                    .as_ref()
                                    .and_then(|p| p.items.get(selected))
                                    .map(|g| HomeDrillAction::OpenGenre(g.title.clone()))
                            }
                            _ => None,
                        }
                    }
                    HomeView::Detail(d) => {
                        let table = &mut d.table;
                        nav(table, &key.code);
                        match key.code {
                            KeyCode::Esc | KeyCode::Backspace => Some(HomeDrillAction::Pop),
                            KeyCode::Enter => {
                                let selected = table.selected().unwrap_or(0);
                                let rows = self.home.detail_rows();
                                match rows.get(selected) {
                                    Some(crate::home::DetailRow::Song(s)) => Some(
                                        HomeDrillAction::Play((*s).clone(), StreamIntent::PlayNow),
                                    ),
                                    Some(crate::home::DetailRow::Album(a)) => {
                                        Some(HomeDrillAction::OpenAlbum((*a).clone()))
                                    }
                                    Some(crate::home::DetailRow::Playlist(p)) => {
                                        Some(HomeDrillAction::OpenPlaylist {
                                            playlist_id: p.id,
                                            name: p.name.clone(),
                                        })
                                    }
                                    Some(crate::home::DetailRow::Artist(a)) => Some(
                                        HomeDrillAction::OpenArtist(a.id.clone(), a.name.clone()),
                                    ),
                                    Some(crate::home::DetailRow::Sub(g)) => {
                                        Some(HomeDrillAction::OpenGenreSub(g.title.clone()))
                                    }
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                }
            };
        match action {
            Some(HomeDrillAction::Pop) => self.home.pop_level(),
            Some(HomeDrillAction::Play(song, intent)) => self.stream_song(song, intent),
            Some(HomeDrillAction::PlayAlbum { song, rest }) => self.play_now_enqueue(song, rest),
            Some(HomeDrillAction::Download(song)) => self.download_confirm = Some(song),
            Some(HomeDrillAction::OpenPlaylist { playlist_id, name }) => {
                self.open_home_list(DrillKind::Playlist(playlist_id, name))
            }
            Some(HomeDrillAction::OpenAlbum(album)) => {
                self.issue_album_songs(album, AlbumDrillSlot::Home)
            }
            Some(HomeDrillAction::OpenArtist(id, name)) => self.open_artist_drill(id, name),
            Some(HomeDrillAction::OpenGenre(name)) => self.open_genre_detail(name, None),
            Some(HomeDrillAction::OpenGenreSub(title)) => {
                let genre = match &self.home.view {
                    crate::home::HomeView::Detail(d) => d.genre.clone(),
                    _ => String::new(),
                };
                self.open_genre_detail(genre, Some(title));
            }
            Some(HomeDrillAction::RefetchGenres) => self.refetch_genres(true),
            _ => {}
        }
    }

    fn ensure_home_fetch(&mut self) {
        if !matches!(self.screen, Screen::Home) || self.home.gen != 0 || self.home.busy() {
            return;
        }
        self.refetch_home(false);
    }

    fn refetch_home(&mut self, fresh: bool) {
        if !matches!(self.screen, Screen::Home) || self.home.busy() {
            return;
        }
        self.home.gen = self.home.gen.wrapping_add(1);
        self.home.mark_loading();
        let gen = self.home.gen;
        tracing::info!("home: loading feeds (gen {gen}, fresh {fresh})");
        self.send_api(ApiCommand::HomeFeeds { gen, fresh });
    }

    fn ensure_tab_fetch(&mut self) {
        if !matches!(self.screen, Screen::Home) {
            return;
        }
        match self.tab {
            Tab::Home => self.ensure_home_fetch(),
            Tab::MyMusic => self.ensure_music_fetch(),
            _ => {}
        }
    }

    fn ensure_music_fetch(&mut self) {
        if !matches!(self.screen, Screen::Home) || self.music.gen != 0 || self.music.busy() {
            return;
        }
        self.refetch_music(false);
    }

    fn refetch_music(&mut self, fresh: bool) {
        if !matches!(self.screen, Screen::Home) || self.music.busy() {
            return;
        }
        self.music.gen = self.music.gen.wrapping_add(1);
        self.music.mark_loading();
        let gen = self.music.gen;
        tracing::info!("music: loading feeds (gen {gen}, fresh {fresh})");
        self.send_api(ApiCommand::MyMusic { gen, fresh });
    }

    fn handle_music_key(&mut self, key: KeyEvent) {
        if self.music.is_root() {
            self.handle_music_root(key);
        } else {
            self.handle_music_drill(key);
        }
    }

    fn handle_music_root(&mut self, key: KeyEvent) {
        use crate::mymusic::{MusicRow, NavDir};
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.music.nav(NavDir::Up),
            KeyCode::Down | KeyCode::Char('j') => self.music.nav(NavDir::Down),
            KeyCode::Char('g') => self.music.edge(NavDir::Up),
            KeyCode::Char('G') => self.music.edge(NavDir::Down),
            KeyCode::Enter => self.activate_music_item(),
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if let Some(MusicRow::Song(s)) = self.music.selected_row() {
                    self.stream_song(s.clone(), StreamIntent::Append);
                }
            }
            KeyCode::Char('y') => {
                if let Some(MusicRow::Song(s)) = self.music.selected_row() {
                    self.download_confirm = Some(s.clone());
                }
            }
            KeyCode::Char('R') => self.refetch_music(true),
            KeyCode::Esc | KeyCode::Backspace => {}
            _ => {}
        }
    }

    fn activate_music_item(&mut self) {
        use crate::mymusic::MusicRow;
        match self.music.selected_row() {
            Some(MusicRow::Head(feed, _)) => self.open_music_drill(feed),
            Some(MusicRow::Song(s)) => self.stream_song(s.clone(), StreamIntent::PlayNow),
            Some(MusicRow::Playlist(p)) => {
                let id = p.id;
                let name = p.name.clone();
                self.tab = Tab::Home;
                self.open_home_list(crate::home::DrillKind::Playlist(id, name));
            }
            _ => {}
        }
    }

    pub(crate) fn open_music_drill(&mut self, feed: crate::mymusic::MusicFeed) {
        self.nav_gen += 1;
        let gen = self.nav_gen;
        self.music.crumb = self
            .music
            .sections
            .iter()
            .find(|sec| sec.feed == feed)
            .map(|sec| sec.title.to_string());
        self.music.pending = Some(crate::mymusic::MusicPending {
            kind: crate::mymusic::MusicDrill::Feed(feed),
            gen,
        });
        self.music.view = crate::mymusic::MusicView::Loading {
            state: Load::Loading,
        };
        self.send_api(ApiCommand::MyMusicFeed { feed, gen });
    }

    fn handle_music_drill(&mut self, key: KeyEvent) {
        use crate::mymusic::MusicView;
        let action = {
            match &mut self.music.view {
                MusicView::Loading { .. } => match key.code {
                    KeyCode::Esc | KeyCode::Backspace => Some(MusicDrillAction::Pop),
                    _ => None,
                },
                MusicView::Playlists { page, table, .. } => match key.code {
                    KeyCode::Esc | KeyCode::Backspace => Some(MusicDrillAction::Pop),
                    KeyCode::Up | KeyCode::Char('k') => {
                        table.select_previous();
                        None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        table.select_next();
                        None
                    }
                    KeyCode::Char('g') => {
                        table.select_first();
                        None
                    }
                    KeyCode::Char('G') => {
                        table.select_last();
                        None
                    }
                    KeyCode::Enter => {
                        let selected = table.selected().unwrap_or(0);
                        page.items
                            .get(selected)
                            .map(|p| MusicDrillAction::OpenPlaylist {
                                playlist_id: p.id,
                                name: p.name.clone(),
                            })
                    }
                    _ => None,
                },
                MusicView::Songs { songs, table, .. } => match key.code {
                    KeyCode::Esc | KeyCode::Backspace => Some(MusicDrillAction::Pop),
                    KeyCode::Up | KeyCode::Char('k') => {
                        table.select_previous();
                        None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        table.select_next();
                        None
                    }
                    KeyCode::Char('g') => {
                        table.select_first();
                        None
                    }
                    KeyCode::Char('G') => {
                        table.select_last();
                        None
                    }
                    KeyCode::Enter => {
                        let selected = table.selected().unwrap_or(0);
                        songs
                            .items
                            .get(selected)
                            .map(|s| MusicDrillAction::Play(s.clone(), StreamIntent::PlayNow))
                    }
                    KeyCode::Char('a') | KeyCode::Char('A') => {
                        let selected = table.selected().unwrap_or(0);
                        songs
                            .items
                            .get(selected)
                            .map(|s| MusicDrillAction::Play(s.clone(), StreamIntent::Append))
                    }
                    KeyCode::Char('y') => {
                        let selected = table.selected().unwrap_or(0);
                        songs
                            .items
                            .get(selected)
                            .map(|s| MusicDrillAction::Download(s.clone()))
                    }
                    _ => None,
                },
                _ => None,
            }
        };
        match action {
            Some(MusicDrillAction::Pop) => self.music.reset(),
            Some(MusicDrillAction::Play(song, intent)) => self.stream_song(song, intent),
            Some(MusicDrillAction::Download(song)) => self.download_confirm = Some(song),
            Some(MusicDrillAction::OpenPlaylist { playlist_id, name }) => {
                self.tab = Tab::Home;
                self.open_home_list(crate::home::DrillKind::Playlist(playlist_id, name));
            }
            _ => {}
        }
    }

    pub(crate) fn open_genres_list(&mut self) {
        self.nav_gen += 1;
        let gen = self.nav_gen;
        self.home.enter_level("Genres".to_string());
        self.home.pending = Some(crate::home::PendingDrill {
            kind: crate::home::DrillKind::Genres,
            gen,
        });
        self.home.view = crate::home::HomeView::Loading {
            state: Load::Loading,
        };
        self.send_api(ApiCommand::Genres { gen, fresh: false });
    }

    fn apply_genres_list(
        &mut self,
        gen: u64,
        result: &ApiResult<crate::api::models::Page<crate::api::models::Genre>>,
    ) {
        if self
            .home
            .pending
            .as_ref()
            .map(|p| p.gen != gen || !matches!(p.kind, crate::home::DrillKind::Genres))
            .unwrap_or(true)
        {
            return;
        }
        self.home.pending = None;
        match result {
            Ok(page) => {
                let mut table = ratatui::widgets::TableState::default();
                if !page.items.is_empty() {
                    table.select(Some(0));
                }
                self.home.genre_cache = Some(page.clone());
                self.home.view = crate::home::HomeView::Genres {
                    page: page.clone(),
                    state: Load::Done,
                    table,
                };
            }
            Err(e) => {
                self.toast_error(format!("genres: {e}"));
                self.home.pop_level();
            }
        }
    }

    pub(crate) fn open_genre_detail(&mut self, genre: String, sub: Option<String>) {
        self.nav_gen += 1;
        let gen = self.nav_gen;
        let label = sub.clone().unwrap_or_else(|| genre.clone());
        self.home.enter_level(label);
        self.home.pending = Some(crate::home::PendingDrill {
            kind: crate::home::DrillKind::Genre(genre.clone(), sub.clone()),
            gen,
        });
        self.home.view = crate::home::HomeView::Loading {
            state: Load::Loading,
        };
        self.send_api(ApiCommand::GenreDetail { genre, sub, gen });
    }

    fn apply_genre_detail(
        &mut self,
        genre: &str,
        sub: &Option<String>,
        gen: u64,
        result: ApiResult<Box<crate::home::GenreDetail>>,
    ) {
        let matched = self.home.pending.as_ref().is_some_and(|p| {
            p.gen == gen
                && matches!(&p.kind, crate::home::DrillKind::Genre(g, s) if g == genre && s == sub)
        });
        if !matched {
            return;
        }
        self.home.pending = None;
        match result {
            Ok(detail) => self.home.view = crate::home::HomeView::Detail(detail),
            Err(e) => {
                self.toast_error(format!("genre: {e}"));
                self.home.pop_level();
            }
        }
    }

    pub(crate) fn refetch_genres(&mut self, fresh: bool) {
        self.nav_gen += 1;
        let gen = self.nav_gen;
        self.home.pending = Some(crate::home::PendingDrill {
            kind: crate::home::DrillKind::Genres,
            gen,
        });
        self.home.view = crate::home::HomeView::Loading {
            state: Load::Loading,
        };
        self.send_api(ApiCommand::Genres { gen, fresh });
    }

    fn handle_history_key(&mut self, key: KeyEvent) {
        let in_filter = self.history_focus == Focus::Input;
        if let Some(c) = Self::printable(&key) {
            if in_filter {
                let before = self.history_input.value().chars().count();
                self.history_input.handle(InputRequest::InsertChar(c));
                if self.history_input.value().chars().count() != before {
                    self.history_table.select(None);
                }
                return;
            }
            match c {
                'a' => self.history_action(StreamIntent::Append),
                'y' => {
                    let rows = self.history_filtered();
                    let idx = self
                        .history_table
                        .selected()
                        .unwrap_or(0)
                        .min(rows.len().saturating_sub(1));
                    if let Some(e) = rows.get(idx) {
                        self.download_confirm = Some(Self::history_song(e));
                    }
                }
                'D' => self.clear_history_question = true,
                '?' => self.help_open = true,
                'e' => {
                    self.errors_open = true;
                    self.errors_scroll = 0;
                }
                'o' => {
                    self.audio_menu_open = true;
                    self.send_player(crate::player::PlayerCommand::AudioDevices, false);
                }
                _ => {}
            }
            return;
        }
        let rows = self.history_filtered();
        let idx = self
            .history_table
            .selected()
            .unwrap_or(0)
            .min(rows.len().saturating_sub(1));
        if !rows.is_empty() {
            self.history_table.select(Some(idx));
        }
        match key.code {
            KeyCode::Enter if in_filter => self.history_focus = Focus::Results,
            KeyCode::Enter => self.history_action(StreamIntent::PlayNow),
            KeyCode::Backspace => {
                self.history_input.handle(InputRequest::DeletePrevChar);
                if !in_filter {
                    self.history_focus = Focus::Input;
                } else {
                    self.history_table.select(None);
                }
            }
            KeyCode::Delete if in_filter => {
                self.history_input.handle(InputRequest::DeleteNextChar);
            }
            KeyCode::Esc => {
                self.history_focus = if in_filter {
                    Focus::Results
                } else {
                    Focus::Input
                };
            }
            KeyCode::Up | KeyCode::Char('k') if !in_filter => {
                self.history_table.select(Some(idx.saturating_sub(1)));
            }
            KeyCode::Down | KeyCode::Char('j') if !in_filter => {
                self.history_table
                    .select(Some((idx + 1).min(rows.len().saturating_sub(1))));
            }
            KeyCode::Char('g') if !in_filter => {
                self.history_table.select(rows.first().map(|_| 0));
            }
            KeyCode::Char('G') if !in_filter => {
                self.history_table
                    .select(rows.last().map(|_| rows.len() - 1));
            }
            _ => {}
        }
    }

    fn handle_clear_history_key(&mut self, key: KeyEvent) {
        self.clear_history_question = false;
        if matches!(key.code, KeyCode::Char('y') | KeyCode::Enter) {
            self.history.clear();
            self.history_table.select(None);
            crate::history::save(&self.history_path, &self.history);
            self.toasts
                .push(Instant::now(), ToastLevel::Success, "history cleared");
        }
    }

    /// Filter text narrows by title/artist substring; with a filter the
    /// survivors are ordered best-match (never history-boosted here —
    /// the boost lives on the search surface inside Home).
    pub(crate) fn history_filtered(&self) -> Vec<crate::history::Entry> {
        let f = self.history_input.value().trim().to_lowercase();
        let mut rows: Vec<crate::history::Entry> = self
            .history
            .iter()
            .filter(|e| {
                f.is_empty()
                    || e.title.to_lowercase().contains(&f)
                    || e.artist_name.to_lowercase().contains(&f)
            })
            .cloned()
            .collect();
        if !f.is_empty() {
            rows.sort_by_key(|e| {
                crate::search::match_rank(&f, &e.title.to_lowercase())
                    .min(crate::search::match_rank(&f, &e.artist_name.to_lowercase()))
            });
        }
        rows
    }

    fn history_song(e: &crate::history::Entry) -> crate::api::models::Song {
        crate::api::models::Song {
            id: e.id,
            provider: Some(e.provider),
            title: e.title.clone(),
            artist: Some(crate::api::models::ArtistRef {
                id: e.artist_id.clone().unwrap_or_default(),
                name: e.artist_name.clone(),
            }),
            album: Some(crate::api::models::Album {
                title: e.album_title.clone(),
                ..crate::api::models::Album::default()
            }),
            album_id: e.album_id,
            genre: e.genre.clone(),
            full_duration_sec: e.duration_secs,
            ..crate::api::models::Song::default()
        }
    }

    fn history_action(&mut self, intent: StreamIntent) {
        let rows = self.history_filtered();
        let idx = self
            .history_table
            .selected()
            .unwrap_or(0)
            .min(rows.len().saturating_sub(1));
        let Some(entry) = rows.get(idx).cloned() else {
            return;
        };
        self.stream_song(Self::history_song(&entry), intent);
    }

    fn handle_player_keys(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl {
            return;
        }
        match key.code {
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('e') => {
                self.errors_open = true;
                self.errors_scroll = 0;
            }
            KeyCode::Char('o') => {
                self.audio_menu_open = true;
                self.send_player(crate::player::PlayerCommand::AudioDevices, false);
            }
            KeyCode::Char('l') => {
                self.loop_one = !self.loop_one;
                let state = if self.loop_one { "on" } else { "off" };
                self.toasts.push(
                    Instant::now(),
                    ToastLevel::Info,
                    format!("loop one: {state}"),
                );
            }
            KeyCode::Char(' ') => {
                if self.player_degraded.is_some() {
                    self.toast_degraded();
                } else {
                    self.send_player(crate::player::PlayerCommand::TogglePlay, false);
                }
            }
            KeyCode::Char(',') => {
                if self.player_has_position() {
                    self.send_player(crate::player::PlayerCommand::Seek { delta: -5.0 }, false);
                }
            }
            KeyCode::Char('.') => {
                if self.player_has_position() {
                    self.send_player(crate::player::PlayerCommand::Seek { delta: 5.0 }, false);
                }
            }
            KeyCode::Char('-') => {
                self.volume = self.volume.saturating_sub(5);
                self.send_player(crate::player::PlayerCommand::Volume { delta: -5 }, false);
            }
            KeyCode::Char('=') | KeyCode::Char('+') => {
                self.volume = (self.volume + 5).min(100);
                self.send_player(crate::player::PlayerCommand::Volume { delta: 5 }, false);
            }
            KeyCode::Char('n') => {
                self.flush_stream_time();
                self.queue.advance();
                self.auto_play_current();
            }
            KeyCode::Char('p') => {
                let Some((pos, _)) = self.queue.current() else {
                    return;
                };
                if pos == 0 {
                    return;
                }
                self.flush_stream_time();
                let previous = self.queue.items[pos - 1].song.clone();
                self.queue.items[pos].status = crate::queue::QueueStatus::Played;
                self.queue.items[pos - 1].status = crate::queue::QueueStatus::Playing;
                self.stream_song(previous, StreamIntent::Reload);
            }
            _ => {}
        }
    }

    fn player_has_position(&self) -> bool {
        self.player_degraded.is_none() && self.player_status.time.is_some()
    }

    fn toast_degraded(&mut self) {
        let reason = self
            .player_degraded
            .clone()
            .unwrap_or_else(|| "player unavailable".into());
        self.toast_error(format!("playback unavailable ({reason})"));
    }

    fn handle_queue_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.queue_table.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => self.queue_table.select_next(),
            KeyCode::Char('g') => self.queue_table.select_first(),
            KeyCode::Char('G') => self.queue_table.select_last(),
            KeyCode::Char('d') => {
                let Some(i) = self.queue_table.selected() else {
                    return;
                };
                self.queue_table.select(Some(i.saturating_sub(1)));
                let removed = self.queue.items.get(i).map(|it| it.song.title.clone());
                self.queue.remove(i);
                if let Some(title) = removed {
                    self.toasts
                        .push(Instant::now(), ToastLevel::Info, format!("removed {title}"));
                }
            }
            KeyCode::Char('J') => {
                let Some(i) = self.queue_table.selected() else {
                    return;
                };
                self.queue.move_down(i);
                self.queue_table
                    .select(Some((i + 1).min(self.queue.items.len() - 1)));
            }
            KeyCode::Char('K') => {
                let Some(i) = self.queue_table.selected() else {
                    return;
                };
                self.queue.move_up(i);
                self.queue_table.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Enter => {
                let Some(i) = self.queue_table.selected() else {
                    return;
                };
                if i < self.queue.items.len() {
                    self.flush_stream_time();
                    self.queue.play_at(i);
                    let song = self.queue.items[i].song.clone();
                    self.stream_song(song, StreamIntent::Reload);
                }
            }
            _ => self.handle_player_keys(key),
        }
    }

    fn handle_player_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::AudioDevices { devices } => self.audio_devices = devices,
            PlayerEvent::Status {
                time,
                duration,
                paused,
            } => {
                if time.is_some() {
                    self.player_status.time = time;
                }
                if duration.is_some() {
                    self.player_status.duration = duration;
                }
                if paused.is_some() {
                    self.player_status.paused = paused;
                }
                if self.player_status.paused == Some(true) {
                    self.flush_stream_time();
                }
            }
            PlayerEvent::BufferingDone => {}
            PlayerEvent::Connected => {
                self.player_started = true;
                if self.player_degraded.take().is_some() {
                    self.toasts
                        .push(Instant::now(), ToastLevel::Success, "playback available");
                }
            }
            PlayerEvent::Idle => {}
            PlayerEvent::EndFile { reason } => {
                self.flush_stream_time();
                match reason.as_str() {
                    "eof" => {
                        self.last_recorded = None;
                        if !self.loop_one {
                            self.queue.advance();
                        }
                        self.auto_play_current();
                    }
                    "error" => {
                        let current = self
                            .queue
                            .current()
                            .map(|(_, it)| (it.song.id, it.song.provider.unwrap_or(1)));
                        if let Some(current) = current {
                            self.stream_cache.remove(&current);
                        }
                        if let Some((id, provider)) = self.last_recorded.take() {
                            if current == Some((id, provider))
                                && crate::history::remove(&mut self.history, id, provider)
                            {
                                crate::history::save(&self.history_path, &self.history);
                            }
                        }
                        self.queue.fail_current();
                        self.toast_error("track failed to play");
                    }
                    _ => {}
                }
            }
            PlayerEvent::ConnectionLost => {
                self.player_degraded = Some("connection lost".into());
                self.toast_error("playback unavailable (connection lost)");
            }
            PlayerEvent::Degraded { reason } => {
                self.player_degraded = Some(reason.clone());
                self.toast_error(format!(
                    "playback unavailable ({reason}) — browsing & downloads still work"
                ));
            }
        }
    }

    /// Record a play at Load-send time. A play whose mpv load later fails
    /// is un-recorded by the EndFile{error} handler using the key below.
    fn record_history_from(&mut self, song: &crate::api::models::Song, stream_url: &str) {
        let provider = song.provider.unwrap_or(1);
        let entry = crate::history::Entry {
            id: song.id,
            provider,
            title: song.title.clone(),
            artist_name: song
                .artist
                .as_ref()
                .map(|a| a.name.clone())
                .unwrap_or_default(),
            artist_id: song.artist.as_ref().map(|a| a.id.clone()),
            album_title: song
                .album
                .as_ref()
                .map(|a| a.title.clone())
                .unwrap_or_default(),
            album_id: song.album_id,
            genre: song.genre.clone(),
            duration_secs: song.full_duration_sec,
            stream_url: stream_url.to_string(),
            played_at_unix: self.now_unix,
            plays: 1,
        };
        self.last_recorded = Some((entry.id, entry.provider));
        crate::history::record(&mut self.history, entry);
        crate::history::save(&self.history_path, &self.history);
    }

    fn flush_stream_time(&mut self) {
        let secs = self.pending_stream_secs;
        self.pending_stream_secs = 0.0;
        self.last_seen_time = None;
        if secs <= 1.0 {
            return;
        }
        if let Some((_, item)) = self.queue.current() {
            let song = &item.song;
            self.send_api(ApiCommand::RecordStreamTime {
                song_id: song.id,
                provider: song.provider.unwrap_or(1),
                secs,
            });
        }
    }

    fn auto_play_current(&mut self) {
        if self.player_degraded.is_some() {
            return;
        }
        if let Some((_, item)) = self.queue.current() {
            let song = item.song.clone();
            self.stream_song(song, StreamIntent::Reload);
        }
    }

    pub(crate) fn toast_error(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.toasts
            .push(Instant::now(), ToastLevel::Error, text.clone());
        self.errors.push(ErrorEntry {
            code: None,
            message: text,
        });
        while self.errors.len() > 100 {
            self.errors.remove(0);
        }
    }

    fn printable(key: &KeyEvent) -> Option<char> {
        match (&key.code, &key.modifiers) {
            (KeyCode::Char(c), m)
                if !m.intersects(
                    crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT,
                ) =>
            {
                Some(*c)
            }
            _ => None,
        }
    }

    fn dispatch_search(&mut self, gen: u64, fresh: bool) {
        if let Some(cell) = self.sent_gen.as_ref() {
            claim_dispatch(cell, gen);
        }
        let Some(SearchView::Root(home)) = self.search.top_mut() else {
            return;
        };
        let q = home.input.value().trim().to_string();
        if q.is_empty() {
            home.clear_results();
            return;
        }
        home.boost = crate::search::Boost::from_entries(&self.history);
        home.mark_dispatched(gen);
        for kind in [SearchKind::Song, SearchKind::Artist, SearchKind::Album] {
            self.send_api(ApiCommand::Search {
                q: q.clone(),
                kind,
                gen,
                offset: 0,
                limit: SEARCH_LIMIT,
                fresh,
            });
        }
    }

    fn issue_artist_albums(&mut self, artist_id: String, update_ctx: bool) {
        self.nav_gen += 1;
        let gen = self.nav_gen;
        if update_ctx {
            if let Some(SearchView::Artist(ctx)) = self.search.top_mut() {
                ctx.gen = gen;
                ctx.state = Load::Loading;
            }
        }
        self.send_api(ApiCommand::ArtistAlbums { artist_id, gen });
    }

    fn issue_album_songs(&mut self, album: Album, slot: AlbumDrillSlot) {
        self.nav_gen += 1;
        let gen = self.nav_gen;
        let home_slot = matches!(slot, AlbumDrillSlot::Home);
        let title = album.title.clone();
        let request = AlbumRequest {
            album: album.clone(),
            gen,
            slot,
        };
        self.send_api(ApiCommand::AlbumSongs { album, gen });
        if home_slot {
            self.home.enter_level(title);
        }
        match slot {
            AlbumDrillSlot::Root => {
                if let Some(SearchView::Root(home)) = self.search.top_mut() {
                    home.pending_album = Some(request);
                }
            }
            AlbumDrillSlot::Artist => {
                if let Some(SearchView::Artist(ctx)) = self.search.top_mut() {
                    ctx.state = Load::Loading;
                    ctx.pending_album = Some(request);
                }
            }
            AlbumDrillSlot::Home => {
                self.home.pending_album = Some(request);
            }
        }
    }

    fn restore_album_request(&mut self, req: crate::search::AlbumRequest) {
        match req.slot {
            AlbumDrillSlot::Home => self.home.pending_album = Some(req),
            AlbumDrillSlot::Root => {
                if let Some(SearchView::Root(home)) = self.search.top_mut() {
                    home.pending_album = Some(req);
                }
            }
            AlbumDrillSlot::Artist => {
                if let Some(SearchView::Artist(ctx)) = self.search.top_mut() {
                    ctx.pending_album = Some(req);
                }
            }
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        let is_root = matches!(self.search.top(), Some(SearchView::Root(_)));
        let is_artist = matches!(self.search.top(), Some(SearchView::Artist(_)));
        if is_root {
            self.handle_search_root(key);
        } else if is_artist {
            self.handle_search_artist(key);
        } else {
            self.handle_search_album(key);
        }
    }

    fn handle_search_root(&mut self, key: KeyEvent) {
        let action = {
            let Some(SearchView::Root(home)) = self.search.top_mut() else {
                return;
            };
            let input_focus = home.focus == Focus::Input;
            if let Some(c) = Self::printable(&key) {
                if input_focus {
                    let before_len = home.input.value().chars().count();
                    home.input.handle(InputRequest::InsertChar(c));
                    if home.input.value().chars().count() != before_len {
                        home.on_input_changed();
                        notify_search_input(&self.search_watch, home);
                    }
                    return;
                }
                match c {
                    '?' => self.help_open = true,
                    'e' => {
                        self.errors_open = true;
                        self.errors_scroll = 0;
                    }
                    'o' => {
                        self.audio_menu_open = true;
                        self.send_player(crate::player::PlayerCommand::AudioDevices, false);
                    }
                    'a' | 'A' | 'y' => {
                        let song = home
                            .rows()
                            .get(home.table.selected().unwrap_or(0))
                            .filter(|r| r.is_song())
                            .map(|r| match r {
                                crate::search::Row::Song(s) => s.clone(),
                                _ => unreachable!("is_song filtered"),
                            });
                        if let Some(song) = song {
                            match c {
                                'y' => self.download_confirm = Some(song),
                                'a' => self.stream_song(song, StreamIntent::Append),
                                _ => self.stream_song(song, StreamIntent::PlayNext),
                            }
                        }
                    }
                    'S' => {
                        home.sort = home.sort.cycle();
                        let label = home.sort.label();
                        self.toasts.push(
                            Instant::now(),
                            ToastLevel::Info,
                            format!("sort: {label}"),
                        );
                    }
                    _ => {}
                }
                return;
            }
            let in_input = home.focus == Focus::Input;
            match key.code {
                KeyCode::Enter if in_input && !home.input.value().trim().is_empty() => {
                    home.focus = Focus::Results;
                    Some(RootAction::Dispatch(home.gen))
                }
                KeyCode::Enter => Some(RootAction::ActivateRow),
                KeyCode::Backspace => {
                    home.input.handle(InputRequest::DeletePrevChar);
                    if !in_input {
                        home.focus = Focus::Input;
                    }
                    home.on_input_changed();
                    notify_search_input(&self.search_watch, home);
                    if home.input.value().trim().is_empty() {
                        Some(RootAction::RefetchHome)
                    } else {
                        None
                    }
                }
                KeyCode::Delete if in_input => {
                    home.input.handle(InputRequest::DeleteNextChar);
                    home.on_input_changed();
                    notify_search_input(&self.search_watch, home);
                    if home.input.value().trim().is_empty() {
                        Some(RootAction::RefetchHome)
                    } else {
                        None
                    }
                }
                KeyCode::Esc if in_input => {
                    if !home.input.value().trim().is_empty() {
                        home.input = Input::default();
                        home.on_input_changed();
                        Some(RootAction::RefetchHome)
                    } else {
                        home.focus = Focus::Results;
                        None
                    }
                }
                KeyCode::Esc if !in_input => {
                    if !home.input.value().trim().is_empty() {
                        home.focus = Focus::Input;
                    }
                    None
                }
                KeyCode::Up if in_input && !home.input.value().trim().is_empty() => {
                    home.focus = Focus::Results;
                    if home.table.selected().is_none() && home.has_rows() {
                        home.table.select(Some(0));
                    }
                    None
                }
                KeyCode::Down if in_input && home.has_rows() => {
                    home.focus = Focus::Results;
                    if home.table.selected().is_none() {
                        home.table.select(Some(0));
                    }
                    None
                }
                KeyCode::Up | KeyCode::Char('k') if !in_input => {
                    home.table.select_previous();
                    None
                }
                KeyCode::Down | KeyCode::Char('j') if !in_input => {
                    home.table.select_next();
                    None
                }
                KeyCode::Char('g') if !in_input => {
                    home.table.select_first();
                    None
                }
                KeyCode::Char('G') if !in_input => {
                    home.table.select_last();
                    None
                }
                KeyCode::Char('R') => {
                    if home.input.value().trim().is_empty() {
                        Some(RootAction::RefetchHome)
                    } else {
                        Some(RootAction::Refresh(home.gen))
                    }
                }
                _ => None,
            }
        };
        match action {
            None => {}
            Some(RootAction::RefetchHome) => self.ensure_home_fetch(),
            Some(RootAction::Dispatch(gen)) => self.dispatch_search(gen, false),
            Some(RootAction::Refresh(gen)) => self.dispatch_search(gen, true),
            Some(RootAction::ActivateRow) => self.activate_search_row(),
        }
    }

    fn handle_search_artist(&mut self, key: KeyEvent) {
        enum Action {
            None,
            Pop,
            Drill(Box<Album>),
            Retry,
            Quit,
        }
        let artist_id = {
            let Some(SearchView::Artist(ctx)) = self.search.top_mut() else {
                return;
            };
            let action = {
                match key.code {
                    KeyCode::Esc | KeyCode::Backspace => Action::Pop,
                    KeyCode::Up | KeyCode::Char('k') => {
                        ctx.table.select_previous();
                        Action::None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        ctx.table.select_next();
                        Action::None
                    }
                    KeyCode::Char('g') => {
                        ctx.table.select_first();
                        Action::None
                    }
                    KeyCode::Char('G') => {
                        ctx.table.select_last();
                        Action::None
                    }
                    KeyCode::Enter => {
                        let selected = ctx.table.selected().unwrap_or(0);
                        match ctx.albums.items.get(selected) {
                            Some(album) => Action::Drill(Box::new(album.clone())),
                            None => Action::None,
                        }
                    }
                    KeyCode::Char('R') if matches!(ctx.state, Load::Failed(_)) => Action::Retry,
                    KeyCode::Char('q') => Action::Quit,
                    _ => Action::None,
                }
            };
            match action {
                Action::None => return,
                Action::Pop => {
                    self.search.pop();
                    return;
                }
                Action::Drill(album) => {
                    self.issue_album_songs(*album, AlbumDrillSlot::Artist);
                    return;
                }
                Action::Retry => ctx.artist.id.clone(),
                Action::Quit => {
                    self.should_quit = true;
                    return;
                }
            }
        };
        self.issue_artist_albums(artist_id, true);
    }

    fn handle_search_album(&mut self, key: KeyEvent) {
        let action = {
            let Some(SearchView::Album(ctx)) = self.search.top_mut() else {
                return;
            };
            match key.code {
                KeyCode::Esc | KeyCode::Backspace => Some(AlbumAction::Pop),
                KeyCode::Char('q') => Some(AlbumAction::Quit),
                KeyCode::Up | KeyCode::Char('k') => {
                    ctx.table.select_previous();
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    ctx.table.select_next();
                    None
                }
                KeyCode::Char('g') => {
                    ctx.table.select_first();
                    None
                }
                KeyCode::Char('G') => {
                    ctx.table.select_last();
                    None
                }
                KeyCode::Enter | KeyCode::Char('a') | KeyCode::Char('A') | KeyCode::Char('y') => {
                    let selected = ctx.table.selected().unwrap_or(0);
                    ctx.album.songs.get(selected).map(|s| match key.code {
                        KeyCode::Char('y') => AlbumAction::Download(s.clone()),
                        KeyCode::Char('a') => AlbumAction::Play(s.clone(), StreamIntent::Append),
                        KeyCode::Char('A') => AlbumAction::Play(s.clone(), StreamIntent::PlayNext),
                        _ => AlbumAction::PlayAlbum {
                            song: s.clone(),
                            rest: ctx.album.songs.get(selected + 1..).unwrap_or(&[]).to_vec(),
                        },
                    })
                }
                _ => None,
            }
        };
        match action {
            Some(AlbumAction::Play(song, intent)) => self.stream_song(song, intent),
            Some(AlbumAction::PlayAlbum { song, rest }) => self.play_now_enqueue(song, rest),
            Some(AlbumAction::Download(song)) => self.download_confirm = Some(song),
            Some(AlbumAction::Pop) => {
                self.search.pop();
            }
            Some(AlbumAction::Quit) => self.should_quit = true,
            None => {}
        }
    }

    fn activate_search_row(&mut self) {
        let target = {
            let Some(SearchView::Root(home)) = self.search.top() else {
                return;
            };
            let Some(idx) = home.table.selected() else {
                return;
            };
            let row = match home.rows().get(idx) {
                Some(r) => r.clone(),
                None => return,
            };
            if row.is_song() {
                match &row {
                    crate::search::Row::Song(s) => Some(RowTarget::Song(s.clone())),
                    _ => None,
                }
            } else if let Some(artist_id) = row.artist_id() {
                Some(RowTarget::Artist(artist_id.to_string()))
            } else {
                row.album().map(|a| RowTarget::Album(a.clone()))
            }
        };
        match target {
            Some(RowTarget::Song(song)) => {
                self.stream_song(song, StreamIntent::PlayNow);
            }
            Some(RowTarget::Artist(artist_id)) => {
                let artist = self.search.top().and_then(|v| match v {
                    SearchView::Root(home) => home
                        .artists
                        .items
                        .iter()
                        .find(|a| a.id == artist_id)
                        .cloned(),
                    _ => None,
                });
                let Some(artist) = artist else {
                    return;
                };
                self.search
                    .stack
                    .push(SearchView::Artist(Box::new(ArtistContext {
                        artist: artist.clone(),
                        albums: Page::default(),
                        state: Load::Loading,
                        gen: 0,
                        table: TableState::default(),
                        pending_album: None,
                    })));
                self.issue_artist_albums(artist.id, true);
            }
            Some(RowTarget::Album(album)) => {
                self.issue_album_songs(album, AlbumDrillSlot::Root);
            }
            None => {}
        }
    }

    fn apply_details(&mut self, details: &UserDetails) {
        self.library_name = details.library.as_ref().map(|l| l.name.clone());
        self.downloads_used = details
            .download_limit
            .zip(details.available_download)
            .map(|(limit, avail)| limit - avail);
        self.downloads_limit = details.download_limit;
        self.stream_time_remaining = details.streaming_time_remaining.clone();
    }

    fn handle_api(&mut self, event: ApiEvent) {
        match event {
            ApiEvent::LibraryResolved { result } => match result {
                Ok(library) => {
                    self.library_name = Some(library.name.clone());
                    self.stored_subdomain = None;
                    if let Screen::Login(w) = &mut self.screen {
                        w.busy = false;
                        w.error = None;
                        w.library = Some(library);
                        w.step = WizardStep::LibraryConfirm;
                    }
                }
                Err(e) => {
                    self.toast_error(format!("library lookup failed: {e}"));
                    if let Screen::Login(w) = &mut self.screen {
                        w.busy = false;
                        w.error = Some(format!("{e}"));
                        if w.fallback.is_none() {
                            w.fallback = Some(Default::default());
                        }
                    }
                }
            },
            ApiEvent::LibraryIdSet { result } => match result {
                Err(e) => {
                    self.toast_error(format!("library id: {e}"));
                    if let Screen::Login(w) = &mut self.screen {
                        w.busy = false;
                        w.error = Some(format!("{e}"));
                    }
                }
                Ok(()) => {
                    self.toasts
                        .push(Instant::now(), ToastLevel::Success, "library id saved");
                    if let Screen::Login(w) = &mut self.screen {
                        w.fallback = None;
                        w.busy = false;
                        w.error = None;
                        w.step = WizardStep::Card;
                    }
                }
            },
            ApiEvent::Restored { result } => {
                self.restoring = false;
                match result {
                    Ok(details) => {
                        self.apply_details(&details);
                        self.screen = Screen::Home;
                        self.send_api(ApiCommand::GetDownloadedIds);
                        self.ensure_home_fetch();
                    }
                    Err(e) => {
                        self.toast_error(format!("re-login failed: {e}"));
                        if let Screen::Login(w) = &mut self.screen {
                            w.error = Some(format!("{e}"));
                        }
                    }
                }
            }
            ApiEvent::LoginResult { result } => match result {
                Ok(details) => {
                    self.apply_details(&details);
                    if let Screen::Login(w) = &mut self.screen {
                        w.busy = false;
                        w.error = None;
                        w.step = WizardStep::Subdomain;
                    }
                    self.screen = Screen::Home;
                    self.toasts
                        .push(Instant::now(), ToastLevel::Success, "signed in");
                    self.send_api(ApiCommand::GetDownloadedIds);
                    self.ensure_home_fetch();
                }
                Err(e) => {
                    self.toast_error(format!("login failed: {e}"));
                    if let Screen::Login(w) = &mut self.screen {
                        w.busy = false;
                        w.error = Some(format!("{e}"));
                    }
                }
            },
            ApiEvent::SearchDispatched { gen } => {
                if let Some(SearchView::Root(home)) = self.search.top_mut() {
                    home.mark_dispatched(gen);
                }
            }
            ApiEvent::SearchResults { gen, kind, result } => {
                if let Some(SearchView::Root(home)) = self.search.top_mut() {
                    match result {
                        Ok(container) => {
                            let (load, page) = match container {
                                ContainerKind::Songs(p) => (Load::Done, AppliedPage::Songs(p)),
                                ContainerKind::Artists(p) => (Load::Done, AppliedPage::Artists(p)),
                                ContainerKind::Albums(p) => (Load::Done, AppliedPage::Albums(p)),
                            };
                            home.apply_kind(gen, kind, load, page);
                        }
                        Err(e) => {
                            home.apply_kind(
                                gen,
                                kind,
                                Load::Failed(e.to_string()),
                                AppliedPage::None,
                            );
                        }
                    }
                }
            }
            ApiEvent::HomeFeed {
                feed,
                gen,
                result,
                section,
            } => {
                if !section {
                    let drill = self.home.pending.as_ref().is_some_and(|p| {
                        matches!(&p.kind, crate::home::DrillKind::Feed(f) if *f == feed)
                            && p.gen == gen
                    });
                    if !drill {
                        return;
                    }
                    self.home.pending = None;
                    self.fill_home_feed_drill(result.clone());
                    return;
                }
                if self.home.gen != gen {
                    return;
                }
                let Some(section) = self.home.section_mut(feed) else {
                    return;
                };
                match result {
                    Ok(data) => {
                        section.data = data.clone();
                        section.state = Load::Done;
                    }
                    Err(e) => section.state = Load::Failed(e.to_string()),
                }
            }
            ApiEvent::MyMusicFeed {
                feed,
                gen,
                result,
                section,
            } => {
                if !section {
                    let drill = self.music.pending.as_ref().is_some_and(|p| {
                        matches!(&p.kind, crate::mymusic::MusicDrill::Feed(f) if *f == feed)
                            && p.gen == gen
                    });
                    if !drill {
                        return;
                    }
                    self.music.pending = None;
                    self.fill_music_drill(result.clone());
                    return;
                }
                if self.music.gen != gen {
                    return;
                }
                let Some(section) = self.music.section_mut(feed) else {
                    return;
                };
                match result {
                    Ok(data) => {
                        section.data = data.clone();
                        section.state = Load::Done;
                    }
                    Err(e) => section.state = Load::Failed(e.to_string()),
                }
            }
            ApiEvent::Genres { gen, result } => {
                self.apply_genres_list(gen, &result);
            }
            ApiEvent::GenreDetail {
                genre,
                sub,
                gen,
                result,
            } => {
                self.apply_genre_detail(&genre, &sub, gen, result);
            }
            ApiEvent::PlaylistSongs {
                playlist_id,
                gen,
                result,
            } => {
                let Some(pending) = self.home.pending.as_ref() else {
                    return;
                };
                let pending_matches = match &pending.kind {
                    crate::home::DrillKind::Playlist(id, _) => *id == playlist_id,
                    _ => false,
                };
                if !pending_matches || pending.gen != gen {
                    return;
                }
                self.home.pending = None;
                let crate::home::HomeView::List {
                    songs,
                    state,
                    table,
                    ..
                } = &mut self.home.view
                else {
                    return;
                };
                match result {
                    Ok(page) => {
                        *songs = page;
                        *state = Load::Done;
                        if table.selected().is_none() && !songs.items.is_empty() {
                            table.select(Some(0));
                        }
                    }
                    Err(e) => *state = Load::Failed(e.to_string()),
                }
            }
            ApiEvent::ArtistAlbums {
                artist_id,
                gen,
                result,
            } => {
                if let Some(SearchView::Artist(ctx)) = self.search.top_mut() {
                    if ctx.artist.id == artist_id && ctx.gen == gen {
                        match result {
                            Ok(page) => {
                                ctx.albums = page;
                                ctx.state = Load::Done;
                                if ctx.table.selected().is_none() {
                                    ctx.table.select(Some(0));
                                }
                            }
                            Err(e) => ctx.state = Load::Failed(e.to_string()),
                        }
                        return;
                    }
                }
                let Some(pending) = self.home.pending.as_ref() else {
                    return;
                };
                let artist_drill =
                    matches!(&pending.kind, crate::home::DrillKind::Artist(id) if *id == artist_id);
                if !artist_drill || pending.gen != gen {
                    return;
                }
                self.home.pending = None;
                match result {
                    Ok(albums) => {
                        let mut table = ratatui::widgets::TableState::default();
                        if !albums.items.is_empty() {
                            table.select(Some(0));
                        }
                        self.home.view = crate::home::HomeView::Albums {
                            albums,
                            state: Load::Done,
                            table,
                        };
                    }
                    Err(e) => {
                        self.toast_error(format!("artist: {e}"));
                        self.home.pop_level();
                    }
                }
            }
            ApiEvent::AlbumSongs {
                album_id,
                gen,
                result,
            } => {
                let req = self
                    .home
                    .pending_album
                    .take()
                    .or_else(|| match self.search.top_mut() {
                        Some(SearchView::Root(home)) => home.pending_album.take(),
                        Some(SearchView::Artist(ctx)) => ctx.pending_album.take(),
                        _ => None,
                    });
                let Some(req) = req else { return };
                if req.album.id != album_id || req.gen != gen {
                    self.restore_album_request(req);
                    return;
                }
                match result {
                    Ok(album) => {
                        if matches!(req.slot, AlbumDrillSlot::Home) {
                            self.home.view = crate::home::HomeView::Album(Box::new(AlbumContext {
                                album,
                                table: Default::default(),
                            }));
                        } else {
                            self.search
                                .stack
                                .push(SearchView::Album(Box::new(AlbumContext {
                                    album,
                                    table: Default::default(),
                                })));
                        }
                    }
                    Err(e) => {
                        self.toast_error(format!("album: {e}"));
                        match req.slot {
                            AlbumDrillSlot::Home => self.home.pop_level(),
                            AlbumDrillSlot::Root => {}
                            AlbumDrillSlot::Artist => {
                                if let Some(SearchView::Artist(ctx)) = self.search.top_mut() {
                                    ctx.state = Load::Failed(e.to_string());
                                }
                            }
                        }
                    }
                }
            }
            ApiEvent::StreamReady {
                song,
                intent,
                result,
            } => match result {
                Ok(ticket) if ticket.stream_url.is_empty() => {
                    tracing::warn!("stream ready but url empty — intent {intent:?}");
                    self.toast_error("no stream URL returned");
                    self.stream_cache
                        .remove(&(song.id, song.provider.unwrap_or(1)));
                    if matches!(intent, StreamIntent::PlayNow | StreamIntent::Reload) {
                        self.queue.fail_current();
                    }
                }
                Ok(ticket) => {
                    self.stream_cache.insert(
                        (song.id, song.provider.unwrap_or(1)),
                        (ticket.stream_url.clone(), self.now_unix),
                    );
                    self.apply_stream_ticket(&song, intent, &ticket.stream_url);
                }
                Err(e) => {
                    tracing::warn!("stream ready failed: {e:#} — intent {intent:?}");
                    self.toast_error(format!("{e}"));
                    if matches!(intent, StreamIntent::PlayNow | StreamIntent::Reload) {
                        self.queue.fail_current();
                    }
                }
            },
            ApiEvent::StreamTimeRecorded { result } => {
                if let Ok(remaining) = result {
                    self.stream_time_remaining = remaining;
                }
            }
            ApiEvent::DownloadComplete { result } => match result {
                Ok(outcome) => {
                    if let Some(details) = outcome.details.as_ref() {
                        self.apply_details(details);
                    }
                    if !self.downloaded_ids.contains(&outcome.song_id) {
                        self.downloaded_ids.push(outcome.song_id);
                    }
                    self.downloads.push(DownloadRow {
                        song_id: outcome.song_id,
                        title: outcome.title,
                        artist: outcome.artist,
                        file_name: outcome.file_name.clone(),
                    });
                    self.toasts.push(
                        Instant::now(),
                        ToastLevel::Success,
                        format!("saved {}", outcome.file_name),
                    );
                }
                Err(e) => {
                    self.toast_error(format!("download failed: {e}"));
                }
            },
            ApiEvent::DownloadedIds { result } => {
                if let Ok(ids) = result {
                    self.downloaded_ids = ids;
                }
            }
            ApiEvent::NetBusy => self.net_active = true,
            ApiEvent::NetIdle => self.net_active = false,
        }
    }

    fn handle_audio_menu_key(&mut self, key: KeyEvent) {
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Char('o')
        ) {
            self.audio_menu_open = false;
            return;
        }
        match key.code {
            KeyCode::Enter => {
                let idx = self.audio_menu_table.selected().unwrap_or(0);
                if let Some((device, desc)) = self.audio_devices.get(idx).cloned() {
                    self.audio_menu_open = false;
                    self.send_player(
                        crate::player::PlayerCommand::SetAudioDevice { device },
                        false,
                    );
                    self.toasts.push(
                        Instant::now(),
                        ToastLevel::Success,
                        format!("audio output: {desc}"),
                    );
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.audio_menu_table.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => self.audio_menu_table.select_next(),
            _ => {}
        }
    }

    fn handle_download_confirm_key(&mut self, key: KeyEvent) {
        let Some(song) = self.download_confirm.as_ref() else {
            return;
        };
        match key.code {
            KeyCode::Char('y') => {
                let song = song.clone();
                let redownload = self.downloaded_ids.contains(&song.id);
                self.download_confirm = None;
                self.send_api(ApiCommand::DownloadSong { song, redownload });
            }
            KeyCode::Char('n') | KeyCode::Esc | KeyCode::Enter => {
                self.download_confirm = None;
            }
            _ => {}
        }
    }
}

fn player_title(song: &crate::api::models::Song) -> String {
    let artist = song
        .artist
        .as_ref()
        .map(|a| a.name.as_str())
        .unwrap_or("unknown artist");
    format!("{} — {}", song.title, artist)
}

#[allow(clippy::large_enum_variant)]
enum RowTarget {
    Song(crate::api::models::Song),
    Artist(String),
    Album(Album),
}

#[allow(clippy::large_enum_variant)]
enum AlbumAction {
    Play(crate::api::models::Song, StreamIntent),
    PlayAlbum {
        song: crate::api::models::Song,
        rest: Vec<crate::api::models::Song>,
    },
    Download(crate::api::models::Song),
    Pop,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumDrillSlot {
    Root,
    Artist,
    Home,
}

enum MusicDrillAction {
    Play(crate::api::models::Song, StreamIntent),
    Download(crate::api::models::Song),
    OpenPlaylist { playlist_id: u64, name: String },
    Pop,
}

enum HomeDrillAction {
    Play(crate::api::models::Song, StreamIntent),
    PlayAlbum {
        song: crate::api::models::Song,
        rest: Vec<crate::api::models::Song>,
    },
    Download(crate::api::models::Song),
    OpenPlaylist {
        playlist_id: u64,
        name: String,
    },
    OpenAlbum(crate::api::models::Album),
    OpenArtist(String, String),
    OpenGenre(String),
    OpenGenreSub(String),
    RefetchGenres,
    Pop,
}

enum RootAction {
    Dispatch(u64),
    Refresh(u64),
    ActivateRow,
    RefetchHome,
}

#[derive(Debug, Clone)]
pub struct DownloadRow {
    #[allow(dead_code)]
    pub song_id: u64,
    pub title: String,
    pub artist: String,
    pub file_name: String,
}

#[derive(Debug, Clone)]
pub struct ErrorEntry {
    pub code: Option<i64>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct DownloadOutcome {
    pub song_id: u64,
    pub title: String,
    pub artist: String,
    pub file_name: String,
    pub details: Option<UserDetails>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::client::UserDetails;
    use crate::api::ApiError;
    use crate::ui::tests::search_rows_app;
    use crossterm::event::KeyModifiers;

    fn plain(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn app() -> App {
        App::new(None)
    }

    fn wizard_mut(app: &mut App) -> &mut LoginWizard {
        match &mut app.screen {
            Screen::Login(w) => w,
            Screen::Home => panic!("wizard expected"),
        }
    }

    fn home() -> App {
        let mut a = app();
        a.make_home();
        // Hermetic history: no real disk file, no leftover entries.
        a.history_path = std::env::temp_dir().join(format!("freegal-test-{}", std::process::id()));
        a.history.clear();
        a
    }

    #[test]
    fn wizard_prefills_saved_card_and_pin() {
        let creds = crate::api::creds::StoredCreds {
            subdomain: "demo".into(),
            card_number: "12345678901234".into(),
            pin: "4321".into(),
            ..Default::default()
        };
        let mut a = App::new(Some(&creds));
        let w = wizard_mut(&mut a);
        assert_eq!(w.card_input.value(), "12345678901234");
        assert_eq!(w.pin_input.value(), "4321");

        // No stored card/PIN → fields stay empty, nothing to leak between runs.
        let mut a = App::new(Some(&crate::api::creds::StoredCreds::default()));
        let w = wizard_mut(&mut a);
        assert!(w.card_input.value().is_empty());
        assert!(w.pin_input.value().is_empty());
    }

    #[test]
    fn tab_key_cycles_forward() {
        let mut a = home();
        a.update(AppEvent::Key(plain(KeyCode::Tab)));
        assert_eq!(a.tab(), Tab::MyMusic);
        a.update(AppEvent::Key(plain(KeyCode::Tab)));
        assert_eq!(a.tab(), Tab::Queue);
        a.update(AppEvent::Key(plain(KeyCode::Tab)));
        assert_eq!(a.tab(), Tab::History);
        a.update(AppEvent::Key(plain(KeyCode::Tab)));
        assert_eq!(a.tab(), Tab::Downloads);
        a.update(AppEvent::Key(plain(KeyCode::Tab)));
        assert_eq!(a.tab(), Tab::Home);
        a.update(AppEvent::Key(plain(KeyCode::Tab)));
        assert_eq!(a.tab(), Tab::MyMusic);
        a.update(AppEvent::Key(plain(KeyCode::Tab)));
        assert_eq!(a.tab(), Tab::Queue);
    }

    #[test]
    fn queue_none_selection_guards_d_and_enter() {
        let mut a = home();
        a.queue.append(crate::api::models::Song {
            id: 1,
            title: "one".into(),
            ..crate::api::models::Song::default()
        });
        a.queue.append(crate::api::models::Song {
            id: 2,
            title: "two".into(),
            ..crate::api::models::Song::default()
        });
        a.update(AppEvent::Key(plain(KeyCode::Char('3'))));
        assert_eq!(a.tab(), Tab::Queue);
        a.update(AppEvent::Key(plain(KeyCode::Char('d'))));
        assert_eq!(a.queue.items.len(), 2);
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        assert!(a.queue.current().is_none());
    }

    #[test]
    fn history_y_downloads_selected_row() {
        let mut a = home();
        let dir = with_local_history(&mut a);
        a.record_history_from(
            &crate::api::models::Song {
                id: 1,
                title: "first".into(),
                ..crate::api::models::Song::default()
            },
            "https://x/1",
        );
        a.record_history_from(
            &crate::api::models::Song {
                id: 2,
                title: "second".into(),
                ..crate::api::models::Song::default()
            },
            "https://x/2",
        );
        a.update(AppEvent::Key(plain(KeyCode::Char('4'))));
        a.history_table.select(Some(0));
        a.update(AppEvent::Key(plain(KeyCode::Char('y'))));
        assert_eq!(
            a.download_confirm.as_ref().map(|s| s.id),
            Some(2),
            "y must respect the table selection"
        );
        let _ = dir.keep();
    }

    #[test]
    fn album_event_routes_to_home_by_slot() {
        let mut a = home();
        let album = crate::api::models::Album {
            id: 9,
            title: "Home Album".into(),
            ..crate::api::models::Album::default()
        };
        a.home.pending_album = Some(crate::search::AlbumRequest {
            album: album.clone(),
            gen: 1,
            slot: AlbumDrillSlot::Home,
        });
        a.update(AppEvent::Api(ApiEvent::AlbumSongs {
            album_id: 9,
            gen: 1,
            result: Ok(album),
        }));
        assert!(matches!(a.home.view, crate::home::HomeView::Album(_)));
        assert!(a.home.pending_album.is_none());
    }

    #[test]
    fn music_drill_uses_nav_gen_keeps_sections_running() {
        let mut a = home();
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        a.music.gen = 7;
        a.open_music_drill(crate::mymusic::MusicFeed::Playlists);
        assert_eq!(
            a.music.gen, 7,
            "drill must not invalidate in-flight sections"
        );
        let pending = a.music.pending.expect("drill pending set");
        assert_eq!(pending.gen, a.nav_gen);
        let _ = arx.try_recv();
    }

    #[test]
    fn digits_jump_directly_to_tabs() {
        let mut a = home();
        a.update(AppEvent::Key(plain(KeyCode::Char('1'))));
        assert_eq!(a.tab(), Tab::Home);
        a.update(AppEvent::Key(plain(KeyCode::Char('2'))));
        assert_eq!(a.tab(), Tab::MyMusic);
        a.update(AppEvent::Key(plain(KeyCode::Char('3'))));
        assert_eq!(a.tab(), Tab::Queue);
        a.update(AppEvent::Key(plain(KeyCode::Char('4'))));
        assert_eq!(a.tab(), Tab::History);
        a.update(AppEvent::Key(plain(KeyCode::Char('5'))));
        assert_eq!(a.tab(), Tab::Downloads);
    }

    #[test]
    fn h_key_opens_history_tab() {
        let mut a = home();
        a.update(AppEvent::Key(plain(KeyCode::Char('4'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('h'))));
        assert_eq!(a.tab(), Tab::History);
    }

    fn with_local_history(a: &mut App) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        a.history_path = dir.path().join("history.json");
        dir
    }

    #[test]
    fn s_at_results_focuses_the_box() {
        let mut a = home();
        assert_eq!(a.tab(), Tab::Home);
        a.update(AppEvent::Key(plain(KeyCode::Char('s'))));
        assert_eq!(a.tab(), Tab::Home, "'s' on the dashboard stays put");
        let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
            panic!("root expected")
        };
        assert_eq!(
            h.focus,
            crate::search::Focus::Input,
            "'s' refocuses the box"
        );
    }

    #[test]
    fn filter_typing_keeps_letters_and_esc_blurs() {
        let mut a = home();
        let _dir = with_local_history(&mut a);
        a.update(AppEvent::Key(plain(KeyCode::Char('4'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('/'))));
        assert_eq!(a.history_focus, crate::search::Focus::Input);
        a.update(AppEvent::Key(plain(KeyCode::Char('q'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('u'))));
        assert_eq!(a.history_input.value(), "qu", "letters type into filter");
        assert_eq!(a.tab(), Tab::History, "no tab jump while typing");
        a.update(AppEvent::Key(plain(KeyCode::Esc)));
        assert_eq!(a.history_focus, crate::search::Focus::Results, "Esc blurs");
        assert_eq!(a.tab(), Tab::History, "Esc never jumps tabs");
    }

    #[test]
    fn history_enter_restreams_stored_song() {
        let mut a = home();
        {
            let _dir = with_local_history(&mut a);
        }
        a.history = vec![crate::history::Entry {
            id: 42,
            provider: 1,
            title: "Past Hit".into(),
            artist_name: "Old Band".into(),
            artist_id: Some("a9".into()),
            album_title: "Long Ago".into(),
            album_id: Some(3),
            genre: Some("Jazz".into()),
            duration_secs: Some(201),
            stream_url: "https://x/old.m3u8".into(),
            played_at_unix: 1_000,
            plays: 1,
        }];
        a.now_unix = 2_000;
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        a.update(AppEvent::Key(plain(KeyCode::Char('4'))));
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        let cmd = loop {
            match arx.try_recv().expect("StreamSong queued") {
                c @ ApiCommand::StreamSong { .. } => break c,
                _ => continue, // skip GetDownloadedIds etc.
            }
        };
        match cmd {
            ApiCommand::StreamSong {
                song,
                intent: StreamIntent::PlayNow,
            } => {
                assert_eq!(song.id, 42);
                assert_eq!(song.title, "Past Hit");
                assert_eq!(
                    song.artist.as_ref().map(|x| x.name.as_str()),
                    Some("Old Band")
                );
                assert_eq!(song.full_duration_sec, Some(201));
            }
            other => panic!("expected StreamSong, got {other:?}"),
        }
        let _ = cmd;
    }

    #[test]
    fn enter_on_library_list_enqueues_rest_and_auto_advances() {
        let mut a = home();
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        let mk = |id: u64, title: &str| crate::api::models::Song {
            id,
            title: title.into(),
            ..crate::api::models::Song::default()
        };
        a.home.view = crate::home::HomeView::List {
            songs: crate::api::models::Page {
                items: vec![mk(1, "One"), mk(2, "Two"), mk(3, "Three")],
                ..crate::api::models::Page::default()
            },
            state: crate::search::Load::Done,
            table: Default::default(),
        };
        a.update(AppEvent::Key(plain(KeyCode::Enter)));

        assert_eq!(a.queue.len(), 3);
        assert_eq!(a.queue.current().unwrap().1.song.id, 1);
        assert_eq!(a.queue.items[1].song.id, 2);
        assert_eq!(a.queue.items[2].song.id, 3);
        let cmd = loop {
            match arx.try_recv().expect("StreamSong queued") {
                c @ ApiCommand::StreamSong { .. } => break c,
                _ => continue,
            }
        };
        match cmd {
            ApiCommand::StreamSong {
                song,
                intent: StreamIntent::PlayNow,
            } => assert_eq!(song.id, 1),
            other => panic!("expected StreamSong, got {other:?}"),
        }

        a.update(AppEvent::Api(ApiEvent::StreamReady {
            song: mk(1, "One"),
            intent: StreamIntent::PlayNow,
            result: Ok(crate::api::client::StreamTicket {
                stream_url: "https://x/manifest.m3u8".into(),
                time_remaining: None,
            }),
        }));
        assert_eq!(a.queue.len(), 3, "pre-enqueued current must not duplicate");
        assert_eq!(a.queue.current().unwrap().1.song.id, 1);

        a.update(AppEvent::Player(crate::player::PlayerEvent::EndFile {
            reason: "eof".into(),
        }));
        assert_eq!(
            a.queue.current().unwrap().1.song.id,
            2,
            "eof advances into the enqueued rest"
        );
        let cmd = loop {
            match arx.try_recv().expect("Reload sent") {
                c @ ApiCommand::StreamSong { .. } => break c,
                _ => continue,
            }
        };
        match cmd {
            ApiCommand::StreamSong {
                song,
                intent: StreamIntent::Reload,
            } => assert_eq!(song.id, 2),
            other => panic!("expected Reload, got {other:?}"),
        }
    }

    #[test]
    fn enter_on_album_context_enqueues_rest() {
        let mut a = home();
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        let mk = |id: u64, title: &str| crate::api::models::Song {
            id,
            title: title.into(),
            ..crate::api::models::Song::default()
        };
        a.search = crate::search::SearchTab {
            stack: vec![crate::search::SearchView::Album(Box::new(
                crate::search::AlbumContext {
                    album: crate::api::models::Album {
                        id: 7,
                        title: "Arrival".into(),
                        songs: vec![mk(1, "One"), mk(2, "Two"), mk(3, "Three")],
                        ..crate::api::models::Album::default()
                    },
                    table: Default::default(),
                },
            ))],
        };
        a.update(AppEvent::Key(plain(KeyCode::Enter)));

        assert_eq!(a.queue.len(), 3);
        assert_eq!(a.queue.current().unwrap().1.song.id, 1);
        assert_eq!(a.queue.items[1].song.id, 2);
        assert_eq!(a.queue.items[2].song.id, 3);
        let cmd = loop {
            match arx.try_recv().expect("StreamSong queued") {
                c @ ApiCommand::StreamSong { .. } => break c,
                _ => continue,
            }
        };
        match cmd {
            ApiCommand::StreamSong {
                song,
                intent: StreamIntent::PlayNow,
            } => assert_eq!(song.id, 1),
            other => panic!("expected StreamSong, got {other:?}"),
        }
    }

    #[test]
    fn stream_cache_hit_skips_ticket_request() {
        let mut a = home();
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        let song = crate::api::models::Song {
            id: 42,
            title: "Cached".into(),
            ..crate::api::models::Song::default()
        };
        a.stream_cache
            .insert((42, 1), ("https://cached/url".into(), 1_000));
        a.now_unix = 1_100;
        a.stream_song(song, StreamIntent::PlayNow);

        assert!(
            arx.try_recv().is_err(),
            "cache hit must not hit the network"
        );
        assert_eq!(a.queue.current().unwrap().1.song.id, 42);
        assert_eq!(
            *a.stream_cache.get(&(42, 1)).unwrap(),
            ("https://cached/url".into(), 1_000)
        );
    }

    #[test]
    fn stream_cache_expires_past_ttl() {
        let mut a = home();
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        let song = crate::api::models::Song {
            id: 42,
            title: "Stale".into(),
            ..crate::api::models::Song::default()
        };
        a.stream_cache
            .insert((42, 1), ("https://stale/url".into(), 1_000));
        a.now_unix = 1_000 + TICKET_TTL_SECS;
        a.stream_song(song, StreamIntent::PlayNow);

        assert!(matches!(arx.try_recv(), Ok(ApiCommand::StreamSong { .. })));
        assert!(a.stream_cache.get(&(42, 1)).is_none());
    }

    #[test]
    fn end_of_file_error_invalidates_cached_ticket() {
        let mut a = home();
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        let song = crate::api::models::Song {
            id: 42,
            title: "Doomed".into(),
            ..crate::api::models::Song::default()
        };
        a.stream_cache
            .insert((42, 1), ("https://x/manifest.m3u8".into(), 1_000));
        a.now_unix = 1_100;
        a.queue.play_now(song.clone());
        a.update(AppEvent::Player(crate::player::PlayerEvent::EndFile {
            reason: "error".into(),
        }));

        assert!(a.stream_cache.get(&(42, 1)).is_none());
        a.now_unix = 1_200;
        a.stream_song(song.clone(), StreamIntent::Reload);
        assert!(matches!(arx.try_recv(), Ok(ApiCommand::StreamSong { .. })));
    }

    #[test]
    fn loop_one_replays_current_on_eof_without_advancing() {
        let mut a = home();
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        let mk = |id: u64, title: &str| crate::api::models::Song {
            id,
            title: title.into(),
            ..crate::api::models::Song::default()
        };
        a.update(AppEvent::Key(plain(KeyCode::Char('l'))));
        assert!(a.loop_one, "l toggles loop on");
        let (ptx, mut prx) = tokio::sync::mpsc::channel(16);
        a.connect_player(ptx);

        a.queue.play_now(mk(1, "One"));
        a.stream_song(mk(1, "One"), StreamIntent::PlayNow);
        loop {
            match arx.try_recv() {
                Ok(ApiCommand::StreamSong { .. }) => break,
                Ok(_) => continue,
                Err(_) => panic!("expected StreamSong"),
            }
        }
        a.update(AppEvent::Api(ApiEvent::StreamReady {
            song: mk(1, "One"),
            intent: StreamIntent::PlayNow,
            result: Ok(crate::api::client::StreamTicket {
                stream_url: "https://x/manifest.m3u8".into(),
                time_remaining: None,
            }),
        }));

        a.update(AppEvent::Player(crate::player::PlayerEvent::EndFile {
            reason: "eof".into(),
        }));
        assert_eq!(
            a.queue.current().unwrap().1.song.id,
            1,
            "loop keeps the track"
        );
        assert!(
            arx.try_recv().is_err(),
            "loop replay replays the cached ticket — no re-ticket request"
        );
        assert!(matches!(
            prx.try_recv(),
            Ok(crate::player::PlayerCommand::Load { .. })
        ));
        assert!(matches!(
            prx.try_recv(),
            Ok(crate::player::PlayerCommand::Load { .. })
        ));
    }

    #[test]
    fn playnow_records_history_and_error_unrecords() {
        let mut a = home();
        let _dir = with_local_history(&mut a);
        let (_prx, _arx) = (None::<u8>, None::<u8>);
        a.update(AppEvent::Api(ApiEvent::StreamReady {
            song: song(7),
            intent: StreamIntent::PlayNow,
            result: Ok(StreamTicket {
                stream_url: "https://x/manifest.m3u8".into(),
                time_remaining: None,
            }),
        }));
        assert_eq!(a.history.len(), 1);
        assert_eq!(a.history[0].title, "t7");
        a.update(AppEvent::Player(PlayerEvent::EndFile {
            reason: "error".into(),
        }));
        assert!(a.history.is_empty(), "mpv failure un-records the play");
        // A second play survives eof (normal finish keeps history).
        a.update(AppEvent::Api(ApiEvent::StreamReady {
            song: song(7),
            intent: StreamIntent::PlayNow,
            result: Ok(StreamTicket {
                stream_url: "https://x/manifest.m3u8".into(),
                time_remaining: None,
            }),
        }));
        assert_eq!(a.history.len(), 1);
        a.update(AppEvent::Player(PlayerEvent::EndFile {
            reason: "eof".into(),
        }));
        assert_eq!(a.history.len(), 1, "eof keeps the entry");
    }

    #[test]
    fn d_clears_history_after_confirm() {
        let mut a = home();
        let dir = with_local_history(&mut a);
        a.history = vec![crate::history::Entry {
            id: 5,
            provider: 1,
            title: "x".into(),
            ..crate::history::Entry::default()
        }];
        a.update(AppEvent::Key(plain(KeyCode::Char('4'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('D'))));
        assert!(a.clear_history_question, "modal open");
        a.update(AppEvent::Key(plain(KeyCode::Char('n'))));
        assert!(!a.clear_history_question);
        assert_eq!(a.history.len(), 1, "n cancels");
        a.update(AppEvent::Key(plain(KeyCode::Char('D'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('y'))));
        assert!(a.history.is_empty(), "y clears");
        assert!(
            dir.path().join("history.json").exists() == false
                || crate::history::load(&dir.path().join("history.json")).is_empty()
        );
        let _ = dir;
    }

    #[test]
    fn sqd_switch_tabs_from_launch_and_s_types() {
        let mut a = home();
        a.update(AppEvent::Key(plain(KeyCode::Char('q'))));
        assert_eq!(
            a.tab(),
            Tab::Queue,
            "letters switch tabs straight from launch"
        );
        a.update(AppEvent::Key(plain(KeyCode::Char('s'))));
        assert_eq!(a.tab(), Tab::Home);
        let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
            panic!("root expected")
        };
        assert_eq!(
            h.focus,
            crate::search::Focus::Input,
            "'s' from another tab lands focused on the box"
        );
        a.update(AppEvent::Key(plain(KeyCode::Char('q'))));
        let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
            panic!("root expected")
        };
        assert_eq!(
            h.input.value(),
            "q",
            "letters type while the box is focused"
        );
        assert_eq!(a.tab(), Tab::Home);
    }

    #[test]
    fn slash_focuses_search_box_from_anywhere() {
        let mut a = home();
        a.update(AppEvent::Key(plain(KeyCode::Char('2'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('/'))));
        assert_eq!(a.tab(), Tab::Home);
        let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
            panic!("root expected")
        };
        assert_eq!(h.focus, crate::search::Focus::Input);
        for c in ['q', 'u', 'e', 'e', 'n'] {
            a.update(AppEvent::Key(plain(KeyCode::Char(c))));
        }
        let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
            panic!("root expected")
        };
        assert_eq!(h.input.value(), "queen", "/ then typing goes into the box");
        a.update(AppEvent::Key(plain(KeyCode::Enter))); // dispatch, focus -> results
        a.update(AppEvent::Key(plain(KeyCode::Char('q'))));
        assert_eq!(a.tab(), Tab::Queue, "from results, q switches to Queue");
    }

    #[test]
    fn ctrl_q_and_ctrl_c_quit_anywhere() {
        let mut a = home();
        a.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
        )));
        assert!(a.should_quit());
        let mut a = app();
        a.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(a.should_quit());
    }

    #[test]
    fn wizard_typing_builds_preview() {
        let mut a = app();
        for c in "https://demo.freegalmusic.com/".chars() {
            a.update(AppEvent::Key(plain(KeyCode::Char(c))));
        }
        let w = wizard_mut(&mut a);
        assert_eq!(
            w.preview.as_deref(),
            Some("→ demo.freegalmusic.com"),
            "preview after typing full URL"
        );
    }

    #[test]
    fn wizard_escape_on_step1_quits() {
        let mut a = app();
        a.update(AppEvent::Key(plain(KeyCode::Esc)));
        assert!(a.should_quit());
    }

    #[test]
    fn wizard_enter_subdomain_sends_resolve_and_sets_busy() {
        let mut a = app();
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        a.connect_api(tx);
        for c in "demo".chars() {
            a.update(AppEvent::Key(plain(KeyCode::Char(c))));
        }
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        let w = wizard_mut(&mut a);
        assert!(w.busy, "wizard blocks until resolved");
        assert!(w.error.is_none());
        assert_eq!(
            w.subdomain_input.value(),
            "demo",
            "input normalized to bare subdomain"
        );
        drop(rx);
    }

    #[test]
    fn wizard_paste_of_card_url() {
        let mut a = app();
        a.update(AppEvent::Paste("https://demo.freegalmusic.com/".into()));
        let w = wizard_mut(&mut a);
        assert_eq!(
            w.preview.as_deref(),
            Some("→ demo.freegalmusic.com"),
            "paste normalizes like typing"
        );
        assert!(w.subdomain_input.value().contains("demo"));
    }

    #[test]
    fn library_resolved_advances_to_confirm() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::LibraryResolved {
            result: Ok(Library {
                id: 7,
                name: "Demo Public Library".into(),
            }),
        }));
        let w = wizard_mut(&mut a);
        assert_eq!(w.step, WizardStep::LibraryConfirm);
        assert!(!w.busy);
        assert_eq!(w.library.as_ref().unwrap().name, "Demo Public Library");
    }

    #[test]
    fn library_lookup_failure_opens_fallback_modal() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::LibraryResolved {
            result: Err(ApiError::Api {
                code: None,
                message: "not found".into(),
            }),
        }));
        let w = wizard_mut(&mut a);
        assert!(w.fallback.is_some(), "S0f fallback appears on failure");
        assert!(w.error.is_some());
    }

    #[test]
    fn fallback_enter_sends_set_library_id() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::LibraryResolved {
            result: Err(ApiError::Api {
                code: None,
                message: "nope".into(),
            }),
        }));
        for c in "42".chars() {
            a.update(AppEvent::Key(plain(KeyCode::Char(c))));
        }
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        let w = wizard_mut(&mut a);
        assert!(w.busy, "waiting for library id set");
    }

    #[test]
    fn fallback_non_numeric_error() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::LibraryResolved {
            result: Err(ApiError::Api {
                code: None,
                message: "nope".into(),
            }),
        }));
        for c in "abc".chars() {
            a.update(AppEvent::Key(plain(KeyCode::Char(c))));
        }
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        let w = wizard_mut(&mut a);
        assert_eq!(w.error.as_deref(), Some("library id must be a number"));
        assert!(w.fallback.is_some());
    }

    #[test]
    fn wizard_flow_confirm_to_pin() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::LibraryResolved {
            result: Ok(Library {
                id: 1,
                name: "L".into(),
            }),
        }));
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        for c in "12345".chars() {
            a.update(AppEvent::Key(plain(KeyCode::Char(c))));
        }
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        let w = wizard_mut(&mut a);
        assert_eq!(w.step, WizardStep::Pin);
        a.update(AppEvent::Key(plain(KeyCode::Esc)));
        let w = wizard_mut(&mut a);
        assert_eq!(w.step, WizardStep::Card, "Esc pops exactly one level");
    }

    #[test]
    fn pin_masks_length_not_value() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::LibraryResolved {
            result: Ok(Library {
                id: 1,
                name: "L".into(),
            }),
        }));
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        for c in "12345".chars() {
            a.update(AppEvent::Key(plain(KeyCode::Char(c))));
        }
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        for c in "secret".chars() {
            a.update(AppEvent::Key(plain(KeyCode::Char(c))));
        }
        let w = wizard_mut(&mut a);
        assert_eq!(
            w.pin_input.value().chars().count(),
            6,
            "PIN input holds the text"
        );
    }

    #[test]
    fn login_success_goes_home_with_credits() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::LoginResult {
            result: Ok(UserDetails {
                library: Some(Library {
                    id: 1,
                    name: "Demo Public Library".into(),
                }),
                download_limit: Some(3),
                available_download: Some(2),
                ..UserDetails::default()
            }),
        }));
        assert!(matches!(a.screen(), Screen::Home));
        assert_eq!(a.downloads_used, Some(1), "3 limit - 2 available = 1 used");
        assert_eq!(a.library_name.as_deref(), Some("Demo Public Library"));
    }

    #[test]
    fn login_failure_stays_on_wizard_with_message() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::LoginResult {
            result: Err(ApiError::Api {
                code: None,
                message: "Invalid PIN".into(),
            }),
        }));
        let w = wizard_mut(&mut a);
        assert_eq!(w.error.as_deref(), Some("Invalid PIN"));
        assert!(matches!(a.screen(), Screen::Login(_)));
    }

    #[test]
    fn toasts_cap_and_expire() {
        let mut a = app();
        let now = Instant::now();
        for i in 0..5 {
            a.toasts.push(now, ToastLevel::Info, format!("t{i}"));
        }
        assert_eq!(a.toasts.items.len(), TOAST_MAX);
        assert_eq!(a.toasts.items.last().unwrap().text, "t4");
    }

    use crate::api::client::StreamTicket;
    use crate::player::{PlayerCommand, PlayerEvent, PlayerStatus};
    use crate::queue::{Queue, QueueStatus};

    fn song(id: u64) -> crate::api::models::Song {
        crate::api::models::Song {
            id,
            title: format!("t{id}"),
            ..Default::default()
        }
    }

    fn connected_pair() -> (
        App,
        tokio::sync::mpsc::Receiver<PlayerCommand>,
        tokio::sync::mpsc::Receiver<ApiCommand>,
    ) {
        let mut a = app();
        let (ptx, prx) = tokio::sync::mpsc::channel(16);
        let (atx, arx) = tokio::sync::mpsc::channel(16);
        a.connect_player(ptx);
        a.connect_api(atx);
        (a, prx, arx)
    }

    #[test]
    fn queue_play_now_inserts_after_current() {
        let mut q = Queue::default();
        q.play_now(song_a(1));
        assert_eq!(current_pos(&q), Some(0));
        q.play_now(song_a(2));
        assert_eq!(current_pos(&q), Some(1));
        assert_eq!(q.items[0].status, QueueStatus::Played);
        assert_eq!(q.items[1].status, QueueStatus::Playing);
    }

    #[test]
    fn queue_play_at_marks_target_playing() {
        let mut q = Queue::default();
        q.append(song_a(1));
        q.append(song_a(2));
        q.append(song_a(3));
        q.play_at(2);
        assert_eq!(q.items[2].status, QueueStatus::Playing);
        q.play_at(0);
        assert_eq!(q.items[0].status, QueueStatus::Playing);
        assert_eq!(q.items[2].status, QueueStatus::Played);
        q.play_at(9);
        assert_eq!(q.items[0].status, QueueStatus::Playing);
    }

    fn song_a(id: u64) -> crate::api::models::Song {
        song(id)
    }

    fn current_pos(q: &Queue) -> Option<usize> {
        q.current().map(|(i, _)| i)
    }

    #[test]
    fn plus_minus_and_equal_adjust_volume() {
        let mut a = home();
        let (ptx, mut prx) = tokio::sync::mpsc::channel(16);
        a.connect_player(ptx);
        a.update(AppEvent::Key(plain(KeyCode::Char('4'))));
        for (key, delta) in [
            (KeyCode::Char('+'), 5),
            (KeyCode::Char('-'), -5),
            (KeyCode::Char('='), 5),
        ] {
            a.update(AppEvent::Key(plain(key)));
            match prx.try_recv().unwrap() {
                PlayerCommand::Volume { delta: d } => assert_eq!(d, delta),
                other => panic!("expected Volume, got {other:?}"),
            }
        }
    }

    #[test]
    fn player_keys_work_from_search_results() {
        let mut a = home();
        let (ptx, mut prx) = tokio::sync::mpsc::channel(16);
        a.connect_player(ptx);
        a.player_status.time = Some(10.0);
        a.update(AppEvent::Key(plain(KeyCode::Char('-'))));
        assert!(matches!(
            prx.try_recv().unwrap(),
            PlayerCommand::Volume { delta: -5 }
        ));
        a.update(AppEvent::Key(plain(KeyCode::Char('+'))));
        assert!(matches!(
            prx.try_recv().unwrap(),
            PlayerCommand::Volume { delta: 5 }
        ));
        a.update(AppEvent::Key(plain(KeyCode::Char('='))));
        assert!(matches!(
            prx.try_recv().unwrap(),
            PlayerCommand::Volume { delta: 5 }
        ));
        a.update(AppEvent::Key(plain(KeyCode::Char(','))));
        assert!(matches!(prx.try_recv().unwrap(), PlayerCommand::Seek { delta } if delta == -5.0));
        a.update(AppEvent::Key(plain(KeyCode::Char('.'))));
        assert!(matches!(prx.try_recv().unwrap(), PlayerCommand::Seek { delta } if delta == 5.0));
        a.update(AppEvent::Key(plain(KeyCode::Char(' '))));
        assert!(matches!(prx.try_recv().unwrap(), PlayerCommand::TogglePlay));
    }

    #[test]
    fn player_keys_type_into_search_input() {
        let mut a = home();
        let (ptx, mut prx) = tokio::sync::mpsc::channel(16);
        a.connect_player(ptx);
        a.update(AppEvent::Key(plain(KeyCode::Char('/'))));
        for c in ['a', 'c', '-', 'd', 'c'] {
            a.update(AppEvent::Key(plain(KeyCode::Char(c))));
        }
        let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
            panic!("root expected")
        };
        assert_eq!(h.input.value(), "ac-dc");
        assert!(
            prx.try_recv().is_err(),
            "typing never sends player commands"
        );
    }

    #[test]
    fn player_keys_type_into_history_filter() {
        let mut a = home();
        let _dir = with_local_history(&mut a);
        let (ptx, mut prx) = tokio::sync::mpsc::channel(16);
        a.connect_player(ptx);
        a.update(AppEvent::Key(plain(KeyCode::Char('4'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('/'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('-'))));
        a.update(AppEvent::Key(plain(KeyCode::Char(' '))));
        assert_eq!(a.history_input.value(), "- ");
        assert!(
            prx.try_recv().is_err(),
            "filter typing never sends player commands"
        );
    }

    #[test]
    fn space_toggles_playback_from_queue_tab() {
        let mut a = home();
        let (ptx, mut prx) = tokio::sync::mpsc::channel(16);
        a.connect_player(ptx);
        a.update(AppEvent::Key(plain(KeyCode::Char('2'))));
        a.update(AppEvent::Key(plain(KeyCode::Char(' '))));
        assert!(matches!(prx.try_recv().unwrap(), PlayerCommand::TogglePlay));
    }

    #[test]
    fn stream_ready_playnow_loads_player() {
        let (mut a, mut prx, _arx) = connected_pair();
        let s = song(7);
        a.update(AppEvent::Api(ApiEvent::StreamReady {
            song: s.clone(),
            intent: StreamIntent::PlayNow,
            result: Ok(StreamTicket {
                stream_url: "https://x/manifest.m3u8".into(),
                time_remaining: None,
            }),
        }));
        assert_eq!(a.queue.current().unwrap().1.song.id, 7);
        assert_eq!(a.queue.current().unwrap().1.status, QueueStatus::Playing);
        match prx.try_recv().unwrap() {
            PlayerCommand::Load { title, .. } => assert!(title.contains("t7")),
            other => panic!("expected Load, got {other:?}"),
        }
    }

    #[test]
    fn stream_ready_err_toasts_and_fails() {
        let (mut a, _prx, _arx) = connected_pair();
        a.queue.append(song(1));
        a.queue.items[0].status = QueueStatus::Playing;
        a.update(AppEvent::Api(ApiEvent::StreamReady {
            song: song(9),
            intent: StreamIntent::PlayNow,
            result: Err(ApiError::Api {
                code: Some(46),
                message: "blocked".into(),
            }),
        }));
        assert_eq!(a.queue.items[0].status, QueueStatus::Failed);
        assert!(!a.toasts.is_empty());
    }

    #[test]
    fn append_when_idle_starts_playback() {
        let (mut a, mut prx, _arx) = connected_pair();
        a.update(AppEvent::Api(ApiEvent::StreamReady {
            song: song(4),
            intent: StreamIntent::Append,
            result: Ok(StreamTicket {
                stream_url: "u".into(),
                time_remaining: None,
            }),
        }));
        assert_eq!(a.queue.current().unwrap().1.song.id, 4);
        assert!(prx.try_recv().is_ok());
    }

    #[test]
    fn track_end_advances_and_continues() {
        let (mut a, _prx, mut arx) = connected_pair();
        a.queue.append(song(1));
        a.queue.items[0].status = QueueStatus::Playing;
        a.queue.append(song(2));
        a.player_started = true;
        a.pending_stream_secs = 2.0;
        a.update(AppEvent::Player(PlayerEvent::EndFile {
            reason: "eof".into(),
        }));
        assert_eq!(a.queue.items[0].status, QueueStatus::Played);
        assert_eq!(a.queue.items[1].status, QueueStatus::Playing);
        assert_eq!(a.pending_stream_secs, 0.0);
        let mut saw_record = false;
        let mut saw_stream = false;
        while let Ok(cmd) = arx.try_recv() {
            match cmd {
                ApiCommand::RecordStreamTime {
                    song_id: 1, secs, ..
                } => {
                    assert!((secs - 2.0).abs() < 1e-9);
                    saw_record = true;
                }
                ApiCommand::StreamSong { song, intent } => {
                    assert_eq!(song.id, 2);
                    assert!(matches!(intent, StreamIntent::Reload));
                    saw_stream = true;
                }
                _ => {}
            }
        }
        assert!(saw_record);
        assert!(saw_stream);
    }

    #[test]
    fn track_error_fails_current() {
        let (mut a, _prx, _arx) = connected_pair();
        a.queue.append(song(1));
        a.queue.items[0].status = QueueStatus::Playing;
        a.update(AppEvent::Player(PlayerEvent::EndFile {
            reason: "error".into(),
        }));
        assert_eq!(a.queue.items[0].status, QueueStatus::Failed);
    }

    #[test]
    fn pause_flushes_stream_time() {
        let (mut a, _prx, mut arx) = connected_pair();
        a.queue.append(song(3));
        a.queue.items[0].status = QueueStatus::Playing;
        a.pending_stream_secs = 3.5;
        a.update(AppEvent::Player(PlayerEvent::Status {
            time: Some(30.0),
            duration: Some(200.0),
            paused: Some(true),
        }));
        assert_eq!(a.pending_stream_secs, 0.0);
        match arx.try_recv().unwrap() {
            ApiCommand::RecordStreamTime { song_id, secs, .. } => {
                assert_eq!(song_id, 3);
                assert!((secs - 3.5).abs() < 1e-9);
            }
            other => panic!("expected RecordStreamTime, got {other:?}"),
        }
    }

    #[test]
    fn tick_accumulates_played_seconds() {
        let (mut a, mut prx, _arx) = connected_pair();
        a.player_started = true;
        a.queue.append(song(5));
        a.queue.items[0].status = QueueStatus::Playing;
        a.player_status = PlayerStatus {
            time: Some(10.0),
            duration: Some(100.0),
            paused: Some(false),
        };
        a.tick();
        assert!(prx.try_recv().is_ok(), "GetStatus sent");
        a.update(AppEvent::Player(PlayerEvent::Status {
            time: Some(13.0),
            duration: None,
            paused: Some(false),
        }));
        a.tick();
        assert!(
            (a.pending_stream_secs - 3.0).abs() < 1e-9,
            "pending={}",
            a.pending_stream_secs
        );
    }

    #[test]
    fn stream_time_recorded_updates_header() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::StreamTimeRecorded {
            result: Ok(Some(StreamTime::Unlimited("UNLIMITED".into()))),
        }));
        assert_eq!(
            a.stream_time_remaining,
            Some(StreamTime::Unlimited("UNLIMITED".into()))
        );
    }

    fn modal_app() -> (App, tokio::sync::mpsc::Receiver<ApiCommand>) {
        let mut a = search_rows_app();
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        a.connect_api(tx);
        match a.search.top_mut() {
            Some(crate::search::SearchView::Root(h)) => {
                h.focus = crate::search::Focus::Results;
                h.table.select(Some(0));
            }
            _ => panic!("root view expected"),
        }
        (a, rx)
    }

    #[test]
    fn y_opens_download_modal() {
        let (mut a, arx) = modal_app();
        a.update(AppEvent::Key(plain(KeyCode::Char('y'))));
        assert!(
            a.download_confirm.is_some(),
            "modal open after y on song row"
        );
        let _ = arx;
    }

    #[test]
    fn backspace_deletes_search_text_and_requeries() {
        let mut a = search_rows_app();
        if let Some(crate::search::SearchView::Root(h)) = a.search.top_mut() {
            h.input = tui_input::Input::default();
        }
        a.update(AppEvent::Key(plain(KeyCode::Char('/'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('a'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('b'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('c'))));
        let (value, gen) = {
            let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
                panic!("root expected")
            };
            (h.input.value().to_string(), h.gen)
        };
        assert_eq!(value, "abc");
        assert_eq!(gen, 4, "three inserts bump gen once each");
        a.update(AppEvent::Key(plain(KeyCode::Backspace)));
        let (value, focus, gen) = {
            let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
                panic!("root expected")
            };
            (h.input.value().to_string(), h.focus, h.gen)
        };
        assert_eq!(value, "ab");
        assert_eq!(gen, 5, "delete requeries (on_input_changed)");
        assert_eq!(focus, crate::search::Focus::Input);
        match &mut a.search.top_mut() {
            Some(crate::search::SearchView::Root(h)) => h.focus = crate::search::Focus::Results,
            _ => panic!("root expected"),
        }
        a.update(AppEvent::Key(plain(KeyCode::Backspace)));
        let (value, focus, gen) = {
            let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
                panic!("root expected")
            };
            (h.input.value().to_string(), h.focus, h.gen)
        };
        assert_eq!(value, "a");
        assert_eq!(
            focus,
            crate::search::Focus::Input,
            "backspace from results refocuses input"
        );
        assert_eq!(gen, 6);
    }

    #[test]
    fn s_types_in_search_input() {
        let mut a = search_rows_app();
        if let Some(crate::search::SearchView::Root(h)) = a.search.top_mut() {
            h.input = tui_input::Input::default();
        }
        a.update(AppEvent::Key(plain(KeyCode::Char('/'))));
        a.update(AppEvent::Key(plain(KeyCode::Char('S'))));
        let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
            panic!("root expected")
        };
        assert_eq!(h.input.value(), "S", "S types while the input is focused");
        assert_eq!(h.sort, crate::search::SortMode::Best);
        assert!(a.toasts.items.is_empty(), "no sort toast while typing");
    }

    #[test]
    fn s_at_results_focus_cycles_sort() {
        let (mut a, arx) = modal_app();
        a.update(AppEvent::Key(plain(KeyCode::Char('S'))));
        let Some(crate::search::SearchView::Root(h)) = a.search.top() else {
            panic!("root expected")
        };
        assert_eq!(h.sort, crate::search::SortMode::TitleAZ);
        assert!(a
            .toasts
            .items
            .iter()
            .any(|t| t.text.contains("sort: title A→Z")));
        let first = h.rows()[0].title().to_string();
        let _ = arx;
        assert_eq!(first, "Instrumental", "rows visibly reordered");
    }

    #[test]
    fn confirm_y_sends_download_song() {
        let (mut a, mut arx) = modal_app();
        a.update(AppEvent::Key(plain(KeyCode::Char('y'))));
        let song = a.download_confirm.clone().expect("modal open");
        a.update(AppEvent::Key(plain(KeyCode::Char('y'))));
        assert!(a.download_confirm.is_none(), "confirm closes modal");
        let cmd = arx.try_recv().expect("DownloadSong queued");
        assert!(matches!(
            cmd,
            ApiCommand::DownloadSong {
                redownload: false,
                ..
            }
        ));
        assert_eq!(
            cmd,
            ApiCommand::DownloadSong {
                song: song.clone(),
                redownload: false
            }
        );
    }

    #[test]
    fn modal_enter_cancels() {
        let (mut a, mut arx) = modal_app();
        a.update(AppEvent::Key(plain(KeyCode::Char('y'))));
        assert!(a.download_confirm.is_some());
        a.update(AppEvent::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(a.download_confirm.is_none());
        assert!(
            arx.try_recv().is_err(),
            "Enter is cancel — no DownloadSong may be sent"
        );
    }

    #[test]
    fn modal_n_cancels() {
        let (mut a, arx) = modal_app();
        a.update(AppEvent::Key(plain(KeyCode::Char('y'))));
        assert!(a.download_confirm.is_some());
        a.update(AppEvent::Key(plain(KeyCode::Char('n'))));
        assert!(a.download_confirm.is_none());
        let _ = arx;
    }

    #[test]
    fn eligible_song_redownloads_without_credit() {
        let (mut a, mut arx) = modal_app();
        a.update(AppEvent::Key(plain(KeyCode::Char('y'))));
        let song = a.download_confirm.clone().expect("modal open");
        a.downloaded_ids.push(song.id);
        a.update(AppEvent::Key(plain(KeyCode::Char('y'))));
        let cmd = arx.try_recv().expect("DownloadSong queued");
        assert_eq!(
            cmd,
            ApiCommand::DownloadSong {
                song,
                redownload: true
            }
        );
    }

    #[test]
    fn download_complete_pushes_row_and_refreshes_credits() {
        let mut a = app();
        let details = crate::api::client::UserDetails {
            download_limit: Some(5),
            available_download: Some(3),
            ..Default::default()
        };
        a.update(AppEvent::Api(ApiEvent::DownloadComplete {
            result: Ok(DownloadOutcome {
                song_id: 7,
                title: "t7".into(),
                artist: "a".into(),
                file_name: "a - t7.mp3".into(),
                details: Some(details),
            }),
        }));
        assert_eq!(a.downloads.len(), 1);
        assert_eq!(a.downloads[0].file_name, "a - t7.mp3");
        assert!(a.downloaded_ids.contains(&7));
        assert_eq!(
            a.downloads_used,
            Some(2),
            "3 limit… 5 limit - 3 available = 2 used"
        );
        assert!(!a.toasts.is_empty(), "saved toast");
    }

    #[test]
    fn download_error_toasts_without_row() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::DownloadComplete {
            result: Err(ApiError::Network("disk full".into())),
        }));
        assert!(a.downloads.is_empty());
        assert!(!a.toasts.is_empty());
    }

    #[test]
    fn downloaded_ids_event_sets_list() {
        let mut a = app();
        a.update(AppEvent::Api(ApiEvent::DownloadedIds {
            result: Ok(vec![1, 2]),
        }));
        assert_eq!(a.downloaded_ids, vec![1, 2]);
    }

    #[test]
    fn mouse_scroll_moves_queue_selection() {
        let mut a = home();
        a.set_tab(Tab::Queue);
        a.queue.append(song(1));
        a.queue.append(song(2));
        a.queue_table.select(Some(0));
        a.update(AppEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 10,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(a.queue_table.selected(), Some(1));
        a.update(AppEvent::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 10,
            row: 10,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(a.queue_table.selected(), Some(0));
    }

    #[test]
    fn mouse_click_selects_and_double_click_plays_queue_row() {
        let mut a = home();
        a.set_tab(Tab::Queue);
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        a.queue.append(song(1));
        a.queue.append(song(2));
        a.queue.append(song(3));
        a.click_zones.push((
            Rect::new(0, 10, 80, 6),
            ClickZone::Rows(RowArea::Queue, true),
        ));
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 13,
            modifiers: KeyModifiers::NONE,
        };
        a.update(AppEvent::Mouse(click));
        assert_eq!(a.queue_table.selected(), Some(2));
        a.update(AppEvent::Mouse(click));
        assert!(matches!(arx.try_recv(), Ok(ApiCommand::StreamSong { .. })));
    }

    #[test]
    fn mouse_click_switches_tab_and_triggers_fetch() {
        let mut a = home();
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        a.click_zones
            .push((Rect::new(0, 0, 80, 1), ClickZone::Tab(4)));
        a.update(AppEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 30,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(a.tab(), Tab::Downloads);
        assert!(matches!(arx.try_recv(), Ok(ApiCommand::GetDownloadedIds)));
    }

    #[test]
    fn mouse_click_on_progress_seeks_to_fraction() {
        let mut a = home();
        let (ptx, mut prx) = tokio::sync::mpsc::channel(16);
        a.connect_player(ptx);
        a.player_status.duration = Some(100.0);
        a.click_zones
            .push((Rect::new(0, 21, 80, 1), ClickZone::Progress));
        a.update(AppEvent::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 40,
            row: 21,
            modifiers: KeyModifiers::NONE,
        }));
        match prx.try_recv().ok().expect("seek command on player channel") {
            crate::player::PlayerCommand::SeekTo { secs } => {
                assert!((secs - 51.25).abs() < 0.01)
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn downloads_tab_open_requests_eligibility() {
        let mut a = home();
        let (atx, mut arx) = tokio::sync::mpsc::channel(16);
        a.connect_api(atx);
        a.update(AppEvent::Key(plain(KeyCode::Char('5'))));
        assert_eq!(a.tab(), Tab::Downloads);
        assert!(matches!(arx.try_recv(), Ok(ApiCommand::GetDownloadedIds)));
    }

    #[test]
    fn help_opens_and_closes_without_quitting() {
        let (mut a, arx) = modal_app();
        let _ = arx;
        a.update(AppEvent::Key(plain(KeyCode::Char('?'))));
        assert!(a.help_open);
        a.update(AppEvent::Key(plain(KeyCode::Esc)));
        assert!(!a.help_open);
        a.update(AppEvent::Key(plain(KeyCode::Char('?'))));
        assert!(a.help_open);
        a.update(AppEvent::Key(plain(KeyCode::Char('q'))));
        assert!(!a.help_open, "q closes the help overlay");
        assert!(!a.should_quit(), "q under an overlay must not quit");
    }

    #[test]
    fn errors_overlay_opens_and_closes() {
        let (mut a, arx) = modal_app();
        let _ = arx;
        a.update(AppEvent::Key(plain(KeyCode::Char('e'))));
        assert!(a.errors_open);
        a.update(AppEvent::Key(plain(KeyCode::Esc)));
        assert!(!a.errors_open);
    }

    #[test]
    fn error_ring_caps_at_100() {
        let mut a = home();
        for i in 0..110 {
            a.toast_error(format!("boom {i}"));
        }
        assert_eq!(a.errors.len(), 100);
    }

    #[test]
    fn toast_error_records_message() {
        let mut a = home();
        a.toast_error("boom");
        assert_eq!(a.errors[0].message, "boom");
        assert_eq!(a.errors[0].code, None);
    }

    #[test]
    fn o_opens_audio_menu_and_requests_devices() {
        let (mut a, mut prx, _arx) = connected_pair();
        a.make_home();
        a.update(AppEvent::Key(plain(KeyCode::Tab)));
        a.update(AppEvent::Key(plain(KeyCode::Tab)));
        a.update(AppEvent::Key(plain(KeyCode::Char('o'))));
        assert!(a.audio_menu_open);
        assert!(matches!(
            prx.try_recv(),
            Ok(crate::player::PlayerCommand::AudioDevices)
        ));
    }

    #[test]
    fn audio_devices_event_populates_list() {
        let mut a = app();
        a.update(AppEvent::Player(crate::player::PlayerEvent::AudioDevices {
            devices: vec![("auto".into(), "Auto".into())],
        }));
        assert_eq!(
            a.audio_devices,
            vec![("auto".to_string(), "Auto".to_string())]
        );
    }

    #[test]
    fn audio_menu_enter_selects_and_closes() {
        let (mut a, mut prx, _arx) = connected_pair();
        a.make_home();
        a.audio_menu_open = true;
        a.audio_devices = vec![("a".into(), "A".into()), ("b".into(), "B".into())];
        a.audio_menu_table.select(Some(1));
        a.update(AppEvent::Key(plain(KeyCode::Enter)));
        assert!(!a.audio_menu_open);
        assert!(matches!(
            prx.try_recv(),
            Ok(crate::player::PlayerCommand::SetAudioDevice { ref device }) if device == "b"
        ));
    }

    #[test]
    fn audio_menu_esc_closes_without_command() {
        let (mut a, mut prx, _arx) = connected_pair();
        a.make_home();
        a.audio_menu_open = true;
        a.update(AppEvent::Key(plain(KeyCode::Esc)));
        assert!(!a.audio_menu_open);
        assert!(prx.try_recv().is_err());
    }
}
