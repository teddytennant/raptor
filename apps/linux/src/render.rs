//! Draws a `raptor_core::Frame` with Cairo + Pango.
//!
//! Text is shaped once per (text, style) and cached. Every glyph cluster is
//! pinned to its cell, and cells are a whole number of device pixels wide, so
//! glyphs stay crisp and on the grid at any scale. Box drawing, block
//! elements, braille and powerline symbols are drawn as geometry so they join
//! with no gaps whatever the font.

use std::cell::RefCell;
use std::collections::HashMap;

use gtk::cairo;
use gtk::pango;
use raptor_core::{CursorStyle, Frame, Rgb, Run, Underline};
use unicode_segmentation::UnicodeSegmentation;

pub const DEFAULT_FAMILY: &str = "JetBrains Mono";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub cell_w: f64,
    pub cell_h: f64,
    /// Baseline, from the top of the cell.
    pub ascent: f64,
    underline_y: f64,
    stroke: f64,
}

struct Piece {
    font: pango::Font,
    glyphs: pango::GlyphString,
    /// Width of the piece's natural shaping, in pixels.
    natural: f64,
    /// Bytes of text the piece covers.
    len: i32,
}

pub struct Renderer {
    pub family: String,
    pub font_size: f64,
    /// Device pixels per logical pixel; cells snap to this grid.
    pub scale: f64,
    fonts: [pango::FontDescription; 4],
    pub metrics: Metrics,
    cache: RefCell<HashMap<(String, usize), Vec<Piece>>>,
}

impl Renderer {
    pub fn new(ctx: &pango::Context, family: &str, font_size: f64, scale: f64) -> Self {
        let mut r = Self {
            family: family.to_owned(),
            font_size,
            scale,
            fonts: std::array::from_fn(|_| pango::FontDescription::new()),
            metrics: Metrics { cell_w: 8.0, cell_h: 16.0, ascent: 12.0, underline_y: 14.0, stroke: 1.0 },
            cache: RefCell::default(),
        };
        r.configure(ctx, family, font_size, scale);
        r
    }

    pub fn configure(&mut self, ctx: &pango::Context, family: &str, size: f64, scale: f64) {
        self.family = family.to_owned();
        self.font_size = size.clamp(6.0, 48.0);
        self.scale = scale.max(1.0);
        self.cache.borrow_mut().clear();
        for (i, f) in self.fonts.iter_mut().enumerate() {
            *f = pango::FontDescription::from_string(&format!("{family}, monospace"));
            f.set_absolute_size(self.font_size * pango::SCALE as f64);
            f.set_weight(if i & 1 == 1 { pango::Weight::Bold } else { pango::Weight::Normal });
            f.set_style(if i & 2 == 2 { pango::Style::Italic } else { pango::Style::Normal });
        }
        let m = ctx.metrics(Some(&self.fonts[0]), None);
        let px = |v: i32| v as f64 / pango::SCALE as f64;
        let snap = |v: f64| (v * self.scale).round() / self.scale;
        let advance = self.shape(ctx, "M", 0).first().map(|p| p.natural).unwrap_or(self.font_size * 0.6);
        let (asc, desc) = (px(m.ascent()), px(m.descent()));
        let height = px(m.height()).max(asc + desc);
        let cell_h = (height * self.scale).ceil() / self.scale;
        let ascent = snap((cell_h - (asc + desc)) / 2.0 + asc);
        let stroke = snap(px(m.underline_thickness()).max(1.0 / self.scale)).max(1.0 / self.scale);
        self.metrics = Metrics {
            cell_w: (advance * self.scale).round().max(1.0) / self.scale,
            cell_h,
            ascent,
            underline_y: (ascent - px(m.underline_position()) + stroke / 2.0).min(cell_h - stroke / 2.0),
            stroke,
        };
        self.cache.borrow_mut().clear();
    }

    pub fn grid(&self, w: f64, h: f64, pad: f64) -> (u16, u16) {
        (((w - pad * 2.0) / self.metrics.cell_w).max(10.0) as u16, ((h - pad * 2.0) / self.metrics.cell_h).max(4.0) as u16)
    }

    /// Shape `text` in one style. Each cluster's glyphs are respaced to fill
    /// exactly the cells it covers, so the pieces lay out on the grid.
    fn shape(&self, ctx: &pango::Context, text: &str, style: usize) -> Vec<Piece> {
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrFontDesc::new(&self.fonts[style]));
        let mut out = Vec::new();
        for item in pango::itemize(ctx, text, 0, text.len() as i32, &attrs, None) {
            let (off, len) = (item.offset() as usize, item.length() as usize);
            let mut glyphs = pango::GlyphString::new();
            pango::shape_full(&text[off..off + len], Some(text), item.analysis(), &mut glyphs);
            let natural = glyphs.width() as f64 / pango::SCALE as f64;
            out.push(Piece { font: item.analysis().font(), glyphs, natural, len: len as i32 });
        }
        out
    }

    /// Respace an ASCII run's glyphs onto the grid; a ligature keeps the cells of its characters.
    fn grid_ascii(&self, pieces: &mut [Piece]) {
        let cell = (self.metrics.cell_w * pango::SCALE as f64).round() as i32;
        for p in pieces {
            let clusters: Vec<i32> = p.glyphs.log_clusters().to_vec();
            let mut run_w = 0;
            for (i, g) in p.glyphs.glyph_info_mut().iter_mut().enumerate() {
                let geo = g.geometry_mut();
                let next = clusters.get(i + 1).copied();
                if next == Some(clusters[i]) {
                    run_w += geo.width();
                    continue;
                }
                let cells = next.unwrap_or(p.len) - clusters[i];
                let room = cell * cells - run_w;
                geo.set_x_offset(geo.x_offset() + (room - geo.width()) / 2);
                geo.set_width(room);
                run_w = 0;
            }
        }
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
        let sel = mix(f.background, f.foreground, 0.28);

        for (row, line) in f.lines.iter().enumerate() {
            let top = row as f64 * m.cell_h;
            for run in &line.runs {
                let bg = if run.selected { Some(sel) } else { run.bg };
                if let Some(bg) = bg {
                    set(cr, bg);
                    cr.rectangle(run.col as f64 * m.cell_w, top, run.cells as f64 * m.cell_w, m.cell_h);
                    let _ = cr.fill();
                }
            }
            for run in &line.runs {
                let fg = if run.faint { mix(run.fg, f.background, 0.5) } else { run.fg };
                self.draw_run(cr, pctx, run, fg, top);
            }
        }

        if let Some(c) = &f.cursor
            && cursor_on
            && f.scrollbar.at_bottom()
        {
            let x = c.col as f64 * m.cell_w;
            let y = c.row as f64 * m.cell_h;
            let cw = if c.wide { m.cell_w * 2.0 } else { m.cell_w };
            let color = f.cursor_color.unwrap_or(f.foreground);
            set(cr, color);
            let t = (1.0 / self.scale).max(1.0);
            if !focused || c.style == CursorStyle::HollowBlock {
                cr.set_line_width(t);
                cr.rectangle(x + t / 2.0, y + t / 2.0, cw - t, m.cell_h - t);
                let _ = cr.stroke();
            } else {
                let bar = (m.cell_w / 8.0).max(1.0).round();
                match c.style {
                    CursorStyle::Bar => cr.rectangle(x, y, bar, m.cell_h),
                    CursorStyle::Underline => cr.rectangle(x, y + m.cell_h - bar, cw, bar),
                    _ => cr.rectangle(x, y, cw, m.cell_h),
                }
                let _ = cr.fill();
                // Redraw the text under a block cursor in the background color.
                if c.style == CursorStyle::Block
                    && let Some(line) = f.lines.get(c.row as usize)
                {
                    cr.save().ok();
                    cr.rectangle(x, y, cw, m.cell_h);
                    cr.clip();
                    for run in line.runs.iter().filter(|r| c.col >= r.col && c.col < r.col + r.cells) {
                        self.draw_run(cr, pctx, run, f.background, y);
                    }
                    cr.restore().ok();
                }
            }
        }
        cr.restore().ok();
    }

    fn draw_run(&self, cr: &cairo::Context, pctx: &pango::Context, run: &Run, fg: Rgb, top: f64) {
        let m = self.metrics;
        set(cr, fg);
        let style = (run.bold as usize) | ((run.italic as usize) << 1);
        let baseline = top + m.ascent;
        if run.widths.iter().all(|w| *w == 1) && run.text.is_ascii() {
            if !run.text.bytes().all(|b| b == b' ') {
                let mut cache = self.cache.borrow_mut();
                if cache.len() > 4096 {
                    cache.clear();
                }
                let pieces = cache.entry((run.text.clone(), style)).or_insert_with(|| {
                    let mut p = self.shape(pctx, &run.text, style);
                    self.grid_ascii(&mut p);
                    p
                });
                cr.move_to(run.col as f64 * m.cell_w, baseline);
                for p in pieces.iter_mut() {
                    pangocairo::functions::show_glyph_string(cr, &p.font, &mut p.glyphs);
                    let adv = p.glyphs.width() as f64 / pango::SCALE as f64;
                    cr.rel_move_to(adv, 0.0);
                }
            }
        } else {
            let mut at = 0;
            let graphemes: Vec<&str> = if run.lens.len() == run.widths.len() && run.lens.iter().map(|l| *l as usize).sum::<usize>() == run.text.len() {
                run.lens
                    .iter()
                    .map(|l| {
                        let g = &run.text[at..at + *l as usize];
                        at += *l as usize;
                        g
                    })
                    .collect()
            } else {
                run.text.graphemes(true).collect()
            };
            let widths: Vec<u8> = if graphemes.len() == run.widths.len() { run.widths.clone() } else { vec![1; graphemes.len()] };
            let mut col = run.col as f64;
            for (g, w) in graphemes.iter().zip(widths) {
                let cells = w as f64;
                let x = col * m.cell_w;
                col += cells;
                let cp = g.chars().next().map(|c| c as u32).unwrap_or(32);
                if cp == 32 || (g.chars().count() == 1 && self.special(cr, cp, x, top, cells, fg)) {
                    continue;
                }
                self.draw_cluster(cr, pctx, g, style, x, baseline, cells);
            }
        }
        self.decorate(cr, run, run.underline_color.unwrap_or(fg), top);
    }

    /// One grapheme, centered in its cells and shrunk if it would spill over.
    #[allow(clippy::too_many_arguments)]
    fn draw_cluster(&self, cr: &cairo::Context, pctx: &pango::Context, g: &str, style: usize, x: f64, baseline: f64, cells: f64) {
        let m = self.metrics;
        let mut cache = self.cache.borrow_mut();
        let pieces = cache.entry((g.to_owned(), style)).or_insert_with(|| self.shape(pctx, g, style));
        let natural: f64 = pieces.iter().map(|p| p.natural).sum();
        let room = cells * m.cell_w;
        cr.save().ok();
        if natural > room * 1.05 {
            let k = room / natural;
            cr.translate(x, baseline - m.ascent + m.cell_h / 2.0);
            cr.scale(k, k);
            cr.move_to(0.0, m.ascent - m.cell_h / 2.0);
        } else {
            cr.move_to(x + ((room - natural) / 2.0 * self.scale).round() / self.scale, baseline);
        }
        for p in pieces.iter_mut() {
            pangocairo::functions::show_glyph_string(cr, &p.font, &mut p.glyphs);
            cr.rel_move_to(p.natural, 0.0);
        }
        cr.restore().ok();
    }

    /// Characters drawn as geometry. Returns false to fall back to the font.
    fn special(&self, cr: &cairo::Context, cp: u32, x: f64, top: f64, cells: f64, fg: Rgb) -> bool {
        let m = self.metrics;
        let (w, h) = (m.cell_w * cells, m.cell_h);
        let drawn = match cp {
            0x2500..=0x257F => self.box_line(cr, cp, x, top, w, h),
            0x2580..=0x259F => block(cr, cp, x, top, w, h, fg),
            0x2800..=0x28FF => {
                braille(cr, cp - 0x2800, x, top, w, h);
                true
            }
            0xE0B0..=0xE0BF => powerline(cr, cp, x, top, w, h, self.light()),
            _ => false,
        };
        if drawn {
            set(cr, fg);
        }
        drawn
    }

    fn light(&self) -> f64 {
        ((self.font_size / 12.0 * self.scale).round().max(1.0)) / self.scale
    }

    fn box_line(&self, cr: &cairo::Context, cp: u32, x: f64, top: f64, w: f64, h: f64) -> bool {
        let snap = |v: f64| (v * self.scale).round() / self.scale;
        let lt = self.light();
        let hv = snap(lt * 2.0).max(lt + 1.0 / self.scale);
        let (cx, cy) = (snap(x + w / 2.0 - lt / 2.0) + lt / 2.0, snap(top + h / 2.0 - lt / 2.0) + lt / 2.0);
        let (r, b) = (x + w, top + h);
        match cp {
            0x256D..=0x2570 => {
                let rad = (w.min(h) / 2.0).max(lt);
                cr.set_line_width(lt);
                let (sx, sy, ex, ey) = match cp {
                    0x256D => (cx, b, r, cy),
                    0x256E => (cx, b, x, cy),
                    0x256F => (cx, top, x, cy),
                    _ => (cx, top, r, cy),
                };
                let dy = if sy > cy { rad } else { -rad };
                let dx = if ex > cx { rad } else { -rad };
                cr.move_to(sx, sy);
                cr.line_to(cx, cy + dy);
                cr.curve_to(cx, cy + dy * 0.45, cx + dx * 0.45, cy, cx + dx, cy);
                cr.line_to(ex, ey);
                let _ = cr.stroke();
                return true;
            }
            0x2571..=0x2573 => {
                cr.set_line_width(lt);
                if cp != 0x2572 {
                    cr.move_to(r, top);
                    cr.line_to(x, b);
                }
                if cp != 0x2571 {
                    cr.move_to(x, top);
                    cr.line_to(r, b);
                }
                let _ = cr.stroke();
                return true;
            }
            _ => {}
        }
        let v = BOX[(cp - 0x2500) as usize];
        let (up, right, down, left) = (v >> 6, (v >> 4) & 3, (v >> 2) & 3, v & 3);
        let thick = |k: u8| if k == 2 { hv } else { lt };
        let g = snap(lt * 1.5).max(lt + 1.0 / self.scale);
        let dash = match cp {
            0x2504..=0x2507 => 3,
            0x2508..=0x250B => 4,
            0x254C..=0x254F => 2,
            _ => 0,
        };
        let hseg = |a: f64, z: f64, y: f64, t: f64| {
            if dash > 0 {
                let step = (z - a) / dash as f64;
                for i in 0..dash {
                    cr.rectangle(a + step * i as f64 + step * 0.15, y - t / 2.0, step * 0.7, t);
                }
            } else {
                cr.rectangle(a, y - t / 2.0, z - a, t);
            }
        };
        let vseg = |a: f64, z: f64, xx: f64, t: f64| {
            if dash > 0 {
                let step = (z - a) / dash as f64;
                for i in 0..dash {
                    cr.rectangle(xx - t / 2.0, a + step * i as f64 + step * 0.15, t, step * 0.7);
                }
            } else {
                cr.rectangle(xx - t / 2.0, a, t, z - a);
            }
        };
        // How far a horizontal arm reaches into the center to meet the vertical arms.
        let vmax = thick(up.max(down)) / 2.0;
        let hmax = thick(left.max(right)) / 2.0;
        let (ud, dd, ld, rd) = (up == 3, down == 3, left == 3, right == 3);
        for (k, sign) in [(left, -1.0), (right, 1.0)] {
            if k == 0 {
                continue;
            }
            let edge = if sign < 0.0 { x } else { r };
            if k == 3 {
                // Two lines; each stops at the vertical line on its own side, if doubled.
                let stop = |own: bool, other: bool| if own { g } else if other { -g } else { 0.0 };
                for (y, own, other) in [(cy - g, ud, dd), (cy + g, dd, ud)] {
                    let inner = cx + sign * stop(own, other);
                    let (a, z) = if sign < 0.0 { (edge, inner + lt / 2.0) } else { (inner - lt / 2.0, edge) };
                    hseg(a, z, y, lt);
                }
            } else {
                let reach = if ud || dd { g + lt / 2.0 } else { vmax };
                let (a, z) = if sign < 0.0 { (edge, cx + reach) } else { (cx - reach, edge) };
                hseg(a, z, cy, thick(k));
            }
        }
        for (k, sign) in [(up, -1.0), (down, 1.0)] {
            if k == 0 {
                continue;
            }
            let edge = if sign < 0.0 { top } else { b };
            if k == 3 {
                let stop = |own: bool, other: bool| if own { g } else if other { -g } else { 0.0 };
                for (xx, own, other) in [(cx - g, ld, rd), (cx + g, rd, ld)] {
                    let inner = cy + sign * stop(own, other);
                    let (a, z) = if sign < 0.0 { (edge, inner + lt / 2.0) } else { (inner - lt / 2.0, edge) };
                    vseg(a, z, xx, lt);
                }
            } else {
                let reach = if ld || rd { g + lt / 2.0 } else { hmax };
                let (a, z) = if sign < 0.0 { (edge, cy + reach) } else { (cy - reach, edge) };
                vseg(a, z, cx, thick(k));
            }
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
        let t = m.stroke;
        cr.set_line_width(t);
        let uy = top + m.underline_y;
        let line = |y: f64| {
            cr.move_to(x0, y);
            cr.line_to(x1, y);
            let _ = cr.stroke();
        };
        match run.underline {
            Underline::None => {}
            Underline::Double => {
                line(uy - t);
                line(uy + t)
            }
            Underline::Curly => {
                let amp = (m.cell_h - m.underline_y).clamp(1.0, 2.0);
                let period = m.cell_w / 1.5;
                cr.move_to(x0, uy);
                let mut x = x0;
                while x < x1 {
                    let nx = (x + period).min(x1);
                    cr.curve_to(x + period / 3.0, uy - amp, nx - period / 3.0, uy + amp, nx, uy);
                    x = nx;
                }
                let _ = cr.stroke();
            }
            Underline::Dotted | Underline::Dashed => {
                let (dotted, dashed) = ([t, t * 2.0], [t * 4.0, t * 2.0]);
                cr.set_dash(if run.underline == Underline::Dotted { &dotted } else { &dashed }, 0.0);
                line(uy);
                cr.set_dash(&[], 0.0);
            }
            Underline::Single => line(uy),
        }
        if run.strikethrough {
            line(top + (m.ascent * 0.65).round())
        }
        if run.overline {
            line(top + t / 2.0)
        }
    }
}

/// Arms of each box-drawing character U+2500..U+257F, two bits each:
/// up, right, down, left. 1 light, 2 heavy, 3 double. Generated from the
/// characters' Unicode names.
#[rustfmt::skip]
const BOX: [u8; 128] = [
    0x11, 0x22, 0x44, 0x88, 0x11, 0x22, 0x44, 0x88, 0x11, 0x22, 0x44, 0x88, 0x14, 0x24, 0x18, 0x28,
    0x05, 0x06, 0x09, 0x0A, 0x50, 0x60, 0x90, 0xA0, 0x41, 0x42, 0x81, 0x82, 0x54, 0x64, 0x94, 0x58,
    0x98, 0xA4, 0x68, 0xA8, 0x45, 0x46, 0x85, 0x49, 0x89, 0x86, 0x4A, 0x8A, 0x15, 0x16, 0x25, 0x26,
    0x19, 0x1A, 0x29, 0x2A, 0x51, 0x52, 0x61, 0x62, 0x91, 0x92, 0xA1, 0xA2, 0x55, 0x56, 0x65, 0x66,
    0x95, 0x59, 0x99, 0x96, 0xA5, 0x5A, 0x69, 0xA6, 0x6A, 0x9A, 0xA9, 0xAA, 0x11, 0x22, 0x44, 0x88,
    0x33, 0xCC, 0x34, 0x1C, 0x3C, 0x07, 0x0D, 0x0F, 0x70, 0xD0, 0xF0, 0x43, 0xC1, 0xC3, 0x74, 0xDC,
    0xFC, 0x47, 0xCD, 0xCF, 0x37, 0x1D, 0x3F, 0x73, 0xD1, 0xF3, 0x77, 0xDD, 0xFF, 0x14, 0x05, 0x41,
    0x50, 0x00, 0x00, 0x00, 0x01, 0x40, 0x10, 0x04, 0x02, 0x80, 0x20, 0x08, 0x21, 0x48, 0x12, 0x84,
];

/// Block elements U+2580..U+259F.
fn block(cr: &cairo::Context, cp: u32, x: f64, top: f64, w: f64, h: f64, fg: Rgb) -> bool {
    let (cx, cy, b) = (x + w / 2.0, top + h / 2.0, top + h);
    let quad = |q: u8| {
        for (bit, qx, qy) in [(1, x, top), (2, cx, top), (4, x, cy), (8, cx, cy)] {
            if q & bit != 0 {
                cr.rectangle(qx, qy, w / 2.0, h / 2.0);
            }
        }
    };
    match cp {
        0x2580 => cr.rectangle(x, top, w, h / 2.0),
        0x2581..=0x2588 => {
            let f = (cp - 0x2580) as f64 / 8.0;
            cr.rectangle(x, b - h * f, w, h * f)
        }
        0x2589..=0x258F => cr.rectangle(x, top, w * (0x2590 - cp) as f64 / 8.0, h),
        0x2590 => cr.rectangle(cx, top, w / 2.0, h),
        0x2591..=0x2593 => {
            let a = (cp - 0x2590) as f64 / 4.0;
            cr.set_source_rgba(fg.r as f64 / 255.0, fg.g as f64 / 255.0, fg.b as f64 / 255.0, a);
            cr.rectangle(x, top, w, h)
        }
        0x2594 => cr.rectangle(x, top, w, h / 8.0),
        0x2595 => cr.rectangle(x + w * 7.0 / 8.0, top, w / 8.0, h),
        0x2596 => quad(4),
        0x2597 => quad(8),
        0x2598 => quad(1),
        0x2599 => quad(1 | 4 | 8),
        0x259A => quad(1 | 8),
        0x259B => quad(1 | 2 | 4),
        0x259C => quad(1 | 2 | 8),
        0x259D => quad(2),
        0x259E => quad(2 | 4),
        0x259F => quad(2 | 4 | 8),
        _ => return false,
    }
    let _ = cr.fill();
    true
}

fn braille(cr: &cairo::Context, bits: u32, x: f64, top: f64, w: f64, h: f64) {
    let (dx, dy) = (w / 2.0, h / 4.0);
    let r = (dx.min(dy) * 0.3).max(0.7);
    for (bit, col, row) in [(0, 0, 0), (1, 0, 1), (2, 0, 2), (3, 1, 0), (4, 1, 1), (5, 1, 2), (6, 0, 3), (7, 1, 3)] {
        if bits & (1 << bit) != 0 {
            cr.arc(x + dx * (col as f64 + 0.5), top + dy * (row as f64 + 0.5), r, 0.0, std::f64::consts::TAU);
            cr.close_path();
        }
    }
    let _ = cr.fill();
}

/// Powerline separators U+E0B0..U+E0BF.
fn powerline(cr: &cairo::Context, cp: u32, x: f64, top: f64, w: f64, h: f64, lt: f64) -> bool {
    let (r, b, cy) = (x + w, top + h, top + h / 2.0);
    let filled = cp.is_multiple_of(2);
    match cp {
        0xE0B0 | 0xE0B1 => {
            cr.move_to(x, top);
            cr.line_to(r, cy);
            cr.line_to(x, b);
        }
        0xE0B2 | 0xE0B3 => {
            cr.move_to(r, top);
            cr.line_to(x, cy);
            cr.line_to(r, b);
        }
        0xE0B4 | 0xE0B5 => {
            cr.move_to(x, top);
            cr.curve_to(x + w * 1.33, top, x + w * 1.33, b, x, b);
        }
        0xE0B6 | 0xE0B7 => {
            cr.move_to(r, top);
            cr.curve_to(r - w * 1.33, top, r - w * 1.33, b, r, b);
        }
        0xE0B8 | 0xE0B9 => {
            cr.move_to(x, top);
            cr.line_to(r, b);
            if filled {
                cr.line_to(x, b);
            }
        }
        0xE0BA | 0xE0BB => {
            cr.move_to(r, top);
            cr.line_to(x, b);
            if filled {
                cr.line_to(r, b);
            }
        }
        0xE0BC | 0xE0BD => {
            cr.move_to(x, b);
            cr.line_to(r, top);
            if filled {
                cr.line_to(x, top);
            }
        }
        0xE0BE | 0xE0BF => {
            cr.move_to(x, top);
            cr.line_to(r, b);
            if filled {
                cr.line_to(r, top);
            }
        }
        _ => return false,
    }
    if filled {
        cr.close_path();
        let _ = cr.fill();
    } else {
        cr.set_line_width(lt);
        let _ = cr.stroke();
    }
    true
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

    /// Renders a real libghostty-vt frame offscreen; set RAPTOR_RENDER_PNG to keep the image.
    #[test]
    fn renders_frame_offscreen() {
        crate::fonts::register();
        let mut t = raptor_term::Terminal::new(60, 12);
        t.set_default_colors(Some(Rgb::new(0xe6, 0xed, 0xf3)), Some(Rgb::new(0x0d, 0x11, 0x17)), Some(Rgb::new(0x34, 0xd3, 0x99)));
        t.write_vt("\x1b[1mRaptor on Linux\x1b[0m: Cairo + Pango -> != ===\r\n\r\n\x1b[1;35mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[4:3mcurly\x1b[0m \x1b[9mstrike\x1b[0m \x1b[7m inverse \x1b[0m\r\nwide: 中文字 ok 👍🏽 ❤️ 🇺🇸\r\n┌──────┬──────┐ ╔══╦══╗ ╭──╮\r\n│ box  │ draw │ ║  ║  ║ ╰──╯\r\n└──────┴──────┘ ╚══╩══╝ ░▒▓█ ⣿⡇\r\n\x1b[44;97m main \x1b[34;42m\u{e0b0}\x1b[30m +3 \x1b[32;49m\u{e0b0}\x1b[0m $ ".as_bytes());
        let frame = t.frame();
        let pctx = pangocairo::FontMap::default().create_context();
        let r = Renderer::new(&pctx, DEFAULT_FAMILY, 14.0, 2.0);
        let (w, h) = ((60.0 * r.metrics.cell_w + 12.0) as i32, (12.0 * r.metrics.cell_h + 12.0) as i32);
        let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w * 2, h * 2).unwrap();
        let cr = cairo::Context::new(&surf).unwrap();
        cr.scale(2.0, 2.0);
        pangocairo::functions::update_context(&cr, &pctx);
        r.draw(&cr, &pctx, &frame, w as f64, h as f64, 6.0, 1.0, true, true);
        drop(cr);
        assert!(r.metrics.cell_w > 4.0 && r.metrics.cell_h > 8.0);
        assert_eq!((r.metrics.cell_w * 2.0).fract(), 0.0, "cells are whole device pixels");
        if let Ok(p) = std::env::var("RAPTOR_RENDER_PNG") {
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
