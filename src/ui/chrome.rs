use crate::app::{App, ClickZone, Tab};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub const MIN_WIDTH: u16 = 40;
pub const MIN_HEIGHT: u16 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeMode {
    Full,
    NoHint,
    FoldedTabs,
    Minimal,
}

#[derive(Debug, Clone, Copy)]
pub struct Chrome {
    pub mode: ChromeMode,
    pub header: Rect,
    pub footer: Option<Rect>,
    pub content: Rect,
    pub bar: Rect,
}

use ratatui::layout::Constraint::{Fill, Length};

pub fn layout_ladder(area: Rect) -> Option<Chrome> {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return None;
    }
    let mode = match area.height {
        h if h >= 20 => ChromeMode::Full,
        h if h >= 16 => ChromeMode::NoHint,
        h if h >= 12 => ChromeMode::FoldedTabs,
        _ => ChromeMode::Minimal,
    };
    let constraints: Vec<Constraint> = match mode {
        ChromeMode::Full => vec![Length(1), Fill(1), Length(1), Length(2)],
        _ => vec![Length(1), Fill(1), Length(2)],
    };
    let rects = Layout::vertical(constraints).split(area);
    let (footer, content, bar) = match mode {
        ChromeMode::Full => (Some(rects[2]), rects[1], rects[3]),
        _ => (None, rects[1], rects[2]),
    };
    Some(Chrome {
        mode,
        header: rects[0],
        footer,
        content,
        bar,
    })
}

fn colors_enabled() -> bool {
    static YES: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *YES.get_or_init(|| std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()))
}

pub fn accent() -> Style {
    if colors_enabled() {
        Style::new().fg(crate::ui::theme::current().accent)
    } else {
        Style::new()
    }
}

pub fn dim() -> Style {
    if colors_enabled() {
        Style::new().fg(crate::ui::theme::current().muted)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    }
}

pub fn strong() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

pub fn success_style() -> Style {
    if colors_enabled() {
        Style::new().fg(crate::ui::theme::current().success)
    } else {
        strong()
    }
}

pub fn selected() -> Style {
    if colors_enabled() {
        let p = crate::ui::theme::current();
        Style::new()
            .bg(p.selection_bg)
            .fg(p.selection_fg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)
    }
}

pub(crate) fn card_style(colors: bool) -> Style {
    if colors {
        Style::new().bg(crate::ui::theme::current().card_bg)
    } else {
        Style::new()
    }
}

pub(crate) fn card_border(colors: bool) -> Style {
    if colors {
        Style::new().fg(crate::ui::theme::current().border)
    } else {
        Style::new()
    }
}

fn pane_caption(title: &str) -> Line<'static> {
    let style = if colors_enabled() {
        Style::new()
            .fg(crate::ui::theme::current().caption)
            .add_modifier(Modifier::BOLD)
    } else {
        strong()
    };
    Line::from(vec![
        Span::raw(" "),
        Span::styled(format!("{title} "), style),
    ])
}

pub fn emphasis_border() -> Style {
    if colors_enabled() {
        let p = crate::ui::theme::current();
        match p.border_kind {
            crate::ui::theme::BorderKind::Rounded => Style::new().fg(p.accent),
            crate::ui::theme::BorderKind::Plain => Style::new().fg(p.border),
        }
    } else {
        Style::new()
    }
}

fn border_type_of(kind: crate::ui::theme::BorderKind) -> ratatui::widgets::BorderType {
    match kind {
        crate::ui::theme::BorderKind::Rounded => ratatui::widgets::BorderType::Rounded,
        crate::ui::theme::BorderKind::Plain => ratatui::widgets::BorderType::Plain,
    }
}

fn card_base(padding: ratatui::widgets::Padding) -> ratatui::widgets::Block<'static> {
    ratatui::widgets::Block::bordered()
        .border_type(border_type_of(crate::ui::theme::current().border_kind))
        .border_style(card_border(colors_enabled()))
        .style(card_style(colors_enabled()))
        .padding(padding)
}

pub fn input_box(focused: bool) -> ratatui::widgets::Block<'static> {
    let block = ratatui::widgets::Block::bordered()
        .border_type(border_type_of(crate::ui::theme::current().border_kind));
    if focused {
        block.border_style(accent().add_modifier(Modifier::BOLD))
    } else {
        block.border_style(card_border(colors_enabled()))
    }
}

pub fn genre_dot(title: &str) -> ratatui::style::Style {
    use ratatui::style::Color;
    if !colors_enabled() {
        return ratatui::style::Style::new();
    }
    const PALETTE: [Color; 12] = [
        Color::LightRed,
        Color::LightGreen,
        Color::LightBlue,
        Color::LightYellow,
        Color::LightMagenta,
        Color::LightCyan,
        Color::Red,
        Color::Green,
        Color::Blue,
        Color::Yellow,
        Color::Magenta,
        Color::Cyan,
    ];
    let idx = title
        .to_ascii_lowercase()
        .bytes()
        .map(|b| b as usize)
        .fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b))
        % PALETTE.len();
    ratatui::style::Style::new().fg(PALETTE[idx])
}

pub fn card(title: &str) -> ratatui::widgets::Block<'static> {
    card_base(ratatui::widgets::Padding::new(2, 2, 0, 1)).title(pane_caption(title))
}

pub fn popup_card(title: &str) -> ratatui::widgets::Block<'static> {
    card_base(ratatui::widgets::Padding::new(2, 2, 0, 0)).title(pane_caption(title))
}

pub fn no_color_active() -> bool {
    !colors_enabled()
}

pub fn error_style() -> Style {
    if colors_enabled() {
        Style::new().fg(crate::ui::theme::current().error)
    } else {
        strong()
    }
}

pub fn render_home(app: &mut App, frame: &mut Frame) {
    let Some(c) = layout_ladder(frame.area()) else {
        return;
    };
    render_header(app, &c, frame);
    render_tab_content(app, &c, frame);
    if c.footer.is_some() {
        render_footer(app, &c, frame);
    }
    render_bar(app, &c, frame);
}

fn spinner_glyph() -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    FRAMES[((ms / 120) % 10) as usize]
}

pub fn render_header(app: &mut App, c: &Chrome, f: &mut Frame) {
    let mut spans = vec![
        Span::styled("♪ ", accent()),
        Span::styled("freegal", strong()),
    ];
    if c.mode != ChromeMode::Minimal {
        spans.push(Span::raw("  ").style(dim()));
        let mut x: usize = 11;
        for (i, t) in Tab::ALL.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" │ ").style(dim()));
                x += 3;
            }
            let start = x;
            let active = *t == app.tab();
            let label = if active {
                format!("[● {}]", t.title())
            } else {
                t.title().to_string()
            };
            spans.push(Span::styled(
                label.clone(),
                if active {
                    accent().add_modifier(Modifier::BOLD)
                } else {
                    dim()
                },
            ));
            let width = label.chars().count();
            x += width;
            app.click_zones.push((
                Rect::new(c.header.x + start as u16, c.header.y, width as u16, 1),
                ClickZone::Tab(i),
            ));
        }
    }
    let left_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    f.render_widget(Paragraph::new(Line::from(spans)), c.header);

    if c.mode != ChromeMode::Minimal {
        let signed = matches!(
            (app.downloads_used, app.downloads_limit),
            (Some(_), Some(_))
        );
        let tail = match &app.stream_time_remaining {
            Some(crate::api::StreamTime::Unlimited(s)) if s == "UNLIMITED" => {
                Some("Stream: ∞".to_string())
            }
            Some(crate::api::StreamTime::Seconds(s)) => Some(format!(
                "Stream: {}",
                crate::ui::search::fmt_secs(Some(*s as u64))
            )),
            _ => None,
        };
        let candidates: Vec<String> = match (signed, &tail) {
            (true, Some(t)) => {
                let (u, l) = (app.downloads_used.unwrap(), app.downloads_limit.unwrap());
                vec![
                    format!("Downloads: {u}/{l} · Resets Mon | {t}"),
                    format!("Downloads: {u}/{l} | {t}"),
                    format!("Downloads: {u}/{l}"),
                ]
            }
            (true, None) => {
                let (u, l) = (app.downloads_used.unwrap(), app.downloads_limit.unwrap());
                vec![
                    format!("Downloads: {u}/{l} · Resets Mon"),
                    format!("Downloads: {u}/{l}"),
                ]
            }
            _ => vec!["not signed in".to_string()],
        };
        let prefix = if app.net_active {
            format!("{} ", spinner_glyph())
        } else {
            String::new()
        };
        let budget = c
            .header
            .width
            .saturating_sub((left_len + 2 + prefix.chars().count()) as u16)
            as usize;
        if let Some(chip) = candidates.into_iter().find(|c| c.chars().count() <= budget) {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(format!("{prefix}{chip}"), dim())))
                    .alignment(Alignment::Right),
                c.header,
            );
        }
    }
}

pub fn render_tab_content(app: &mut App, c: &Chrome, f: &mut Frame) {
    let searching = match app.search.top() {
        Some(crate::search::SearchView::Root(h)) => {
            h.focus == crate::search::Focus::Input || !h.input.value().trim().is_empty()
        }
        Some(_) => true,
        None => false,
    };
    let inner = if c.mode == ChromeMode::Minimal {
        let title = if searching {
            "Search"
        } else {
            app.tab().title()
        };
        let block = card(title);
        f.render_widget(&block, c.content);
        block.inner(c.content)
    } else {
        c.content
    };
    match app.tab() {
        Tab::Home => {
            if searching {
                crate::ui::search::render(app, inner, f);
            } else {
                crate::ui::home::render(app, inner, f);
            }
        }
        Tab::MyMusic => crate::ui::mymusic::render(app, inner, f),
        Tab::Queue => crate::ui::queue::render(app, inner, f),
        Tab::History => crate::ui::history::render(app, inner, f),
        Tab::Downloads => crate::ui::downloads::render(app, inner, f),
    }
}

pub const KEYMAP: &[(&str, &str, bool)] = &[
    ("1-5 / s m q d h", "switch tab", true),
    ("/", "edit search / filter", true),
    ("Enter", "play now", true),
    ("a / A", "append / play next", true),
    ("y", "download", true),
    ("S", "cycle sort", false),
    ("D", "clear history", false),
    ("Space", "play-pause", false),
    ("n / p", "next / prev", false),
    (", / .", "seek 5s", false),
    ("- / + / =", "volume", false),
    ("l", "loop current song", false),
    ("d", "remove queue row", false),
    ("J / K", "move queue row", false),
    ("Esc", "back", true),
    ("g / G", "top / bottom", false),
    ("R / e", "retry / error log", false),
    ("?", "help", true),
    ("o", "audio output", true),
    ("Ctrl-Q / Ctrl-C", "quit", true),
];

const HINTS: &[(&str, &str)] = &[
    ("Enter", "play"),
    ("a", "queue"),
    ("y", "download"),
    ("?", "help"),
    ("1-5", "tabs"),
    ("Ctrl-Q", "quit"),
];

const HINTS_LOGIN: &[(&str, &str)] = &[("Enter", "continue"), ("Esc", "back"), ("Ctrl-C", "quit")];

fn hints_line(table: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (k, action)) in table.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" · ").style(dim()));
        }
        spans.push(Span::raw("[").style(accent()));
        spans.push(Span::styled((*k).to_string(), strong()));
        spans.push(Span::raw("]").style(accent()));
        spans.push(Span::raw(format!(" {action}")).style(dim()));
    }
    Line::from(spans)
}

pub fn hint_line() -> Line<'static> {
    hints_line(HINTS)
}

pub fn hint_login() -> Line<'static> {
    hints_line(HINTS_LOGIN)
}

pub fn render_footer(app: &App, c: &Chrome, f: &mut Frame) {
    let row = c.footer.expect("footer checked");
    let left = hint_line();
    let left_len = left.width();
    f.render_widget(Paragraph::new(left), row);
    let right = format!(
        "queue {} · Vol {}%{}",
        app.queue.len(),
        app.volume,
        if app.loop_one { " · ↻" } else { "" }
    );
    if row.width as usize >= right.len() + left_len + 2 {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(right, dim()))).alignment(Alignment::Right),
            row,
        );
    }
}

pub fn render_bar(app: &mut App, c: &Chrome, f: &mut Frame) {
    let playing = app.player_degraded.is_some() || app.queue.current().is_some();
    if !playing {
        return;
    }
    let pill = c.bar.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 0,
    });
    f.render_widget(
        ratatui::widgets::Block::default().style(card_style(colors_enabled())),
        pill,
    );
    let row = Layout::vertical(vec![Length(1), Length(1)]).split(pill);
    let [l1, l2] = [row[0], row[1]];
    let [el, gr] = Layout::horizontal(vec![Length(12), Fill(1)]).areas(l2);
    if app.player_status.duration.is_some() {
        app.click_zones.push((gr, ClickZone::Progress));
    }

    let (label, label_style) = if app.player_degraded.is_some() {
        ("✕ DEAD", error_style())
    } else {
        match app.queue.current() {
            Some((_, it)) => {
                let playing_status = it.status == crate::queue::QueueStatus::Playing;
                match (playing_status, app.player_status.paused) {
                    (true, Some(false)) => ("▶ PLAYING", accent()),
                    (true, _) => ("‖ PAUSED", accent()),
                    _ => ("▶ PLAYING", accent()),
                }
            }
            None => ("▶ IDLE", dim()),
        }
    };
    let mut left = vec![Span::styled(label, label_style), Span::raw(" ")];
    match app.queue.current() {
        Some((_, it)) => {
            let title = it.song.title.clone();
            let artist = it
                .song
                .artist
                .as_ref()
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "unknown artist".into());
            left.push(Span::styled(format!("{title} — {artist}"), strong()));
        }
        None => {
            left.push(Span::styled("playback unavailable", dim()));
        }
    }
    if app.loop_one {
        left.push(Span::styled(" ↻", accent()));
    }
    f.render_widget(Paragraph::new(Line::from(left)), l1);
    let (elapsed_txt, total_txt) = (
        crate::ui::search::fmt_secs(app.player_status.time.map(|t| t as u64)),
        crate::ui::search::fmt_secs(app.player_status.duration.map(|d| d as u64)),
    );
    let frac = match (app.player_status.time, app.player_status.duration) {
        (Some(t), Some(d)) if d > 0.0 => Some((t / d).clamp(0.0, 1.0)),
        _ => None,
    };
    f.render_widget(
        Paragraph::new(Span::styled(format!("{elapsed_txt} / {total_txt}"), dim())),
        el,
    );
    let width = gr.width.max(1) as usize;
    let gauge = match frac {
        Some(f) => {
            let filled = (f * (width as f64 - 1.0)).round() as usize;
            let rest = width - filled - 1;
            Line::from(vec![
                Span::styled("━".repeat(filled), accent()),
                Span::styled("●", accent().add_modifier(Modifier::BOLD)),
                Span::styled("─".repeat(rest), dim()),
            ])
        }
        None => Line::from(Span::styled("·".repeat(width), dim())),
    };
    f.render_widget(Paragraph::new(gauge).alignment(Alignment::Right), gr);
}

pub fn render_too_small(f: &mut Frame) {
    let area = f.area();
    let center = Layout::vertical(vec![Fill(1), Length(2), Fill(1)]).split(area)[1];
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("terminal too small", strong())).alignment(Alignment::Center),
            Line::from(format!("needs at least {MIN_WIDTH}x{MIN_HEIGHT}"))
                .style(dim())
                .alignment(Alignment::Center),
        ]),
        center,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    #[test]
    fn card_styles_degrade_without_color() {
        use ratatui::style::Color;
        assert_eq!(
            card_style(true),
            Style::new().bg(Color::Rgb(0x1e, 0x20, 0x30))
        );
        assert_eq!(card_style(false), Style::new());
        assert_eq!(
            card_border(true),
            Style::new().fg(Color::Rgb(0x41, 0x48, 0x68))
        );
        assert_eq!(card_border(false), Style::new());
    }

    #[test]
    fn below_floor_is_none() {
        assert!(layout_ladder(rect(MIN_WIDTH - 1, 10)).is_none());
        assert!(layout_ladder(rect(40, MIN_HEIGHT - 1)).is_none());
        assert!(layout_ladder(rect(1, 1)).is_none());
    }

    #[test]
    fn minimal_rung_at_floor() {
        let c = layout_ladder(rect(40, 10)).unwrap();
        assert_eq!(c.mode, ChromeMode::Minimal);
        assert!(c.footer.is_none());
        assert_eq!(c.header.height, 1);
        assert_eq!(c.content.height, 7);
        assert_eq!(c.bar.height, 2);
    }

    #[test]
    fn folded_tabs_rung() {
        let c = layout_ladder(rect(60, 14)).unwrap();
        assert_eq!(c.mode, ChromeMode::FoldedTabs);
        assert!(c.footer.is_none());
        assert_eq!(c.content.height, 11);
    }

    #[test]
    fn no_hint_rung() {
        let c = layout_ladder(rect(80, 18)).unwrap();
        assert_eq!(c.mode, ChromeMode::NoHint);
        assert!(c.footer.is_none());
        assert_eq!(c.content.height, 15);
    }

    #[test]
    fn full_rung() {
        let c = layout_ladder(rect(80, 24)).unwrap();
        assert_eq!(c.mode, ChromeMode::Full);
        assert!(c.footer.is_some());
        assert_eq!(c.header.height, 1);
        assert_eq!(c.footer.unwrap().height, 1);
        assert_eq!(c.content.height, 20);
        assert_eq!(c.bar.height, 2);
    }

    #[test]
    fn regions_fill_the_width() {
        for (w, h) in [(40u16, 10), (60, 14), (80, 18), (80, 24), (120, 50)] {
            let c = layout_ladder(rect(w, h)).unwrap();
            assert_eq!(c.header.width, w);
            assert_eq!(c.content.width, w);
            assert_eq!(c.bar.width, w);
        }
    }
}
