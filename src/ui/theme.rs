use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderKind {
    Rounded,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub name: &'static str,
    pub accent: Color,
    pub fg_bright: Color,
    pub muted: Color,
    pub border: Color,
    pub card_bg: Color,
    pub caption: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub error: Color,
    pub success: Color,
    pub border_kind: BorderKind,
}

pub const TOKYO_NIGHT: Palette = Palette {
    name: "tokyo-night",
    accent: Color::Rgb(0x7a, 0xa2, 0xf7),
    fg_bright: Color::Rgb(0xc0, 0xca, 0xf5),
    muted: Color::Rgb(0x56, 0x5f, 0x89),
    border: Color::Rgb(0x41, 0x48, 0x68),
    card_bg: Color::Rgb(0x1e, 0x20, 0x30),
    caption: Color::Rgb(0x56, 0x5f, 0x89),
    selection_bg: Color::Rgb(0x29, 0x2e, 0x42),
    selection_fg: Color::Rgb(0xc0, 0xca, 0xf5),
    error: Color::Rgb(0xf7, 0x76, 0x8e),
    success: Color::Rgb(0x9e, 0xce, 0x6a),
    border_kind: BorderKind::Rounded,
};

pub const NORD: Palette = Palette {
    name: "nord",
    accent: Color::Rgb(0x88, 0xc0, 0xd0),
    fg_bright: Color::Rgb(0xec, 0xef, 0xf4),
    muted: Color::Rgb(0x4c, 0x56, 0x6a),
    border: Color::Rgb(0x4c, 0x56, 0x6a),
    card_bg: Color::Rgb(0x3b, 0x42, 0x52),
    caption: Color::Rgb(0x88, 0xc0, 0xd0),
    selection_bg: Color::Rgb(0x43, 0x4c, 0x5e),
    selection_fg: Color::Rgb(0xec, 0xef, 0xf4),
    error: Color::Rgb(0xbf, 0x61, 0x6a),
    success: Color::Rgb(0xa3, 0xbe, 0x8c),
    border_kind: BorderKind::Plain,
};

static THEME: std::sync::OnceLock<&'static Palette> = std::sync::OnceLock::new();

#[cfg(test)]
thread_local! {
    static OVERRIDE: std::cell::RefCell<Option<&'static Palette>> =
        const { std::cell::RefCell::new(None) };
}

pub fn current() -> &'static Palette {
    #[cfg(test)]
    if let Some(p) = OVERRIDE.with(|c| *c.borrow()) {
        return p;
    }
    THEME.get_or_init(|| &TOKYO_NIGHT)
}

pub fn resolve(name: Option<&str>) -> &'static Palette {
    match name.map(|n| n.trim().to_ascii_lowercase()).as_deref() {
        Some("nord") => &NORD,
        _ => &TOKYO_NIGHT,
    }
}

pub fn init_from_omarchy() {
    let name = omarchy_theme_name();
    let palette = resolve(name.as_deref());
    tracing::info!(
        "theme: {} → {}",
        name.as_deref().unwrap_or("unset"),
        palette.name
    );
    let _ = THEME.set(palette);
}

fn omarchy_theme_name() -> Option<String> {
    let path = dirs::home_dir()?.join(".local/state/omarchy/current/theme.name");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
pub fn with_palette<T>(p: &'static Palette, f: impl FnOnce() -> T) -> T {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            OVERRIDE.with(|c| *c.borrow_mut() = None);
        }
    }
    OVERRIDE.with(|c| *c.borrow_mut() = Some(p));
    let _guard = Guard;
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_tokyo_night() {
        assert_eq!(current().name, "tokyo-night");
        assert_eq!(TOKYO_NIGHT.accent, Color::Rgb(0x7a, 0xa2, 0xf7));
        assert_eq!(TOKYO_NIGHT.fg_bright, Color::Rgb(0xc0, 0xca, 0xf5));
        assert_eq!(TOKYO_NIGHT.muted, Color::Rgb(0x56, 0x5f, 0x89));
        assert_eq!(TOKYO_NIGHT.border, Color::Rgb(0x41, 0x48, 0x68));
        assert_eq!(TOKYO_NIGHT.card_bg, Color::Rgb(0x1e, 0x20, 0x30));
        assert_eq!(TOKYO_NIGHT.caption, Color::Rgb(0x56, 0x5f, 0x89));
        assert_eq!(TOKYO_NIGHT.selection_bg, Color::Rgb(0x29, 0x2e, 0x42));
        assert_eq!(TOKYO_NIGHT.selection_fg, Color::Rgb(0xc0, 0xca, 0xf5));
        assert_eq!(TOKYO_NIGHT.error, Color::Rgb(0xf7, 0x76, 0x8e));
        assert_eq!(TOKYO_NIGHT.success, Color::Rgb(0x9e, 0xce, 0x6a));
        assert_eq!(TOKYO_NIGHT.border_kind, BorderKind::Rounded);
    }

    #[test]
    fn nord_palette_pins_and_resolution() {
        assert_eq!(resolve(Some("nord")).name, "nord");
        assert_eq!(resolve(Some("NORD")).name, "nord");
        assert_eq!(resolve(Some(" nord ")).name, "nord");
        assert_eq!(resolve(Some("tokyo-night")).name, "tokyo-night");
        assert_eq!(resolve(Some("catppuccin")).name, "tokyo-night");
        assert_eq!(resolve(None).name, "tokyo-night");
        assert_eq!(NORD.accent, Color::Rgb(0x88, 0xc0, 0xd0));
        assert_eq!(NORD.fg_bright, Color::Rgb(0xec, 0xef, 0xf4));
        assert_eq!(NORD.muted, Color::Rgb(0x4c, 0x56, 0x6a));
        assert_eq!(NORD.border, Color::Rgb(0x4c, 0x56, 0x6a));
        assert_eq!(NORD.card_bg, Color::Rgb(0x3b, 0x42, 0x52));
        assert_eq!(NORD.caption, Color::Rgb(0x88, 0xc0, 0xd0));
        assert_eq!(NORD.selection_bg, Color::Rgb(0x43, 0x4c, 0x5e));
        assert_eq!(NORD.selection_fg, Color::Rgb(0xec, 0xef, 0xf4));
        assert_eq!(NORD.error, Color::Rgb(0xbf, 0x61, 0x6a));
        assert_eq!(NORD.success, Color::Rgb(0xa3, 0xbe, 0x8c));
        assert_eq!(NORD.border_kind, BorderKind::Plain);
    }

    #[test]
    fn palette_override_scopes() {
        with_palette(&NORD, || assert_eq!(current().name, "nord"));
        assert_eq!(current().name, "tokyo-night");
    }
}
