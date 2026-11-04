use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use ratatui::Frame;

use crate::app::{App, ClickZone, RowArea};
use crate::mymusic::{empty_line, MusicData, MusicRow, MusicView};
use crate::search::Load;
use crate::ui::chrome::{self, dim, strong};
use crate::ui::home::{card_row, head_row, render_list_status, song_row};

pub fn render(app: &mut App, area: Rect, f: &mut Frame) {
    app.music.last_width = area.width;
    if area.height == 0 {
        return;
    }
    match &mut app.music.view {
        MusicView::Root => render_root(app, area, f),
        _ if area.height > 0 => {
            let [crumb, rest] =
                Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
            if crumb.height > 0 {
                render_crumb(app, crumb, f);
            }
            if rest.height == 0 {
                return;
            }
            match &mut app.music.view {
                MusicView::Loading { state } => render_loading(state, rest, f),
                MusicView::Playlists { page, state, table } => {
                    if rest.height > 1 {
                        app.click_zones.push((
                            Rect::new(rest.x, rest.y, rest.width, rest.height - 1),
                            ClickZone::Rows(RowArea::MusicPlaylists, false),
                        ));
                    }
                    render_playlists(page, state, table, rest, f);
                }
                MusicView::Songs {
                    songs,
                    state,
                    table,
                    ..
                } => {
                    if rest.height > 1 {
                        app.click_zones.push((
                            Rect::new(rest.x, rest.y, rest.width, rest.height - 1),
                            ClickZone::Rows(RowArea::MusicList, false),
                        ));
                    }
                    render_songs(songs, state, table, rest, f);
                }
                MusicView::Root => {}
            }
        }
        _ => {}
    }
}

fn render_crumb(app: &mut App, area: Rect, f: &mut Frame) {
    let home_rect = Rect::new(area.x + 1, area.y, 8.min(area.width.saturating_sub(1)), 1);
    app.click_zones.push((home_rect, ClickZone::MusicCrumb(0)));
    f.render_widget(
        Paragraph::new(Span::styled("My Music".to_string(), dim())),
        home_rect,
    );
    let Some(title) = app.music.crumb.clone() else {
        return;
    };
    let sep_x = (area.x + 1 + 8).min(area.right());
    if area.width > 8 + 4 {
        let sep = Rect::new(sep_x, area.y, 3.min(area.right().saturating_sub(sep_x)), 1);
        f.render_widget(
            Paragraph::new(Span::styled(" \u{203a} ".to_string(), dim())),
            sep,
        );
        let seg_x = sep_x + 3;
        let seg = Rect::new(seg_x, area.y, area.right().saturating_sub(seg_x), 1);
        f.render_widget(Paragraph::new(Span::styled(title, strong())), seg);
    }
}

fn render_root(app: &mut App, area: Rect, f: &mut Frame) {
    let [strip, _gap, main, status_row] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    crate::ui::home::render_launcher(app, strip, f, true);
    render_status(app, status_row, f);
    if main.height == 0 {
        return;
    }
    app.click_zones
        .push((main, ClickZone::Rows(RowArea::MusicRoot, false)));
    let rows = owned_music_rows(app);
    if rows.is_empty() {
        let text = if app.music.busy() {
            ""
        } else {
            "press R to load your music"
        };
        if !text.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(text, dim()))
                    .alignment(ratatui::layout::Alignment::Center),
                main,
            );
        }
        return;
    }
    let spacing: u16 = if main.width >= 64 { 2 } else { 1 };
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
    .row_highlight_style(chrome::selected());
    f.render_stateful_widget(table, main, &mut app.music.root_table);
}

fn render_status(app: &App, row: Rect, f: &mut Frame) {
    let text = if app.music.busy() {
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
    app.music.sections.iter().find_map(|s| match &s.state {
        Load::Failed(e) => Some(format!("{}: {e}", s.title)),
        _ => None,
    })
}

fn owned_music_rows(app: &App) -> Vec<Row<'static>> {
    let mut rows = Vec::new();
    let mut item_no = 0usize;
    for r in crate::mymusic::music_rows(&app.music) {
        match r {
            MusicRow::Head(feed, title) => {
                rows.push(head_row(title));
                if let Some(section) = app.music.section(feed) {
                    if section.state == Load::Done && is_empty(&section.data) {
                        rows.push(Row::new(vec![
                            Cell::from(""),
                            Cell::from(Span::styled(empty_line(feed), dim())),
                            Cell::from(""),
                            Cell::from(""),
                        ]));
                    }
                }
            }
            MusicRow::Song(s) => {
                item_no += 1;
                rows.push(song_row(item_no, s));
            }
            MusicRow::Playlist(p) => {
                item_no += 1;
                rows.push(card_row(
                    item_no,
                    p.name.clone(),
                    p.description.clone(),
                    format!("{} songs", p.songs_count),
                ));
            }
        }
    }
    rows
}

fn is_empty(data: &MusicData) -> bool {
    match data {
        MusicData::None => true,
        MusicData::Songs(p) => p.items.is_empty(),
        MusicData::Playlists(p) => p.items.is_empty(),
    }
}

fn render_loading(state: &Load, area: Rect, f: &mut Frame) {
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
        Load::Failed(e) => format!("Error: {e}"),
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

fn render_playlists(
    page: &crate::api::models::Page<crate::api::models::PlaylistCard>,
    state: &Load,
    table: &mut ratatui::widgets::TableState,
    area: Rect,
    f: &mut Frame,
) {
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    if body.height > 0 {
        if page.items.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    if matches!(state, Load::Loading) {
                        "  loading…"
                    } else {
                        "  Your created playlists will appear here."
                    },
                    dim(),
                )),
                body,
            );
        } else {
            let rows: Vec<Row> = page
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
    render_list_status(state, page.items.len(), "playlists", "Esc: back", status, f);
}

fn render_songs(
    page: &crate::api::models::Page<crate::api::models::Song>,
    state: &Load,
    table: &mut ratatui::widgets::TableState,
    area: Rect,
    f: &mut Frame,
) {
    let [body, status] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
    if body.height > 0 {
        if page.items.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(
                    if matches!(state, Load::Loading) {
                        "  loading…"
                    } else {
                        "  nothing here yet"
                    },
                    dim(),
                )),
                body,
            );
        } else {
            let rows: Vec<Row> = page
                .items
                .iter()
                .enumerate()
                .map(|(i, s)| song_row(i + 1, s))
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
    render_list_status(state, page.items.len(), "tracks", "Esc: back", status, f);
}
