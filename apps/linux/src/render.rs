//! Draws a `shrew_core::Frame` with Cairo + Pango.

use gtk::cairo;
use gtk::pango;
use shrew_core::{CursorStyle, Frame, Rgb, Run, Underline};

pub const FONT_FAMILY: &str = "JetBrains Mono, Cascadia Mono, Fira Code, DejaVu Sans Mono, monospace";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub cell_w: f64,
    pub cell_h: f64,
    pub ascent: f64,
}

pub struct Renderer {
    pub font_size: f64,
    fonts: [pango::FontDescription; 4],
    pub metrics: Metrics,
}

impl Renderer {
    pub fn new(ctx: &pango::Context, font_size: f64) -> Self {
        let mut r = Self {
            font_size,
            fonts: std::array::from_fn(|_| pango::FontDescription::new()),
            metrics: Metrics { cell_w: 8.0, cell_h: 16.0, ascent: 12.0 },
        };
        r.set_font_size(ctx, font_size);
        r
    }

    pub fn set_font_size(&mut self, ctx: &pango::Context, size: f64) {
        self.font_size = size.clamp(6.0, 36.0);
        for (i, f) in self.fonts.iter_mut().enumerate() {
            *f = pango::FontDescription::from_string(FONT_FAMILY);
            f.set_absolute_size(self.font_size * pango::SCALE as f64);
            f.set_weight(if i & 1 == 1 { pango::Weight::Bold } else { pango::Weight::Normal });
            f.set_style(if i & 2 == 2 { pango::Style::Italic } else { pango::Style::Normal });
        }
        let layout = pango::Layout::new(ctx);
        layout.set_font_description(Some(&self.fonts[0]));
        layout.set_text("MMMMMMMMMM");
        let (_, logical) = layout.pixel_extents();
        let baseline = layout.baseline() as f64 / pango::SCALE as f64;
        // Measure precisely in Pango units for fractional advances.
        let (_, log_units) = layout.extents();
        self.metrics = Metrics {
            cell_w: log_units.width() as f64 / pango::SCALE as f64 / 10.0,
            cell_h: (logical.height() as f64 * 1.04).ceil(),
            ascent: baseline + (logical.height() as f64 * 0.02),
        };
    }

    pub fn grid(&self, w: f64, h: f64, pad: f64) -> (u16, u16) {
        (((w - pad * 2.0) / self.metrics.cell_w).max(10.0) as u16, ((h - pad * 2.0) / self.metrics.cell_h).max(4.0) as u16)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        cr: &cairo::Context,
        pctx: &pango::Context,
        f: &Frame,
        w: f64,
        h: f64,
        pad: f64,
        scale: f64,
        focused: bool,
        cursor_on: bool,
    ) {
        set(cr, f.background);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();
        cr.save().ok();
        cr.translate(pad, pad);
        cr.scale(scale, scale);
        let m = self.metrics;
        let sel = mix(f.foreground, f.background, 0.30);
        let layout = pango::Layout::new(pctx);

        for (row, line) in f.lines.iter().enumerate() {
            let top = row as f64 * m.cell_h;
            for run in &line.runs {
                let bg = if run.selected { Some(sel) } else { run.bg };
                if let Some(bg) = bg {
                    set(cr, bg);
                    cr.rectangle(run.col as f64 * m.cell_w, top, run.cells as f64 * m.cell_w, m.cell_h + 0.5);
                    let _ = cr.fill();
                }
            }
            for run in &line.runs {
                if run.text.chars().all(|c| c == ' ') {
                    continue;
                }
                let fg = if run.faint { mix(run.fg, f.background, 0.5) } else { run.fg };
                let font = &self.fonts[(run.bold as usize) | ((run.italic as usize) << 1)];
                layout.set_font_description(Some(font));
                self.draw_run(cr, &layout, run, fg, top);
                self.decorate(cr, run, run.underline_color.unwrap_or(fg), top);
            }
        }

        if let Some(c) = &f.cursor
            && cursor_on && f.scrollbar.at_bottom() {
                let x = c.col as f64 * m.cell_w;
                let y = c.row as f64 * m.cell_h;
                let cw = if c.wide { m.cell_w * 2.0 } else { m.cell_w };
                set(cr, f.cursor_color.unwrap_or(f.foreground));
                if !focused || c.style == CursorStyle::HollowBlock {
                    cr.set_line_width(1.0);
                    cr.rectangle(x + 0.5, y + 0.5, cw - 1.0, m.cell_h - 1.0);
                    let _ = cr.stroke();
                } else {
                    match c.style {
                        CursorStyle::Bar => cr.rectangle(x, y, 2.0, m.cell_h),
                        CursorStyle::Underline => cr.rectangle(x, y + m.cell_h - 2.0, cw, 2.0),
                        _ => cr.rectangle(x, y, cw, m.cell_h),
                    }
                    let _ = cr.fill();
                    if c.style == CursorStyle::Block
                        && let Some(run) =
                            f.lines.get(c.row as usize).and_then(|l| l.runs.iter().find(|r| c.col >= r.col && c.col < r.col + r.cells))
                        {
                            let idx = grapheme_index(&run.widths, (c.col - run.col) as usize);
                            if let Some(ch) = run.text.chars().nth(idx).filter(|ch| *ch != ' ') {
                                layout.set_font_description(Some(&self.fonts[(run.bold as usize) | ((run.italic as usize) << 1)]));
                                set(cr, f.background);
                                layout.set_text(&ch.to_string());
                                cr.move_to(x, y + (m.cell_h - layout.pixel_size().1 as f64) / 2.0);
                                pangocairo::functions::show_layout(cr, &layout);
                            }
                        }
                }
            }
        cr.restore().ok();

        let sb = f.scrollbar;
        if sb.total > sb.len && !sb.at_bottom() {
            let track = h - pad * 2.0;
            let thumb = (track * sb.len as f64 / sb.total as f64).max(24.0);
            let y = pad + (track - thumb) * sb.offset as f64 / (sb.total - sb.len) as f64;
            set(cr, mix(f.foreground, f.background, 0.55));
            cr.rectangle(w - 5.0, y, 3.0, thumb);
            let _ = cr.fill();
        }
    }

    fn draw_run(&self, cr: &cairo::Context, layout: &pango::Layout, run: &Run, fg: Rgb, top: f64) {
        let m = self.metrics;
        set(cr, fg);
        let simple = run.widths.iter().all(|w| *w == 1) && run.text.is_ascii();
        if simple {
            layout.set_text(&run.text);
            cr.move_to(run.col as f64 * m.cell_w, top + (m.cell_h - layout.pixel_size().1 as f64) / 2.0);
            pangocairo::functions::show_layout(cr, layout);
            return;
        }
        // Place every grapheme on its cell; draw box characters as geometry.
        let mut col = run.col as f64;
        for (i, ch) in run.text.chars().enumerate() {
            let w = *run.widths.get(i).unwrap_or(&1) as f64;
            let x = col * m.cell_w;
            if !self.box_char(cr, ch as u32, x, top, w) && ch != ' ' {
                layout.set_text(&ch.to_string());
                let (lw, lh) = layout.pixel_size();
                let dx = ((w * m.cell_w) - lw as f64).max(0.0) / 2.0;
                cr.move_to(x + dx, top + (m.cell_h - lh as f64) / 2.0);
                pangocairo::functions::show_layout(cr, layout);
            }
            col += w;
        }
    }

    fn box_char(&self, cr: &cairo::Context, cp: u32, x: f64, top: f64, cells: f64) -> bool {
        if !(0x2500..=0x259F).contains(&cp) {
            return false;
        }
        let m = self.metrics;
        let (w, h) = (m.cell_w * cells, m.cell_h);
        let (cx, cy, r, b) = (x + w / 2.0, top + h / 2.0, x + w, top + h);
        let t = 1.0;
        let hl = |a: f64, z: f64, th: f64| cr.rectangle(a, cy - th / 2.0, z - a, th);
        let vl = |a: f64, z: f64, th: f64| cr.rectangle(cx - th / 2.0, a, th, z - a);
        match cp {
            0x2500 => hl(x, r, t),
            0x2501 => hl(x, r, 2.0),
            0x2502 => vl(top, b, t),
            0x2503 => vl(top, b, 2.0),
            0x250C | 0x256D => {
                hl(cx - t / 2.0, r, t);
                vl(cy - t / 2.0, b, t)
            }
            0x2510 | 0x256E => {
                hl(x, cx + t / 2.0, t);
                vl(cy - t / 2.0, b, t)
            }
            0x2514 | 0x2570 => {
                hl(cx - t / 2.0, r, t);
                vl(top, cy + t / 2.0, t)
            }
            0x2518 | 0x256F => {
                hl(x, cx + t / 2.0, t);
                vl(top, cy + t / 2.0, t)
            }
            0x251C => {
                vl(top, b, t);
                hl(cx, r, t)
            }
            0x2524 => {
                vl(top, b, t);
                hl(x, cx, t)
            }
            0x252C => {
                hl(x, r, t);
                vl(cy, b, t)
            }
            0x2534 => {
                hl(x, r, t);
                vl(top, cy, t)
            }
            0x253C => {
                hl(x, r, t);
                vl(top, b, t)
            }
            0x2580 => cr.rectangle(x, top, w, h / 2.0),
            0x2584 => cr.rectangle(x, cy, w, h / 2.0),
            0x2588 => cr.rectangle(x, top, w, h),
            0x258C => cr.rectangle(x, top, w / 2.0, h),
            0x2590 => cr.rectangle(cx, top, w / 2.0, h),
            0x2581..=0x2587 => {
                let f = (cp - 0x2580) as f64 / 8.0;
                cr.rectangle(x, b - h * f, w, h * f)
            }
            0x2589..=0x258F => cr.rectangle(x, top, w * (0x2590 - cp) as f64 / 8.0, h),
            _ => return false,
        }
        let _ = cr.fill();
        true
    }

    fn decorate(&self, cr: &cairo::Context, run: &Run, color: Rgb, top: f64) {
        if run.underline == Underline::None && !run.strikethrough && !run.overline {
            return;
        }
        let m = self.metrics;
        let (x0, x1) = (run.col as f64 * m.cell_w, (run.col + run.cells) as f64 * m.cell_w);
        set(cr, color);
        cr.set_line_width(1.0);
        let uy = top + m.cell_h - 1.5;
        let line = |y: f64| {
            cr.move_to(x0, y);
            cr.line_to(x1, y);
            let _ = cr.stroke();
        };
        match run.underline {
            Underline::None => {}
            Underline::Double => {
                line(uy);
                line(uy - 2.5)
            }
            Underline::Curly => {
                cr.move_to(x0, uy);
                let mut x = x0;
                let mut up = true;
                while x < x1 {
                    let nx = (x + 2.5).min(x1);
                    cr.curve_to(x + 0.8, uy + if up { -2.0 } else { 2.0 }, nx - 0.8, uy + if up { -2.0 } else { 2.0 }, nx, uy);
                    x = nx;
                    up = !up;
                }
                let _ = cr.stroke();
            }
            Underline::Dotted | Underline::Dashed => {
                cr.set_dash(if run.underline == Underline::Dotted { &[1.0, 2.0] } else { &[3.0, 3.0] }, 0.0);
                line(uy);
                cr.set_dash(&[], 0.0);
            }
            Underline::Single => line(uy),
        }
        if run.strikethrough {
            line(top + m.cell_h / 2.0)
        }
        if run.overline {
            line(top + 0.5)
        }
    }
}

fn grapheme_index(widths: &[u8], off: usize) -> usize {
    let mut c = 0;
    for (i, w) in widths.iter().enumerate() {
        if off < c + *w as usize {
            return i;
        }
        c += *w as usize;
    }
    widths.len().saturating_sub(1)
}

pub fn set(cr: &cairo::Context, c: Rgb) {
    cr.set_source_rgb(c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0);
}

pub fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let m = |x: u8, y: u8| (x as f64 * (1.0 - t) + y as f64 * t).round() as u8;
    Rgb::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::prelude::*;

    /// Renders a real libghostty-vt frame offscreen; set SHREW_RENDER_PNG to keep the image.
    #[test]
    fn renders_frame_offscreen() {
        let mut t = shrew_term::Terminal::new(60, 12);
        t.set_default_colors(Some(Rgb::new(0xe6, 0xed, 0xf3)), Some(Rgb::new(0x0d, 0x11, 0x17)), Some(Rgb::new(0x34, 0xd3, 0x99)));
        t.write_vt("\x1b[1mShrew on Linux\x1b[0m — Cairo + Pango\r\n\r\n\x1b[1;35mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[4:3mcurly\x1b[0m \x1b[9mstrike\x1b[0m \x1b[7m inverse \x1b[0m\r\nwide: 中文字 ok\r\n┌──────┬──────┐\r\n│ box  │ draw │\r\n└──────┴──────┘\r\n$ ".as_bytes());
        let frame = t.frame();
        let fm = pangocairo::FontMap::new();
        let pctx = fm.create_context();
        let r = Renderer::new(&pctx, 14.0);
        let (w, h) = ((60.0 * r.metrics.cell_w + 12.0) as i32, (12.0 * r.metrics.cell_h + 12.0) as i32);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w * 2, h * 2).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        cr.scale(2.0, 2.0);
        pangocairo::functions::update_context(&cr, &pctx);
        r.draw(&cr, &pctx, &frame, w as f64, h as f64, 6.0, 1.0, true, true);
        drop(cr);
        assert!(r.metrics.cell_w > 4.0 && r.metrics.cell_h > 8.0);
        if let Ok(p) = std::env::var("SHREW_RENDER_PNG") {
            let mut surf = surf;
            let stride = surf.stride() as usize;
            let (sw, sh) = (surf.width(), surf.height());
            let data = surf.data().unwrap().to_vec();
            let tex = gtk::gdk::MemoryTexture::new(
                sw,
                sh,
                gtk::gdk::MemoryFormat::B8g8r8a8Premultiplied,
                &gtk::glib::Bytes::from_owned(data),
                stride,
            );
            tex.save_to_png(p).unwrap();
        }
    }
}
