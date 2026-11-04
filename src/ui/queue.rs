use crate::app::App;
use crate::queue::QueueStatus;
use crate::ui::search::fmt_secs;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Row, Table};
use ratatui::Frame;

use super::chrome;

pub fn render(app: &mut App, area: ratatui::layout::Rect, frame: &mut Frame) {
    let header = Row::new(vec![
        Span::styled("#", chrome::dim()),
        Span::styled("Title", chrome::dim()),
        Span::styled("Artist", chrome::dim()),
        Span::styled("Len", chrome::dim()),
        Span::styled("Status", chrome::dim()),
    ]);
    let items = app.queue.items.clone();
    let now_playing = app.queue.current().map(|(i, _)| i).unwrap_or(usize::MAX);
    let rows = items.iter().enumerate().map(|(i, item)| {
        let n = if i == now_playing {
            "▶".to_string()
        } else {
            (i + 1).to_string()
        };
        let title = crate::ui::search::truncate_cell(&item.song.title, 38);
        let artist = item
            .song
            .artist
            .as_ref()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| "-".into());
        let dur = fmt_secs(item.song.full_duration_sec);
        let status = status_span(&item.status, now_playing == i);
        Row::new(vec![
            Span::styled(n, chrome::dim()),
            Span::raw(title),
            Span::raw(artist),
            Span::styled(dur, chrome::dim()),
            status,
        ])
    });
    let table = Table::new(
        rows,
        vec![
            Constraint::Length(4),
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Length(6),
            Constraint::Length(18),
        ],
    )
    .header(header)
    .column_spacing(if area.width >= 64 { 2 } else { 1 })
    .row_highlight_style(crate::ui::search::accent_bold());

    if app.queue.is_empty() {
        frame.render_widget(empty_hint(), area);
        return;
    }
    let table_area = if area.height >= 9 {
        let [_g, t] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        t
    } else {
        let [t] = Layout::vertical([Constraint::Fill(1)]).areas(area);
        t
    };
    app.click_zones.push((
        table_area,
        crate::app::ClickZone::Rows(crate::app::RowArea::Queue, true),
    ));
    frame.render_stateful_widget(table, table_area, &mut app.queue_table);
    crate::ui::search::render_scrollbar(
        app.queue.items.len(),
        app.queue_table.selected(),
        table_area,
        frame,
    );
    if app.queue.items.len() > area.height.saturating_sub(1) as usize {
        footer("+ more below", area, frame);
    }
}

fn status_span(status: &QueueStatus, playing_now: bool) -> Span<'static> {
    match status {
        QueueStatus::Playing if playing_now => Span::styled(
            status.label(),
            chrome::accent().add_modifier(Modifier::BOLD),
        ),
        QueueStatus::Failed => Span::styled(status.label(), chrome::error_style()),
        QueueStatus::Played => Span::styled(status.label(), chrome::dim()),
        _ => Span::raw(status.label()),
    }
}

fn empty_hint() -> ratatui::widgets::Paragraph<'static> {
    ratatui::widgets::Paragraph::new(vec![
        Line::from(Span::styled("the queue is empty", chrome::strong())),
        Line::from(Span::styled(
            "add tracks from Home or Search",
            chrome::dim(),
        )),
    ])
}

fn footer(text: &str, area: ratatui::layout::Rect, frame: &mut Frame) {
    if area.height <= 1 {
        return;
    }
    let row = ratatui::layout::Rect {
        x: area.x,
        y: area.y + area.height - 1,
        width: area.width,
        height: 1,
    };
    frame.render_widget(
        ratatui::widgets::Paragraph::new(Span::styled(text.to_string(), chrome::dim())),
        row,
    );
}
