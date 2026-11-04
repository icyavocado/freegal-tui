mod api;
mod app;
mod barstatus;
mod history;
mod home;
mod mymusic;
mod player;
mod queue;
mod search;
mod ui;

use std::io;
use std::panic;
use std::time::Duration;

use crate::api::{creds, task as api_task};
use crate::app::{ApiCommand, App, AppEvent};
use crate::player::{socket_path, PlayerCommand};
use crate::search::{sent_gen_cell, SearchQuery};
use anyhow::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    KeyEventKind,
};

const TICK_MS: u64 = 250;

fn main() -> Result<()> {
    init_logging();
    crate::ui::theme::init_from_omarchy();

    let runtime = std::sync::Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?,
    );
    let creds_path = creds::creds_path().ok();
    let creds = creds_path
        .as_deref()
        .and_then(|p| creds::load(p).ok())
        .flatten()
        .unwrap_or_default();
    let mut app = App::new(Some(&creds));
    app.history_path = crate::history::history_path();
    app.history = crate::history::load(&app.history_path);

    let (api_cmd_tx, api_cmd_rx) = tokio::sync::mpsc::channel::<ApiCommand>(16);
    app.connect_api(api_cmd_tx.clone());
    if app.restoring {
        let _ = api_cmd_tx.try_send(ApiCommand::Restore);
    }

    let (search_watch_tx, search_watch_rx) = tokio::sync::watch::channel(SearchQuery::default());
    let sent_gen = sent_gen_cell();
    app.connect_search_debounce(search_watch_tx, sent_gen.clone());

    let (player_tx, player_rx) = tokio::sync::mpsc::channel::<PlayerCommand>(16);
    app.connect_player(player_tx.clone());
    let library_domain = creds
        .subdomain
        .trim()
        .rsplit_once('.')
        .map(|(head, _)| head)
        .unwrap_or(creds.subdomain.as_str());
    let library_domain = if library_domain.is_empty() {
        String::new()
    } else {
        format!("https://{library_domain}.freegalmusic.com")
    };

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let task_events = event_tx.clone();
    let runtime_thread = {
        let runtime = runtime.clone();
        std::thread::Builder::new()
            .name("tokio-runtime".into())
            .spawn(move || {
                runtime.block_on(async move {
                    tokio::spawn(api_task::run(
                        api_cmd_rx,
                        task_events.clone(),
                        creds.clone(),
                        creds_path.clone(),
                    ));
                    tokio::spawn(api_task::run_search_debounce(
                        search_watch_rx,
                        api_cmd_tx.clone(),
                        task_events.clone(),
                        sent_gen.clone(),
                    ));
                    tokio::spawn(crate::player::run(
                        player_rx,
                        task_events.clone(),
                        socket_path(),
                        library_domain,
                    ));
                    shutdown_rx.recv().await;
                });
            })?
    };

    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
        default_hook(info);
    }));

    let result = run_tui(&mut app, &mut event_rx);

    let _ = crossterm::execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();

    drop(event_tx);
    drop(shutdown_tx);
    let _ = runtime_thread.join();
    std::sync::Arc::into_inner(runtime)
        .expect("runtime thread joined; worker clones dropped")
        .shutdown_background();

    result
}

fn run_tui(
    app: &mut App,
    event_rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Result<()> {
    let mut terminal = ratatui::try_init()?;
    crossterm::execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;

    loop {
        terminal.draw(|f| ui::render(app, f))?;

        if !crossterm::event::poll(Duration::from_millis(TICK_MS))? {
            app.tick();
        }

        while crossterm::event::poll(Duration::ZERO)? {
            match crossterm::event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.update(AppEvent::Key(key));
                }
                Event::Paste(text) => app.update(AppEvent::Paste(text)),
                Event::Mouse(me) => app.update(AppEvent::Mouse(me)),
                _ => {}
            }
        }

        while let Ok(event) = event_rx.try_recv() {
            app.update(event);
        }

        if app.should_quit() {
            break;
        }
    }
    barstatus::clear();
    Ok(())
}

fn init_logging() {
    let Some(state_dir) = dirs::state_dir().map(|d| d.join("freegal")) else {
        return;
    };
    if std::fs::create_dir_all(&state_dir).is_err() {
        return;
    }
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state_dir.join("freegal.log"))
    else {
        return;
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(filter)
        .with_writer(std::sync::Mutex::new(file))
        .try_init();
}
