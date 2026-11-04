use crate::app::App;
use crate::app::{LoginWizard, Screen, WizardStep};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use tui_input::Input;

use super::chrome;

pub fn render_login(app: &App, frame: &mut Frame) {
    let Screen::Login(w) = app.screen() else {
        return;
    };
    let area = centered(frame.area(), 62, 14);
    let block = chrome::popup_card(&format!("sign in · step {} of 4", step_n(w)));
    frame.render_widget(&block, area);
    let inner = block.inner(area);

    if let Some(fallback) = w.fallback.as_ref() {
        render_fallback(inner, &fallback.input, w.busy, frame);
        return;
    }

    let rows = Layout::vertical(vec![
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let banner = if app.restoring {
        Line::from(Span::styled("connecting to your library…", chrome::dim()))
    } else {
        Line::from(Span::styled(step_banner(w.step), chrome::dim()))
    };
    frame.render_widget(Paragraph::new(banner), rows[0]);

    if w.busy {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("working…", chrome::dim())))
                .alignment(Alignment::Right),
            rows[6],
        );
    } else {
        frame.render_widget(Paragraph::new(chrome::hint_login()), rows[6]);
    }

    match w.step {
        WizardStep::Subdomain => {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled("Library website", chrome::strong())),
                    Line::from(Span::styled(
                        "your library card unlocks this app",
                        chrome::dim(),
                    )),
                ]),
                rows[1],
            );
            let (line, cursor) = input_line(&w.subdomain_input, false, rows[3].width);
            frame.render_widget(Paragraph::new(line), rows[3]);
            if let Some(x) = cursor {
                set_cursor(frame, rows[3], x);
            }
            if let Some(text) = &w.preview {
                frame.render_widget(
                    Paragraph::new(Span::styled(text.clone(), chrome::accent())),
                    rows[4],
                );
            }
            render_error(w.error.as_deref(), rows[5], frame);
        }
        WizardStep::LibraryConfirm => {
            let name = w
                .library
                .as_ref()
                .map(|l| l.name.as_str())
                .unwrap_or("unknown library");
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(Span::styled("Confirm library", chrome::strong())),
                    Line::from(name.to_string()),
                ]),
                rows[1],
            );
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "Enter: yes, this is my library   Esc: change library",
                    chrome::dim(),
                )),
                rows[4],
            );
        }
        WizardStep::Card => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "Library card number",
                    chrome::strong(),
                ))),
                rows[1],
            );
            let (line, cursor) = input_line(&w.card_input, false, rows[3].width);
            frame.render_widget(Paragraph::new(line), rows[3]);
            if let Some(x) = cursor {
                set_cursor(frame, rows[3], x);
            }
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "saved for silent re-login (0600 file)",
                    chrome::dim(),
                )),
                rows[4],
            );
            render_error(w.error.as_deref(), rows[5], frame);
        }
        WizardStep::Pin => {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("PIN", chrome::strong()))),
                rows[1],
            );
            let (line, cursor) = input_line(&w.pin_input, true, rows[3].width);
            frame.render_widget(Paragraph::new(line), rows[3]);
            if let Some(x) = cursor {
                set_cursor(frame, rows[3], x);
            }
            render_error(w.error.as_deref(), rows[5], frame);
        }
    }
}

fn step_n(w: &LoginWizard) -> u8 {
    match w.step {
        WizardStep::Subdomain => 1,
        WizardStep::LibraryConfirm => 2,
        WizardStep::Card => 3,
        WizardStep::Pin => 4,
    }
}

fn step_banner(step: WizardStep) -> String {
    let names = ["library", "confirm", "card", "pin"];
    let current = step as usize;
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            if i == current {
                format!("[{name}]")
            } else {
                (*name).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn render_error(msg: Option<&str>, row: Rect, frame: &mut Frame) {
    if let Some(msg) = msg {
        let style = if chrome::no_color_active() {
            chrome::strong()
        } else {
            chrome::error_style()
        };
        frame.render_widget(Paragraph::new(Span::styled(format!("✗ {msg}"), style)), row);
    }
}

fn render_fallback(area: Rect, input: &Input, busy: bool, frame: &mut Frame) {
    let rows = Layout::vertical(vec![
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "manual library id",
            chrome::strong(),
        ))),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "library lookup failed - enter the numeric id",
            chrome::dim(),
        )),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(input_line(input, false, rows[2].width).0),
        rows[2],
    );
    if !busy {
        frame.render_widget(
            Paragraph::new(Span::styled("Enter: use id · Esc: back", chrome::dim())),
            rows[3],
        );
    }
}

fn input_line(input: &Input, mask: bool, width: u16) -> (Line<'static>, Option<usize>) {
    let width = width.max(1) as usize;
    let value = input.value();
    let cursor_pos = input.cursor().min(value.len());
    let shown: String = if mask {
        "•".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    let visible_from = cursor_pos.saturating_sub(width / 2);
    let clipped: String = shown.chars().skip(visible_from).take(width).collect();
    let col = cursor_pos.saturating_sub(visible_from);
    (Line::from(clipped), Some(col))
}

fn set_cursor(frame: &mut Frame, row: Rect, col: usize) {
    if row.width == 0 {
        return;
    }
    let x = row.x + col.min(row.width.saturating_sub(1) as usize) as u16;
    frame.set_cursor_position((x, row.y));
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    Rect::new(x, y, w, h)
}
