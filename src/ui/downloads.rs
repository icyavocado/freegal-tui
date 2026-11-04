//! Downloads tab (specs/02-tui.md S3): session-local downloaded rows.

use crate::app::App;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Row, Table};
use ratatui::Frame;

use super::chrome;

pub fn render(app: &mut App, area: Rect, frame: &mut Frame) {
    if app.downloads.is_empty() {
        let hint = ratatui::layout::Layout::vertical(vec![
            Constraint::Fill(1),
            Constraint::Length(2),
            Constraint::Fill(1),
        ])
        .split(area)[1];
        frame.render_widget(
            ratatui::widgets::Paragraph::new(vec![
                Line::from(Span::styled("nothing downloaded yet", chrome::strong()))
                    .alignment(ratatui::layout::Alignment::Center),
                Line::from(Span::styled("press y on a song to download", chrome::dim()))
                    .alignment(ratatui::layout::Alignment::Center),
            ]),
            hint,
        );
        return;
    }

    let header = Row::new([
        Span::styled("Title", chrome::dim()),
        Span::styled("Artist", chrome::dim()),
        Span::styled("File", chrome::dim()),
    ]);
    let rows = app
        .downloads
        .iter()
        .map(|d| {
            Row::new(vec![
                Span::raw(d.title.clone()),
                Span::raw(if d.artist.is_empty() {
                    "-".into()
                } else {
                    d.artist.clone()
                }),
                Span::styled(d.file_name.clone(), chrome::dim()),
            ])
        })
        .collect::<Vec<_>>();
    let table = Table::new(
        rows,
        [
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Fill(2),
        ],
    )
    .column_spacing(if area.width >= 64 { 2 } else { 1 })
    .header(header);
    let table_area = if area.height >= 9 {
        let [_g, t] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        t
    } else {
        let [t] = Layout::vertical([Constraint::Fill(1)]).areas(area);
        t
    };
    frame.render_widget(table, table_area);
}
