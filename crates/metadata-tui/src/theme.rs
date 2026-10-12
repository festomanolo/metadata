//! Restrained palette with truecolor, 16-color and monochrome fallbacks.
use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeKind {
    Dark,
    Light,
    Mono,
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub kind: ThemeKind,
    pub accent: Color,
    pub dim: Color,
    pub warn: Color,
    pub err: Color,
    pub ok: Color,
    pub border: Color,
    pub sel_bg: Color,
    pub sel_fg: Color,
}

pub fn truecolor() -> bool {
    std::env::var("COLORTERM")
        .map(|v| v == "truecolor" || v == "24bit")
        .unwrap_or(false)
}

impl Theme {
    pub fn new(kind: ThemeKind, truecolor: bool) -> Theme {
        let c = |rgb: (u8, u8, u8), fallback: Color| {
            if truecolor {
                Color::Rgb(rgb.0, rgb.1, rgb.2)
            } else {
                fallback
            }
        };
        match kind {
            ThemeKind::Dark => Theme {
                kind,
                accent: c((102, 204, 220), Color::Cyan),
                dim: c((128, 134, 148), Color::DarkGray),
                warn: c((230, 190, 90), Color::Yellow),
                err: c((235, 105, 105), Color::Red),
                ok: c((120, 200, 140), Color::Green),
                border: c((70, 76, 92), Color::DarkGray),
                sel_bg: c((38, 46, 62), Color::Blue),
                sel_fg: Color::Reset,
            },
            ThemeKind::Light => Theme {
                kind,
                accent: c((0, 110, 140), Color::Blue),
                dim: c((110, 114, 124), Color::DarkGray),
                warn: c((160, 105, 0), Color::Yellow),
                err: c((180, 40, 40), Color::Red),
                ok: c((20, 130, 60), Color::Green),
                border: c((170, 174, 184), Color::Gray),
                sel_bg: c((214, 228, 240), Color::Gray),
                sel_fg: Color::Black,
            },
            ThemeKind::Mono => Theme {
                kind,
                accent: Color::Reset,
                dim: Color::Reset,
                warn: Color::Reset,
                err: Color::Reset,
                ok: Color::Reset,
                border: Color::Reset,
                sel_bg: Color::Reset,
                sel_fg: Color::Reset,
            },
        }
    }
    pub fn mono(&self) -> bool {
        self.kind == ThemeKind::Mono
    }
    pub fn base(&self) -> Style {
        Style::default()
    }
    pub fn dim_s(&self) -> Style {
        if self.mono() { Style::default().add_modifier(Modifier::DIM) } else { Style::default().fg(self.dim) }
    }
    pub fn accent_s(&self) -> Style {
        if self.mono() { Style::default().add_modifier(Modifier::BOLD) } else { Style::default().fg(self.accent) }
    }
    pub fn warn_s(&self) -> Style {
        if self.mono() { Style::default().add_modifier(Modifier::ITALIC) } else { Style::default().fg(self.warn) }
    }
    pub fn err_s(&self) -> Style {
        if self.mono() { Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED) } else { Style::default().fg(self.err) }
    }
    pub fn ok_s(&self) -> Style {
        if self.mono() { Style::default() } else { Style::default().fg(self.ok) }
    }
    pub fn border_s(&self) -> Style {
        if self.mono() { Style::default() } else { Style::default().fg(self.border) }
    }
    pub fn sel_s(&self) -> Style {
        if self.mono() { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default().bg(self.sel_bg).fg(self.sel_fg) }
    }
    pub fn title_s(&self) -> Style {
        self.accent_s().add_modifier(Modifier::BOLD)
    }
}
