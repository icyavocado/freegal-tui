use crate::api::models::{Album, Song};
use crate::app::{App, ClickZone, RowArea};
use crate::search::{ArtistContext, Focus, Load, SearchHome, SearchView};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row as RRow, Table};
use ratatui::Frame;
use tui_input::Input;

use super::chrome;

pub fn render(app: &mut App, area: Rect, frame: &mut Frame) {
    let mut zones = std::mem::take(&mut app.click_zones);
    if let Some(view) = app.search.top_mut() {
        match view {
            SearchView::Root(home) => render_root(home, area, frame, &mut zones),
            SearchView::Artist(ctx) => render_artist(ctx, area, frame, &mut zones),
            SearchView::Album(ctx) => {
                render_album(ctx, area, frame, &mut zones, RowArea::SearchAlbum)
            }
        }
    }
    app.click_zones = zones;
}

fn dim() -> Style {
    chrome::dim()
}

fn strong() -> Style {
    chrome::strong()
}

fn render_root(
    home: &mut SearchHome,
    area: Rect,
    frame: &mut Frame,
    zones: &mut Vec<(Rect, ClickZone)>,
) {
    if area.height == 0 {
        return;
    }
    let rows = home.rows();
    let busy = home.any_busy();
    let error = home.first_error().map(str::to_owned);
    let has_rows = home.has_rows();
    let more = home.more_available();
    let sort = home.sort.label().to_owned();
    let query = home.input.value().trim().to_string();
    let zero = !query.is_empty()
        && !busy
        && error.is_none()
        && !has_rows
        && home.kind_state.iter().all(|l| matches!(l, Load::Done));
    let input_focus = matches!(home.focus, Focus::Input);
    let SearchHome {
        input,
        table: table_state,
        ..
    } = home;

    let (input_row, list, status_row) = if area.height >= 12 {
        let [ib, _g1, t, _g2, s] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(chrome::input_box(input_focus), ib);
        let inner = ib.inner(ratatui::layout::Margin::new(0, 1));
        (inner, t, s)
    } else if area.height >= 9 {
        let [a, _g1, t, _g2, s] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        (a, t, s)
    } else {
        let [a, t, s] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);
        (a, t, s)
    };
    zones.push((input_row, ClickZone::SearchInput));
    zones.push((list, ClickZone::Rows(RowArea::SearchRoot, true)));
    render_input(
        input,
        input_focus,
        input_row,
        frame,
        " 🔍 Search songs, artists, albums — press / to type",
    );

    let status = (busy, error, has_rows, more, sort);
    if zero {
        render_zero_results(&query, list, frame);
        frame.render_widget(
            ratatui::widgets::Paragraph::new(Line::default()),
            status_row,
        );
        return;
    }

    let widths = [
        Constraint::Length(3),
        Constraint::Length(7),
        Constraint::Percentage(28),
        Constraint::Percentage(19),
        Constraint::Percentage(24),
        Constraint::Length(6),
        Constraint::Length(3),
    ];
    let header = ["#", "Type", "Title", "Artist", "Album", "Len", "E"]
        .into_iter()
        .map(|h| Cell::from(Span::styled(h.to_string(), dim())));
    let body = rows.iter().enumerate().map(|(i, r)| {
        let e_badge = match r {
            crate::search::Row::Song(s) if s.explicit => {
                Span::styled("[E]", dim().add_modifier(Modifier::BOLD))
            }
            _ => Span::raw(""),
        };
        RRow::new(vec![
            Cell::from(Span::styled(format!("{}", i + 1), dim())),
            Cell::from(Span::styled(r.kind_label().to_string(), strong())),
            Cell::from(truncate_cell(r.title(), list.width * 28 / 100)),
            Cell::from(truncate_cell(r.artist_name(), list.width * 19 / 100)),
            Cell::from(truncate_cell(&r.album_label(), list.width * 24 / 100)),
            Cell::from(fmt_secs(r.duration_secs())),
            Cell::from(e_badge),
        ])
    });
    let table = Table::new(body, widths)
        .column_spacing(if area.width >= 64 { 2 } else { 1 })
        .header(ratatui::widgets::Row::new(header))
        .row_highlight_style(accent_bold());
    frame.render_stateful_widget(table, list, table_state);
    render_scrollbar(rows.len(), table_state.selected(), list, frame);

    render_status(&status, status_row, frame);
}

fn render_zero_results(query: &str, area: Rect, frame: &mut Frame) {
    use ratatui::layout::Alignment;
    let [_top, mid, _bot] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(2),
        Constraint::Fill(1),
    ])
    .areas(area);
    let lines = vec![
        Line::from(Span::styled(
            format!("No results found for \"{query}\""),
            strong(),
        )),
        Line::from(Span::styled(
            "try checking spelling or using fewer keywords",
            dim(),
        )),
    ];
    frame.render_widget(
        ratatui::widgets::Paragraph::new(lines).alignment(Alignment::Center),
        mid,
    );
}

pub fn render_scrollbar(length: usize, position: Option<usize>, area: Rect, frame: &mut Frame) {
    use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
    let viewport = area.height as usize;
    if length <= viewport {
        return;
    }
    let state = ScrollbarState::new(length).position(position.unwrap_or(0));
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None),
        area,
        &mut { state },
    );
}

pub(crate) fn render_input(
    input: &mut Input,
    focused: bool,
    row: Rect,
    frame: &mut Frame,
    placeholder: &str,
) {
    let value = input.value().to_string();
    let cursor = input.cursor();
    let (prompt, value_style, hint) = if focused {
        (
            "/ ",
            chrome::accent().add_modifier(Modifier::BOLD),
            Some(" Enter run · Esc results"),
        )
    } else if value.is_empty() {
        ("  ", dim(), Some(placeholder))
    } else {
        ("  ", dim(), None)
    };
    let mut spans = vec![Span::styled(
        prompt,
        chrome::accent().add_modifier(Modifier::BOLD),
    )];
    spans.push(Span::styled(value, value_style));
    if let Some(h) = hint {
        spans.push(Span::styled(h, dim()));
    }
    frame.render_widget(ratatui::widgets::Paragraph::new(Line::from(spans)), row);
    if focused && row.width > 4 {
        let col = (cursor as u16).min(row.width - 4);
        frame.set_cursor_position((row.x + 2 + col, row.y));
    }
}

fn render_status(stats: &(bool, Option<String>, bool, bool, String), row: Rect, frame: &mut Frame) {
    let (busy, error, has_rows, more, sort) = stats;
    let line = if *busy {
        Line::from(Span::styled("loading…", dim()))
    } else if let Some(msg) = error {
        Line::from(Span::styled(
            format!("Error: {msg} — R to retry"),
            chrome::error_style(),
        ))
    } else if !*has_rows {
        Line::default()
    } else if *more {
        Line::from(Span::styled("+ more — refine your search", dim()))
    } else {
        Line::from(Span::styled(format!("sort: {sort}"), dim()))
    };
    frame.render_widget(ratatui::widgets::Paragraph::new(line), row);
}

fn render_artist(
    ctx: &mut ArtistContext,
    area: Rect,
    frame: &mut Frame,
    zones: &mut Vec<(Rect, ClickZone)>,
) {
    if area.height == 0 {
        return;
    }
    let name = ctx.artist.name.clone();
    let label = ctx.artist.label.clone();
    let state = ctx.state.clone();
    let albums = &ctx.albums;

    let (head_row, table_area, status_row) = if area.height >= 9 {
        let [a, _g1, t, _g2, s] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        (a, t, s)
    } else {
        let [a, t, s] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);
        (a, t, s)
    };

    let mut spans = vec![Span::styled(name, strong())];
    if let Some(label) = &label {
        spans.push(Span::styled(format!("  ·  {label}"), dim()));
    }
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(spans)),
        head_row,
    );

    let widths = [
        Constraint::Length(6),
        Constraint::Fill(1),
        Constraint::Length(8),
    ];
    let header = ["Year", "Album", "Tracks"]
        .into_iter()
        .map(|h| Cell::from(Span::styled(h.to_string(), dim())));
    let body = albums.items.iter().map(|a: &Album| {
        let tracks = if a.songs.is_empty() {
            String::new()
        } else {
            a.songs.len().to_string()
        };
        RRow::new(vec![
            Cell::from(a.year.map(|y| y.to_string()).unwrap_or_default()),
            Cell::from(truncate_cell(&a.title, table_area.width.saturating_sub(16))),
            Cell::from(tracks),
        ])
    });
    let table = Table::new(body, widths)
        .header(ratatui::widgets::Row::new(header))
        .row_highlight_style(accent_bold());
    zones.push((table_area, ClickZone::Rows(RowArea::SearchArtist, true)));
    frame.render_stateful_widget(table, table_area, &mut ctx.table);

    let status = match state {
        Load::Loading | Load::Dirty => "loading albums…",
        Load::Failed(_) => "albums failed — R to retry",
        _ => "Esc: back",
    };
    let style = if matches!(state, Load::Failed(_)) {
        chrome::error_style()
    } else {
        dim()
    };
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Span::styled(status, style)),
        status_row,
    );
}

pub(crate) fn render_album(
    ctx: &mut crate::search::AlbumContext,
    area: Rect,
    frame: &mut Frame,
    zones: &mut Vec<(Rect, ClickZone)>,
    zone_row: RowArea,
) {
    if area.height == 0 {
        return;
    }
    let title = ctx.album.title.clone();
    let artist = ctx.album.artist.as_ref().map(|a| a.name.clone());
    let year = ctx.album.year;
    let genre = ctx.album.genre.clone();

    let lines_needed = 2u16;
    if area.height < lines_needed + 3 {
        return;
    }
    let (head, table_area) = if area.height >= 9 {
        let [a, _g1, t] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(area);
        (a, t)
    } else {
        let [a, t] = Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(area);
        (a, t)
    };

    let mut head_lines = vec![Line::from(vec![Span::styled(title, strong())])];
    let mut meta = vec![];
    if let Some(name) = &artist {
        meta.push(name.clone());
    }
    if let Some(year) = year {
        meta.push(year.to_string());
    }
    if let Some(genre) = &genre {
        meta.push(genre.clone());
    }
    if !meta.is_empty() {
        head_lines.push(Line::from(Span::styled(meta.join("  ·  "), dim())));
    }
    frame.render_widget(ratatui::widgets::Paragraph::new(head_lines), head);

    let widths = [
        Constraint::Length(4),
        Constraint::Fill(1),
        Constraint::Length(6),
    ];
    let header = ["#", "Title", "Len"]
        .into_iter()
        .map(|h| Cell::from(Span::styled(h.to_string(), dim())));
    let body = ctx.album.songs.iter().map(|s: &Song| {
        RRow::new(vec![
            Cell::from(s.track_number.map(|t| t.to_string()).unwrap_or_default()),
            Cell::from(truncate_cell(&s.title, table_area.width.saturating_sub(12))),
            Cell::from(fmt_secs(s.full_duration_sec)),
        ])
    });
    let table = Table::new(body, widths)
        .header(ratatui::widgets::Row::new(header))
        .row_highlight_style(accent_bold());
    zones.push((table_area, ClickZone::Rows(zone_row, true)));
    frame.render_stateful_widget(table, table_area, &mut ctx.table);
}

pub fn accent_bold() -> Style {
    chrome::selected()
}

pub fn truncate_cell(text: &str, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    text.chars().take(width as usize).collect()
}

pub fn fmt_secs(secs: Option<u64>) -> String {
    match secs {
        None => "-".to_string(),
        Some(s) => format!("{}:{:02}", s / 60, s % 60),
    }
}
