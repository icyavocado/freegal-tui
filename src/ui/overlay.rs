use crate::app::App;
use crate::ui::chrome;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    Rect::new(x, y, w, h)
}

/// Erase background text around `area` so popups don't blend with content.
/// Clamped inside the outer page border so the frame stays intact.
pub(crate) fn render_backdrop(area: Rect, frame: &mut Frame) {
    let outer = frame.area();
    let page_inner = Rect::new(
        outer.x.saturating_add(1),
        outer.y.saturating_add(1),
        outer.width.saturating_sub(2),
        outer.height.saturating_sub(2),
    );
    let margin = Rect::new(
        area.x.saturating_sub(1),
        area.y.saturating_sub(1),
        area.width.saturating_add(2),
        area.height.saturating_add(2),
    )
    .intersection(page_inner);
    if margin.width > 0 && margin.height > 0 {
        frame.render_widget(Clear, margin);
    }
}

pub fn render_audio_menu(app: &App, frame: &mut Frame) {
    let h = (app.audio_devices.len() as u16 + 4)
        .min(22)
        .min(frame.area().height.saturating_sub(9));
    let area = centered(frame.area(), 62, h);
    render_backdrop(area, frame);
    let block = chrome::popup_card("audio output").border_style(chrome::emphasis_border());
    frame.render_widget(&block, area);
    let inner = block.inner(area);

    if app.audio_devices.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("scanning outputs…", chrome::dim())),
            inner,
        );
        return;
    }

    let selected = app.audio_menu_table.selected().unwrap_or(0);
    let mut lines: Vec<Line> = Vec::new();
    for (i, (id, desc)) in app.audio_devices.iter().enumerate() {
        let now = i == selected;
        let id = if id.chars().count() > 34 {
            format!("{}…", id.chars().take(33).collect::<String>())
        } else {
            id.clone()
        };
        lines.push(Line::from(vec![
            Span::raw(if now { "▶ " } else { "  " }),
            Span::styled(id, if now { chrome::strong() } else { chrome::dim() }),
            Span::styled(format!(" {desc}"), chrome::dim()),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter: use · Esc: cancel",
        chrome::dim(),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

pub fn render_help(frame: &mut Frame) {
    let rows = (chrome::KEYMAP.len() as u16 + 1).div_ceil(2) + 6;
    let area = centered(frame.area(), 76, rows);
    render_backdrop(area, frame);
    let block = chrome::popup_card("help");
    frame.render_widget(&block, area);
    let inner = block.inner(area);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "Esc closes · keys are focus-scoped",
            chrome::dim(),
        )),
        Line::from(""),
    ];
    let mut it = chrome::KEYMAP.iter();
    while let Some(a) = it.next() {
        let b = it.next().copied();
        let mut spans = vec![
            Span::styled(format!("{:<16}", a.0), chrome::strong()),
            Span::raw(format!("{:<18}", a.1)),
        ];
        if let Some((keys, action, _)) = b {
            spans.push(Span::styled(format!("{:<16}", keys), chrome::strong()));
            spans.push(Span::raw(action.to_string()));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "personal use · your library card · human pace",
        chrome::dim(),
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

pub fn render_errors(app: &App, frame: &mut Frame) {
    let count = app.errors.len().max(1) as u16;
    let area = centered(frame.area(), 80, (count + 4).min(18));
    render_backdrop(area, frame);
    let block = chrome::popup_card("errors (newest last)");
    frame.render_widget(&block, area);
    let inner = block.inner(area);

    if app.errors.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("no errors recorded", chrome::dim()))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }
    let window = inner.height as usize;
    let total = app.errors.len();
    let offset = total
        .saturating_sub(window)
        .saturating_sub(app.errors_scroll.min(total.saturating_sub(window)));
    let rows = Layout::vertical(vec![Constraint::Length(1); window]).split(inner);
    for (i, e) in app.errors.iter().skip(offset).take(window).enumerate() {
        let text = match e.code {
            Some(c) => format!("({c}) {}", e.message),
            None => e.message.clone(),
        };
        frame.render_widget(Paragraph::new(Span::styled(text, chrome::dim())), rows[i]);
    }
}
