use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;

use crate::app::{App, ClickZone, RowArea};
use crate::home::{HomeFeedData, HomeFocus, HomeRow, HomeView, SIDEBAR_MIN_WIDTH};
use crate::ui::chrome::{self, accent, dim, strong};
use crate::ui::search::{fmt_secs, render_input};

pub fn render(app: &mut App, area: Rect, f: &mut Frame) {
    app.home.last_width = area.width;
    if area.height == 0 {
        return;
    }
    let in_root = matches!(app.home.view, HomeView::Root);
    let focused = matches!(
        app.search.top(),
        Some(crate::search::SearchView::Root(h)) if h.focus == crate::search::Focus::Input
    );
    let panes = if in_root && (area.height >= 12 || focused) {
        let chips_h = chip_lines(area.width).len().max(1) as u16;
        let [strip, _gap, chips, _gap2, rest] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(chips_h),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(area);
        render_launcher(app, strip, f, false);
        render_chips(app, chips, f);
        rest
    } else {
        area
    };
    if panes.width >= SIDEBAR_MIN_WIDTH {
        let [side, _gap, main] = Layout::horizontal([
            Constraint::Length(24),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(panes);
        render_curated(app, side, f);
        render_main(app, main, f);
    } else {
        render_main(app, panes, f);
    }
}

fn render_curated(app: &mut App, area: Rect, f: &mut Frame) {
    let focused = app.home.focus == HomeFocus::Curated;
    let mut block = chrome::card("library playlists");
    if focused {
        block = block.border_style(accent());
    }
    f.render_widget(&block, area);
    let inner = block.inner(area);
    if inner.height == 0 {
        return;
    }
    if app.home.curated_len() == 0 {
        let text = if app.home.busy() || !app.home.curated_ok() {
            "  loading…"
        } else {
            "  no lists yet"
        };
        f.render_widget(Paragraph::new(Span::styled(text, dim())), inner);
        return;
    }
    let rows: Vec<Row> = match &app.home.curated().data {
        HomeFeedData::Playlists(page) => page
            .items
            .iter()
            .map(|p| {
                Row::new(vec![Cell::from(Line::from(vec![
                    Span::styled(p.name.clone(), strong()),
                    Span::styled(format!(" ({})", p.songs_count), dim()),
                ]))])
            })
            .collect(),
        _ => vec![],
    };
    let table = Table::new(rows, [Constraint::Fill(1)]);
    let highlight = if focused {
        chrome::selected()
    } else {
        Style::new()
    };
    app.click_zones
        .push((inner, ClickZone::Rows(RowArea::HomeCurated, false)));
    f.render_stateful_widget(
        table.row_highlight_style(highlight),
        inner,
        &mut app.home.curated_table,
    );
}

fn render_main(app: &mut App, area: Rect, f: &mut Frame) {
    if app.home.is_root() {
        render_drill(app, area, f);
        return;
    }
    if area.height == 0 {
        return;
    }
    let [crumb, rest] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    if crumb.height > 0 {
        render_breadcrumb(app, crumb, f);
    }
    if rest.height == 0 {
        return;
    }
    render_drill(app, rest, f);
}

fn render_breadcrumb(app: &mut App, area: Rect, f: &mut Frame) {
    let crumbs = app.home.crumbs.clone();
    let mut x = area.x.saturating_add(1);
    let limit = area.x + area.width;
    let last = crumbs.len().saturating_sub(1);
    for (i, label) in crumbs.iter().enumerate() {
        let style = if i == last { strong() } else { dim() };
        let count = label.chars().count() as u16;
        if count > 0 {
            let rect = Rect::new(x, area.y, count.min(limit.saturating_sub(x)), 1);
            if i < last {
                app.click_zones.push((rect, ClickZone::HomeCrumb(i)));
            }
            f.render_widget(Paragraph::new(Span::styled(label.clone(), style)), rect);
            x = x.saturating_add(count);
        }
        if i < last {
            let sep = Rect::new(x, area.y, 3.min(limit.saturating_sub(x)), 1);
            f.render_widget(
                Paragraph::new(Span::styled(" \u{203a} ".to_string(), dim())),
                sep,
            );
            x = x.saturating_add(3);
        }
        if x >= limit {
            break;
        }
    }
}

fn render_drill(app: &mut App, area: Rect, f: &mut Frame) {
    match &mut app.home.view {
        HomeView::Root => render_sections(app, area, f),
        HomeView::Genres { page, state, table } => {
            if area.height > 1 {
                app.click_zones.push((
                    Rect::new(area.x, area.y, area.width, area.height - 1),
                    ClickZone::Rows(RowArea::HomeGenres, false),
                ));
            }
            render_genres(page, state, table, area, f);
        }
        HomeView::Detail(_) => render_genre_detail(app, area, f),
        HomeView::Album(ctx) => {
            let mut zones = std::mem::take(&mut app.click_zones);
            crate::ui::search::render_album(ctx, area, f, &mut zones, RowArea::HomeAlbum);
            app.click_zones = zones;
        }
        HomeView::Loading { state } => render_loading(state, area, f),
        HomeView::List {
            songs,
            state,
            table,
        } => {
            if area.height > 1 {
                app.click_zones.push((
                    Rect::new(area.x, area.y, area.width, area.height - 1),
                    ClickZone::Rows(RowArea::HomeList, false),
                ));
            }
            render_song_list(songs, state, table, area, f);
        }
        HomeView::Albums {
            albums,
            state,
            table,
        } => {
            if area.height > 1 {
                app.click_zones.push((
                    Rect::new(area.x, area.y, area.width, area.height - 1),
                    ClickZone::Rows(RowArea::HomeAlbums, false),
                ));
            }
            render_album_list(albums, state, table, area, f);
        }
        HomeView::Playlists {
            playlists,
            state,
            table,
        } => {
            if area.height > 1 {
                app.click_zones.push((
                    Rect::new(area.x, area.y, area.width, area.height - 1),
                    ClickZone::Rows(RowArea::HomePlaylists, false),
                ));
            }
            render_playlist_list(playlists, state, table, area, f);
        }
        HomeView::Artists {
            artists,
            state,
            table,
        } => {
            if area.height > 1 {
                app.click_zones.push((
                    Rect::new(area.x, area.y, area.width, area.height - 1),
                    ClickZone::Rows(RowArea::HomeArtists, false),
                ));
            }
            render_artist_list(artists, state, table, area, f);
        }
    }
}

fn render_sections(app: &mut App, area: Rect, f: &mut Frame) {
    let [main, status_row] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    render_status(app, status_row, f);
    render_feed_table(app, main, f);
}

pub(crate) fn render_launcher(app: &mut App, area: Rect, f: &mut Frame, jump_search: bool) {
    let focused = matches!(
        app.search.top(),
        Some(crate::search::SearchView::Root(h)) if h.focus == crate::search::Focus::Input
    );
    app.click_zones.push((
        area,
        if jump_search {
            ClickZone::JumpSearch
        } else {
            ClickZone::SearchInput
        },
    ));
    let block = chrome::input_box(focused);
    f.render_widget(&block, area);
    if let Some(crate::search::SearchView::Root(h)) = app.search.top_mut() {
        let inner = area.inner(ratatui::layout::Margin::new(0, 1));
        render_input(
            &mut h.input,
            focused,
            inner,
            f,
            " 🔍 Search songs, artists, albums — press / to type",
        );
    }
}

fn chip_w(label: &str) -> u16 {
    4 + label.chars().count() as u16
}

fn chip_lines(width: u16) -> Vec<Vec<usize>> {
    if width == 0 {
        return vec![vec![]];
    }
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut x = 0u16;
    for (i, (label, _)) in crate::home::quick_links().iter().enumerate() {
        let w = chip_w(label);
        if x.saturating_add(w) > width && !lines.last().unwrap().is_empty() {
            lines.push(Vec::new());
            x = 0;
        }
        lines.last_mut().unwrap().push(i);
        x = x.saturating_add(w).saturating_add(2);
    }
    lines
}

fn render_chips(app: &mut App, area: Rect, f: &mut Frame) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let links = crate::home::quick_links();
    let lines = chip_lines(area.width);
    let spans_per_line: Vec<Vec<Span>> = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|&i| {
                    let (label, _) = links[i];
                    Span::from(format!("[→ {label}]"))
                })
                .collect()
        })
        .collect();
    for (li, spans) in spans_per_line.iter().enumerate() {
        let mut x = area.x;
        for (ci, span) in spans.iter().enumerate() {
            let idx = lines[li][ci];
            let (label, _) = links[idx];
            let w = chip_w(label).min(area.right().saturating_sub(x));
            if w > 0 {
                let rect = Rect::new(x, area.y + li as u16, w, 1);
                let active =
                    app.home.focus == crate::home::HomeFocus::Links && idx == app.home.link_sel;
                let style = if active {
                    chrome::accent().add_modifier(ratatui::style::Modifier::BOLD)
                } else {
                    dim()
                };
                app.click_zones.push((rect, ClickZone::Chip(idx)));
                f.render_widget(
                    Paragraph::new(Span::styled(span.content.clone(), style)),
                    rect,
                );
            }
            x = x.saturating_add(chip_w(label)).saturating_add(2);
        }
    }
}

fn render_status(app: &App, row: Rect, f: &mut Frame) {
    let text = if app.home.busy() {
        "loading…".to_string()
    } else if let Some(fail) = failed_section(app) {
        format!("Error: {fail} — R to retry")
    } else {
        String::new()
    };
    if text.is_empty() {
        f.render_widget(Paragraph::new(Line::default()), row);
    } else {
        let style = if text.starts_with("Error") {
            chrome::error_style()
        } else {
            dim()
        };
        f.render_widget(Paragraph::new(Span::styled(text, style)), row);
    }
}

fn failed_section(app: &App) -> Option<String> {
    app.home.sections.iter().find_map(|s| match &s.state {
        crate::search::Load::Failed(e) => Some(format!("{}: {e}", s.title)),
        _ => None,
    })
}

fn render_feed_table(app: &mut App, area: Rect, f: &mut Frame) {
    if area.height == 0 {
        return;
    }
    app.click_zones
        .push((area, ClickZone::Rows(RowArea::HomeSections, false)));
    let rows = owned_section_rows(app);
    if rows.is_empty() {
        let text = if app.home.busy() {
            ""
        } else {
            "press R to load the home feeds"
        };
        if !text.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(text, dim()))
                    .alignment(ratatui::layout::Alignment::Center),
                area,
            );
        }
        return;
    }
    let spacing: u16 = if area.width >= 64 { 2 } else { 1 };
    let active = app.home.focus == HomeFocus::Sections;
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Percentage(46),
            Constraint::Percentage(30),
            Constraint::Length(6),
        ],
    )
    .column_spacing(spacing)
    .row_highlight_style(if active {
        chrome::selected()
    } else {
        Style::new()
    });
    f.render_stateful_widget(table, area, &mut app.home.sections_table);
}

fn owned_section_rows(app: &App) -> Vec<Row<'static>> {
    let mut rows = Vec::new();
    let mut item_no: usize = 0;
    for r in app.home.home_rows() {
        match r {
            HomeRow::Head(_, label) => {
                rows.push(head_row(label));
            }
            HomeRow::Song(s) => {
                item_no += 1;
                rows.push(Row::new(vec![
                    Cell::from(Span::styled(item_no.to_string(), dim())),
                    Cell::from(Span::styled(s.title.clone(), strong())),
                    Cell::from(Span::styled(artist_of_song(s), dim())),
                    Cell::from(Span::styled(fmt_secs(s.full_duration_sec), dim())),
                ]));
            }
            HomeRow::Album(a) => {
                item_no += 1;
                rows.push(card_row(
                    item_no,
                    a.title.clone(),
                    a.artist
                        .as_ref()
                        .map(|x| x.name.clone())
                        .unwrap_or_default(),
                    a.year.map(|y| y.to_string()).unwrap_or_default(),
                ));
            }
            HomeRow::Playlist(p) => {
                item_no += 1;
                rows.push(card_row(
                    item_no,
                    p.name.clone(),
                    p.description.clone(),
                    format!("{} songs", p.songs_count),
                ));
            }
            HomeRow::Artist(a) => {
                item_no += 1;
                rows.push(card_row(
                    item_no,
                    a.name.clone(),
                    a.label.clone().unwrap_or_default(),
                    String::new(),
                ));
            }
        }
    }
    rows
}

pub(crate) fn head_row(label: &str) -> Row<'static> {
    Row::new(vec![
        Cell::from(""),
        Cell::from(Line::from(vec![
            Span::styled("→ ", accent()),
            Span::styled(label.to_string(), strong()),
        ])),
        Cell::from(""),
        Cell::from(""),
    ])
}

pub(crate) fn song_row(no: usize, s: &crate::api::models::Song) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(no.to_string(), dim())),
        Cell::from(Span::styled(s.title.clone(), strong())),
        Cell::from(Span::styled(artist_of_song(s), dim())),
        Cell::from(Span::styled(fmt_secs(s.full_duration_sec), dim())),
    ])
}

fn render_genres(
    page: &crate::api::models::Page<crate::api::models::Genre>,
    state: &crate::search::Load,
    table: &mut TableState,
    area: Rect,
    f: &mut Frame,
) {
    if area.height == 0 {
        return;
    }
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    if body.height > 0 {
        if page.items.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    if matches!(state, crate::search::Load::Loading) {
                        "  loading…"
                    } else {
                        "  no genres known to this library"
                    },
                    dim(),
                )),
                body,
            );
        } else {
            let rows: Vec<Row> = page
                .items
                .iter()
                .map(|g| {
                    Row::new(vec![
                        Cell::from(Span::styled("●", crate::ui::chrome::genre_dot(&g.title))),
                        Cell::from(Span::styled(g.title.to_uppercase(), strong())),
                    ])
                })
                .collect();
            let view = Table::new(rows, [Constraint::Length(3), Constraint::Fill(1)])
                .column_spacing(2)
                .row_highlight_style(chrome::selected());
            f.render_stateful_widget(view, body, table);
        }
    }
    render_list_status(state, page.items.len(), "genres", "Esc: back  ", status, f);
}

fn render_genre_detail(app: &mut App, area: Rect, f: &mut Frame) {
    if area.height == 0 {
        return;
    }
    let (rows, count) = {
        let crate::home::HomeView::Detail(d) = &app.home.view else {
            return;
        };
        let rows = owned_detail_rows(d);
        let count = crate::home::detail_rows_len(d);
        (rows, count)
    };
    if area.height > 1 {
        app.click_zones.push((
            Rect::new(area.x, area.y, area.width, area.height - 1),
            ClickZone::Rows(RowArea::HomeGenreDetail, false),
        ));
    }
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    if body.height > 0 {
        if rows.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled("  nothing in this genre", dim())),
                body,
            );
        } else {
            let view = Table::new(
                rows,
                [
                    Constraint::Length(3),
                    Constraint::Percentage(46),
                    Constraint::Percentage(30),
                    Constraint::Length(6),
                ],
            )
            .column_spacing(if body.width >= 64 { 2 } else { 1 })
            .row_highlight_style(chrome::selected());
            let crate::home::HomeView::Detail(d) = &mut app.home.view else {
                return;
            };
            f.render_stateful_widget(view, body, &mut d.table);
        }
    }
    render_list_status(detail_state(app), count, "items", "Esc: back", status, f);
}

fn detail_state(app: &App) -> &crate::search::Load {
    match &app.home.view {
        crate::home::HomeView::Detail(d) => &d.state,
        _ => &crate::search::Load::Idle,
    }
}

fn owned_detail_rows(d: &crate::home::GenreDetail) -> Vec<Row<'static>> {
    let mut rows = Vec::new();
    let mut item_no = 0usize;
    for r in crate::home::HomeTab::detail_rows_for(d) {
        match r {
            crate::home::DetailRow::Head(label) => rows.push(head_row(label)),
            crate::home::DetailRow::Song(s) => {
                item_no += 1;
                rows.push(song_row(item_no, s));
            }
            crate::home::DetailRow::Album(a) => {
                item_no += 1;
                rows.push(card_row(
                    item_no,
                    a.title.clone(),
                    a.artist
                        .as_ref()
                        .map(|x| x.name.clone())
                        .unwrap_or_default(),
                    a.year.map(|y| y.to_string()).unwrap_or_default(),
                ));
            }
            crate::home::DetailRow::Playlist(p) => {
                item_no += 1;
                rows.push(card_row(
                    item_no,
                    p.name.clone(),
                    p.description.clone(),
                    format!("{} songs", p.songs_count),
                ));
            }
            crate::home::DetailRow::Artist(a) => {
                item_no += 1;
                rows.push(card_row(
                    item_no,
                    a.name.clone(),
                    a.label.clone().unwrap_or_default(),
                    String::new(),
                ));
            }
            crate::home::DetailRow::Sub(g) => {
                rows.push(Row::new(vec![
                    Cell::from(Span::styled("●", crate::ui::chrome::genre_dot(&g.title))),
                    Cell::from(Span::styled(g.title.clone(), strong())),
                    Cell::from(""),
                    Cell::from(""),
                ]));
            }
        }
    }
    rows
}

pub(crate) fn card_row(no: usize, title: String, sub: String, tail: String) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(no.to_string(), dim())),
        Cell::from(Span::styled(title, strong())),
        Cell::from(Span::styled(sub, dim())),
        Cell::from(Span::styled(tail, dim())),
    ])
}

fn artist_of_song(s: &crate::api::models::Song) -> String {
    s.artist
        .as_ref()
        .map(|a| a.name.clone())
        .unwrap_or_default()
}

fn render_loading(state: &crate::search::Load, area: Rect, f: &mut Frame) {
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    if body.height > 0 {
        let text = if area.width >= 30 {
            "  loading…"
        } else {
            "  …"
        };
        f.render_widget(Paragraph::new(Span::styled(text, dim())), body);
    }
    let status_text = match state {
        crate::search::Load::Failed(e) => format!("Error: {e}"),
        _ => "Esc: back".to_string(),
    };
    let style = if status_text.starts_with("Error") {
        chrome::error_style()
    } else {
        dim()
    };
    f.render_widget(
        Paragraph::new(Span::styled(status_text, style))
            .alignment(ratatui::layout::Alignment::Right),
        status,
    );
}

fn render_song_list(
    songs: &crate::api::models::Page<crate::api::models::Song>,
    state: &crate::search::Load,
    table: &mut TableState,
    area: Rect,
    f: &mut Frame,
) {
    if area.height == 0 {
        return;
    }
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    if body.height > 0 {
        if songs.items.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    if matches!(state, crate::search::Load::Loading) {
                        "  loading…"
                    } else {
                        "  nothing here yet"
                    },
                    dim(),
                )),
                body,
            );
        } else {
            let rows: Vec<Row> = songs
                .items
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    Row::new(vec![
                        Cell::from(Span::styled((i + 1).to_string(), dim())),
                        Cell::from(Span::styled(s.title.clone(), strong())),
                        Cell::from(Span::styled(artist_of_song(s), dim())),
                        Cell::from(Span::styled(fmt_secs(s.full_duration_sec), dim())),
                    ])
                })
                .collect();
            let view = Table::new(
                rows,
                [
                    Constraint::Length(3),
                    Constraint::Percentage(46),
                    Constraint::Percentage(32),
                    Constraint::Length(6),
                ],
            )
            .column_spacing(if body.width >= 64 { 2 } else { 1 })
            .row_highlight_style(chrome::selected());
            f.render_stateful_widget(view, body, table);
        }
    }
    render_list_status(state, songs.items.len(), "tracks", "Esc: back", status, f);
}

fn render_album_list(
    albums: &crate::api::models::Page<crate::api::models::Album>,
    state: &crate::search::Load,
    table: &mut TableState,
    area: Rect,
    f: &mut Frame,
) {
    if area.height == 0 {
        return;
    }
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    if body.height > 0 {
        if albums.items.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    if matches!(state, crate::search::Load::Loading) {
                        "  loading…"
                    } else {
                        "  nothing here yet"
                    },
                    dim(),
                )),
                body,
            );
        } else {
            let rows: Vec<Row> = albums
                .items
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    card_row(
                        i + 1,
                        a.title.clone(),
                        a.artist
                            .as_ref()
                            .map(|x| x.name.clone())
                            .unwrap_or_default(),
                        a.year.map(|y| y.to_string()).unwrap_or_default(),
                    )
                })
                .collect();
            let view = Table::new(
                rows,
                [
                    Constraint::Length(3),
                    Constraint::Percentage(46),
                    Constraint::Percentage(32),
                    Constraint::Length(6),
                ],
            )
            .column_spacing(if body.width >= 64 { 2 } else { 1 })
            .row_highlight_style(chrome::selected());
            f.render_stateful_widget(view, body, table);
        }
    }
    render_list_status(state, albums.items.len(), "albums", "Esc: back", status, f);
}

fn render_playlist_list(
    playlists: &crate::api::models::Page<crate::api::models::PlaylistCard>,
    state: &crate::search::Load,
    table: &mut TableState,
    area: Rect,
    f: &mut Frame,
) {
    if area.height == 0 {
        return;
    }
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    if body.height > 0 {
        if playlists.items.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    if matches!(state, crate::search::Load::Loading) {
                        "  loading…"
                    } else {
                        "  nothing here yet"
                    },
                    dim(),
                )),
                body,
            );
        } else {
            let rows: Vec<Row> = playlists
                .items
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    card_row(
                        i + 1,
                        p.name.clone(),
                        p.description.clone(),
                        format!("{} songs", p.songs_count),
                    )
                })
                .collect();
            let view = Table::new(
                rows,
                [
                    Constraint::Length(3),
                    Constraint::Percentage(46),
                    Constraint::Percentage(30),
                    Constraint::Length(10),
                ],
            )
            .column_spacing(if body.width >= 64 { 2 } else { 1 })
            .row_highlight_style(chrome::selected());
            f.render_stateful_widget(view, body, table);
        }
    }
    render_list_status(
        state,
        playlists.items.len(),
        "playlists",
        "Esc: back",
        status,
        f,
    );
}

fn render_artist_list(
    artists: &crate::api::models::Page<crate::api::models::Artist>,
    state: &crate::search::Load,
    table: &mut TableState,
    area: Rect,
    f: &mut Frame,
) {
    if area.height == 0 {
        return;
    }
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    if body.height > 0 {
        if artists.items.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    if matches!(state, crate::search::Load::Loading) {
                        "  loading…"
                    } else {
                        "  nothing here yet"
                    },
                    dim(),
                )),
                body,
            );
        } else {
            let rows: Vec<Row> = artists
                .items
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    card_row(
                        i + 1,
                        a.name.clone(),
                        a.label.clone().unwrap_or_default(),
                        String::new(),
                    )
                })
                .collect();
            let view = Table::new(
                rows,
                [
                    Constraint::Length(3),
                    Constraint::Percentage(60),
                    Constraint::Percentage(30),
                    Constraint::Length(6),
                ],
            )
            .column_spacing(if body.width >= 64 { 2 } else { 1 })
            .row_highlight_style(chrome::selected());
            f.render_stateful_widget(view, body, table);
        }
    }
    render_list_status(
        state,
        artists.items.len(),
        "artists",
        "Esc: back",
        status,
        f,
    );
}

pub(crate) fn render_list_status(
    state: &crate::search::Load,
    count: usize,
    unit: &str,
    actions: &str,
    status: Rect,
    f: &mut Frame,
) {
    let status_text = match state {
        crate::search::Load::Loading => "loading…".to_string(),
        crate::search::Load::Failed(e) => format!("Error: {e}"),
        crate::search::Load::Done => format!("{} {unit} · {}", count, actions),
        _ => String::new(),
    };
    let style = if status_text.starts_with("Error") {
        chrome::error_style()
    } else {
        dim()
    };
    f.render_widget(
        Paragraph::new(Span::styled(status_text, style))
            .alignment(ratatui::layout::Alignment::Right),
        status,
    );
}
