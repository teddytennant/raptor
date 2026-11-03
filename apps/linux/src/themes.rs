//! Terminal themes, shared in spirit with the iOS and Android apps: the same
//! names and colors, and the window chrome is derived from the terminal.

use shrew_core::{ColorTheme, Rgb};

pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub bg: u32,
    pub fg: u32,
    pub cursor: u32,
    pub accent: u32,
    /// `None` means use whatever colors the server sends.
    pub ansi: Option<[u32; 16]>,
}

pub const THEMES: &[Theme] = &[
    Theme {
        id: "shrew",
        name: "Shrew",
        bg: 0x0D1117,
        fg: 0xE6EDF3,
        cursor: 0x5EE0A8,
        accent: 0x5EE0A8,
        ansi: Some([
            0x484F58, 0xFF7B72, 0x3FB950, 0xD29922, 0x58A6FF, 0xBC8CFF, 0x39C5CF, 0xB1BAC4, 0x6E7681, 0xFFA198, 0x56D364, 0xE3B341, 0x79C0FF,
            0xD2A8FF, 0x56D4DD, 0xFFFFFF,
        ]),
    },
    Theme {
        id: "midnight",
        name: "Midnight",
        bg: 0x0F1626,
        fg: 0xD8DEE9,
        cursor: 0x88C0D0,
        accent: 0x8AB4F8,
        ansi: Some([
            0x3B4252, 0xE06C75, 0xA3BE8C, 0xE5C07B, 0x81A1C1, 0xB48EAD, 0x88C0D0, 0xE5E9F0, 0x4C566A, 0xEF8891, 0xB5D19E, 0xF0D08C, 0x8AB4F8,
            0xC8A2C8, 0x8FBCBB, 0xECEFF4,
        ]),
    },
    Theme {
        id: "mocha",
        name: "Catppuccin Mocha",
        bg: 0x1E1E2E,
        fg: 0xCDD6F4,
        cursor: 0xF5E0DC,
        accent: 0xCBA6F7,
        ansi: Some([
            0x45475A, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xBAC2DE, 0x585B70, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA,
            0xF5C2E7, 0x94E2D5, 0xA6ADC8,
        ]),
    },
    Theme {
        id: "gruvbox",
        name: "Gruvbox",
        bg: 0x282828,
        fg: 0xEBDBB2,
        cursor: 0xEBDBB2,
        accent: 0xFABD2F,
        ansi: Some([
            0x3C3836, 0xCC241D, 0x98971A, 0xD79921, 0x458588, 0xB16286, 0x689D6A, 0xA89984, 0x928374, 0xFB4934, 0xB8BB26, 0xFABD2F, 0x83A598,
            0xD3869B, 0x8EC07C, 0xEBDBB2,
        ]),
    },
    Theme {
        id: "solarized-light",
        name: "Solarized Light",
        bg: 0xFDF6E3,
        fg: 0x586E75,
        cursor: 0x586E75,
        accent: 0x268BD2,
        ansi: Some([
            0x073642, 0xDC322F, 0x859900, 0xB58900, 0x268BD2, 0xD33682, 0x2AA198, 0xEEE8D5, 0x002B36, 0xCB4B16, 0x586E75, 0x657B83, 0x839496,
            0x6C71C4, 0x93A1A1, 0xFDF6E3,
        ]),
    },
    Theme { id: "server", name: "Server colors", bg: 0x0D1117, fg: 0xE6EDF3, cursor: 0x5EE0A8, accent: 0x5EE0A8, ansi: None },
];

pub fn by_id(id: &str) -> &'static Theme {
    THEMES.iter().find(|t| t.id == id).unwrap_or(&THEMES[0])
}

fn rgb(v: u32) -> Rgb {
    Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Theme {
    pub fn color_theme(&self) -> Option<ColorTheme> {
        let ansi = self.ansi?;
        Some(ColorTheme { foreground: rgb(self.fg), background: rgb(self.bg), cursor: rgb(self.cursor), ansi: ansi.map(rgb) })
    }

    pub fn is_light(&self) -> bool {
        rgb(self.bg).luminance() > 0.4
    }

    /// CSS that tints libadwaita's chrome to sit around this terminal.
    pub fn css(&self) -> String {
        let mix = |a: u32, b: u32, t: f32| -> String {
            let (a, b) = (rgb(a), rgb(b));
            let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
            format!("#{:02x}{:02x}{:02x}", m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
        };
        let hex = |v: u32| format!("#{v:06x}");
        let (bg, fg, acc) = (self.bg, self.fg, self.accent);
        format!(
            r#"
            @define-color window_bg_color {win};
            @define-color window_fg_color {fgc};
            @define-color view_bg_color {win};
            @define-color view_fg_color {fgc};
            @define-color headerbar_bg_color {win};
            @define-color headerbar_fg_color {fgc};
            @define-color headerbar_backdrop_color {win};
            @define-color sidebar_bg_color {side};
            @define-color sidebar_fg_color {fgc};
            @define-color sidebar_backdrop_color {side};
            @define-color card_bg_color {raised};
            @define-color popover_bg_color {raised};
            @define-color popover_fg_color {fgc};
            @define-color dialog_bg_color {raised};
            @define-color dialog_fg_color {fgc};
            @define-color accent_bg_color {acc};
            @define-color accent_color {acc};
            @define-color accent_fg_color {onacc};
            .shrew-terminal {{ background: {bgc}; }}
            .shrew-panes {{ background: {gap}; }}
            .shrew-ring {{ border: 1px solid transparent; }}
            .shrew-pane.focused .shrew-ring {{ border-color: alpha({acc}, 0.7); }}
            .shrew-pill {{ background: alpha({raised}, 0.94); color: {fgc}; }}
            .shrew-tile {{ background: alpha({acc}, 0.16); color: {acc}; border-radius: 7px; min-width: 26px; min-height: 26px; font-family: "JetBrains Mono", monospace; font-weight: 700; }}
            .shrew-palette row {{ padding: 6px 10px; border-radius: 8px; }}
            .shrew-palette .cat {{ font-size: 0.75em; font-weight: 700; opacity: 0.55; letter-spacing: 0.05em; }}
            .shrew-palette .glyph {{ color: {acc}; font-family: "JetBrains Mono", monospace; min-width: 18px; }}
            "#,
            win = mix(bg, fg, 0.04),
            side = mix(bg, fg, 0.07),
            raised = mix(bg, fg, 0.10),
            gap = mix(bg, fg, 0.16),
            bgc = hex(bg),
            fgc = hex(fg),
            acc = hex(acc),
            onacc = if rgb(acc).luminance() > 0.35 { "#0b0d10" } else { "#ffffff" },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_resolve_and_build_css() {
        assert_eq!(by_id("nope").id, "shrew");
        assert!(by_id("solarized-light").is_light());
        assert!(!by_id("mocha").is_light());
        assert!(by_id("server").color_theme().is_none());
        let css = by_id("gruvbox").css();
        assert!(css.contains("#fabd2f") && css.contains("@define-color window_bg_color #"));
    }
}
