pub mod chrome;
pub mod downloads;
pub mod history;
pub mod home;
pub mod mymusic;
pub mod overlay;
pub mod queue;
pub mod search;
pub mod theme;

mod login;

#[cfg(test)]
pub mod showcase;
#[cfg(test)]
pub mod tests;

use crate::app::{App, Screen};
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub fn render(app: &mut App, frame: &mut Frame) {
    app.click_zones.clear();
    if chrome::layout_ladder(frame.area()).is_none() {
        chrome::render_too_small(frame);
        return;
    }
    match app.screen() {
        Screen::Login(_) => login::render_login(&*app, frame),
        Screen::Home => chrome::render_home(app, frame),
    }
    render_toasts(&app.toasts, frame);
    let opening_modal = app.download_confirm.is_some()
        || app.clear_history_question
        || app.help_open
        || app.errors_open
        || app.audio_menu_open;
    if opening_modal && matches!(app.screen(), Screen::Home) {
        if let Some(c) = chrome::layout_ladder(frame.area()) {
            frame.render_widget(ratatui::widgets::Clear, c.content);
        }
    }
    if app.download_confirm.is_some() {
        render_download_confirm(app, frame);
    } else if app.clear_history_question {
        render_clear_history_confirm(app, frame);
    } else if app.help_open {
        overlay::render_help(frame);
    } else if app.errors_open {
        overlay::render_errors(app, frame);
    } else if app.audio_menu_open {
        overlay::render_audio_menu(app, frame);
    }
}

fn render_clear_history_confirm(app: &mut App, frame: &mut Frame) {
    let area = login_centered(frame.area(), 44, 5);
    crate::ui::overlay::render_backdrop(area, frame);
    let block = chrome::popup_card("clear history");
    frame.render_widget(&block, area);
    frame.render_widget(
        ratatui::widgets::Paragraph::new(vec![
            ratatui::text::Line::from(Span::styled(
                format!("Forget all {} played entries?", app.history.len()),
                chrome::accent(),
            )),
            ratatui::text::Line::from(Span::styled("y: clear · n/Esc: cancel", chrome::dim())),
        ]),
        block.inner(area),
    );
}

fn render_download_confirm(app: &mut App, frame: &mut Frame) {
    let Some(song) = app.download_confirm.as_ref() else {
        return;
    };
    let area = login_centered(frame.area(), 62, 8);
    crate::ui::overlay::render_backdrop(area, frame);
    let block = chrome::popup_card("download");
    frame.render_widget(&block, area);
    let inner = block.inner(area);
    let eligible = app.downloaded_ids.contains(&song.id);
    let cost_line = if eligible {
        Line::from(Span::styled(
            "already downloaded - redownload (no credit)",
            chrome::accent(),
        ))
    } else {
        match app.downloads_limit.zip(app.downloads_used) {
            Some((limit, used)) if used >= limit => Line::from(Span::styled(
                "no credits remaining this week",
                chrome::error_style(),
            )),
            Some((limit, used)) => Line::from(Span::styled(
                format!("Uses 1 credit - {} of {} remain", limit - used, limit),
                chrome::strong(),
            )),
            None => Line::from(Span::styled("Uses 1 credit", chrome::strong())),
        }
    };
    let artist = song
        .artist
        .as_ref()
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "unknown artist".into());
    let album = song
        .album
        .as_ref()
        .map(|a| a.title.clone())
        .unwrap_or_default();
    let meta = Line::from(Span::styled(
        format!("{artist} · {album}").trim_end().to_string(),
        chrome::dim(),
    ));
    frame.render_widget(
        ratatui::widgets::Paragraph::new(vec![
            Line::from(Span::styled(song.title.clone(), chrome::strong())),
            meta,
            cost_line,
            Line::from(Span::styled(
                "y: confirm · n/Esc/Enter: cancel",
                chrome::dim(),
            )),
        ]),
        inner,
    );
}

fn login_centered(area: ratatui::layout::Rect, w: u16, h: u16) -> ratatui::layout::Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    ratatui::layout::Rect::new(x, y, w, h)
}

fn render_toasts(toasts: &crate::app::Toasts, frame: &mut Frame) {
    if toasts.is_empty() {
        return;
    }
    let area = frame.area();
    let count = toasts.items.len() as u16;
    let bottom = area.height.saturating_sub(3);
    let rows = ratatui::layout::Layout::vertical(vec![
        ratatui::layout::Constraint::Length(1);
        count as usize
    ])
    .split(ratatui::layout::Rect {
        x: area.x,
        y: bottom - count,
        width: area.width,
        height: count,
    });
    for (i, toast) in toasts.items.iter().enumerate() {
        frame.render_widget(ratatui::widgets::Clear, rows[i]);
        let style = match toast.level {
            crate::app::ToastLevel::Info => chrome::dim(),
            crate::app::ToastLevel::Success => chrome::success_style(),
            crate::app::ToastLevel::Error => chrome::error_style(),
        };
        let text: String = toast
            .text
            .chars()
            .take((area.width as usize).saturating_sub(2))
            .collect();
        frame.render_widget(
            ratatui::widgets::Paragraph::new(ratatui::text::Line::from(Span::styled(text, style)))
                .alignment(ratatui::layout::Alignment::Right),
            rows[i],
        );
    }
}
