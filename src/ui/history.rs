use crate::app::App;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row as RRow, Table};
use ratatui::Frame;

use super::chrome;
use super::search;

pub fn render(app: &mut App, area: Rect, frame: &mut Frame) {
    if area.height == 0 {
        return;
    }
    let rows = app.history_filtered();
    let in_filter = app.history_focus == crate::search::Focus::Input;
    let (input_row, table_area, status_row) = if area.height >= 12 {
        let [ib, _g1, t, _g2, s] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);
        frame.render_widget(chrome::input_box(in_filter), ib);
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

    search::render_input(
        &mut app.history_input,
        in_filter,
        input_row,
        frame,
        " 🔍 Filter played songs — / to edit",
    );
    app.click_zones
        .push((input_row, crate::app::ClickZone::HistoryInput));
    app.click_zones.push((
        table_area,
        crate::app::ClickZone::Rows(crate::app::RowArea::History, true),
    ));

    let widths = [
        Constraint::Length(8),
        Constraint::Percentage(36),
        Constraint::Percentage(24),
        Constraint::Percentage(28),
    ];
    let header = ["Played", "Title", "Artist", "Album"]
        .into_iter()
        .map(|h| Cell::from(Span::styled(h.to_string(), chrome::dim())));
    let body = rows.iter().map(|e| {
        RRow::new(vec![
            Cell::from(Span::styled(
                crate::history::rel_time(app.now_unix, e.played_at_unix),
                chrome::dim(),
            )),
            Cell::from(search::truncate_cell(&e.title, table_area.width * 36 / 100)),
            Cell::from(search::truncate_cell(
                &e.artist_name,
                table_area.width * 24 / 100,
            )),
            Cell::from(search::truncate_cell(
                &e.album_title,
                table_area.width * 28 / 100,
            )),
        ])
    });
    let table = Table::new(body, widths)
        .column_spacing(if area.width >= 64 { 2 } else { 1 })
        .header(ratatui::widgets::Row::new(header))
        .row_highlight_style(search::accent_bold());
    frame.render_stateful_widget(table, table_area, &mut app.history_table);
    search::render_scrollbar(rows.len(), app.history_table.selected(), table_area, frame);

    let total = app.history.len();
    let status = if total == 0 {
        "no history yet — play something".to_owned()
    } else if rows.is_empty() {
        "no matches".to_owned()
    } else {
        format!("{total} played")
    };
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(Span::styled(status, chrome::dim()))),
        status_row,
    );
}
