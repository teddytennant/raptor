//! A GTK terminal widget backed by a `raptor_core::TerminalSession`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, glib, pango};
use raptor_core::{Key, Raptor, Scrollbar, TerminalListener, TerminalSession, TerminalStatus};

use crate::render::{Renderer, mix, set};

const PAD: f64 = 6.0;
/// The scrollbar shows this long after the last scroll.
const SCROLLBAR_LINGER: Duration = Duration::from_millis(1200);

enum Msg {
    Frame,
    Status(TerminalStatus),
    Clipboard(String),
}

struct Bridge(async_channel::Sender<Msg>);
impl TerminalListener for Bridge {
    fn frame_ready(&self) {
        let _ = self.0.try_send(Msg::Frame);
    }
    fn status_changed(&self, s: TerminalStatus) {
        let _ = self.0.try_send(Msg::Status(s));
    }
    fn clipboard(&self, t: String) {
        let _ = self.0.try_send(Msg::Clipboard(t));
    }
    fn size_changed(&self, _: u16, _: u16) {
        let _ = self.0.try_send(Msg::Frame);
    }
}

/// What the user can change about every terminal.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    pub family: String,
    pub size: f64,
    pub blink: bool,
    pub copy_on_select: bool,
}

#[derive(Default)]
struct Mouse {
    /// Buttons held that the program is being told about.
    held: Vec<&'static str>,
    last_cell: Option<(u16, u16)>,
    /// A selection drag (as opposed to one reported to the program).
    selecting: bool,
    /// Dragging the scrollbar thumb.
    scrollbar: bool,
}

struct State {
    session: Option<Arc<TerminalSession>>,
    renderer: Option<Renderer>,
    prefs: Prefs,
    status: TerminalStatus,
    scroll_rem: f64,
    cursor_on: bool,
    blink_until: Instant,
    blink_timer: Option<glib::SourceId>,
    sel_anchor: Option<(u16, u16)>,
    mouse: Mouse,
    /// From the last frame drawn.
    scrollbar: Scrollbar,
    alt_screen: bool,
    scrolled_at: Option<Instant>,
    preedit: String,
    on_focus: Option<Rc<dyn Fn()>>,
}

/// The terminal surface plus a status pill overlay.
#[derive(Clone)]
pub struct TermView {
    pub root: gtk::Overlay,
    area: gtk::DrawingArea,
    pill: gtk::Box,
    pill_label: gtk::Label,
    pill_spinner: gtk::Spinner,
    im: gtk::IMMulticontext,
    state: Rc<RefCell<State>>,
}

impl TermView {
    pub fn new(core: &Raptor, server: &str, session: &str, block: &str, take_size: bool, prefs: Prefs) -> Self {
        let area = gtk::DrawingArea::builder().hexpand(true).vexpand(true).focusable(true).can_focus(true).build();
        area.add_css_class("raptor-terminal");
        area.set_cursor_from_name(Some("text"));
        let pill_label = gtk::Label::new(None);
        let pill_spinner = gtk::Spinner::new();
        let pill = gtk::Box::builder().spacing(8).halign(gtk::Align::Center).valign(gtk::Align::Start).margin_top(12).visible(false).build();
        pill.add_css_class("raptor-pill");
        pill.set_can_target(false);
        pill.append(&pill_spinner);
        pill.append(&pill_label);
        let root = gtk::Overlay::builder().child(&area).build();
        root.add_css_class("raptor-pane");
        // Dims a pane without focus and draws the focus ring; never takes input.
        let ring = gtk::Box::builder().hexpand(true).vexpand(true).can_target(false).build();
        ring.add_css_class("raptor-ring");
        root.add_overlay(&ring);
        root.add_overlay(&pill);

        let (tx, rx) = async_channel::bounded(64);
        let sess = core.open_terminal(server, session, block, Arc::new(Bridge(tx))).ok();
        if take_size && let Some(s) = &sess {
            s.take_control();
        }
        let state = Rc::new(RefCell::new(State {
            session: sess,
            renderer: None,
            prefs,
            status: TerminalStatus::Connecting,
            scroll_rem: 0.0,
            cursor_on: true,
            blink_until: Instant::now(),
            blink_timer: None,
            sel_anchor: None,
            mouse: Mouse::default(),
            scrollbar: Scrollbar::default(),
            alt_screen: false,
            scrolled_at: None,
            preedit: String::new(),
            on_focus: None,
        }));
        let im = gtk::IMMulticontext::new();
        let this = Self { root, area, pill, pill_label, pill_spinner, im, state };
        this.wire(rx);
        this.update_pill();
        this
    }

    pub fn grab_focus(&self) {
        self.area.grab_focus();
    }

    pub fn has_focus(&self) -> bool {
        self.area.has_focus()
    }

    /// Called when this pane takes keyboard focus (click, Ctrl+Tab).
    pub fn connect_focus(&self, f: impl Fn() + 'static) {
        self.state.borrow_mut().on_focus = Some(Rc::new(f));
    }

    /// Force a terminal theme, or `None` for the server's colors.
    pub fn set_theme(&self, theme: Option<raptor_core::ColorTheme>) {
        if let Some(s) = self.session() {
            s.set_theme(theme);
        }
        self.area.queue_draw();
    }

    pub fn session(&self) -> Option<Arc<TerminalSession>> {
        self.state.borrow().session.clone()
    }

    pub fn shutdown(&self) {
        let mut st = self.state.borrow_mut();
        if let Some(s) = st.session.take() {
            s.close();
        }
        if let Some(t) = st.blink_timer.take() {
            t.remove();
        }
    }

    pub fn set_prefs(&self, prefs: &Prefs) {
        let mut st = self.state.borrow_mut();
        if st.prefs == *prefs {
            return;
        }
        st.prefs = prefs.clone();
        if let Some(r) = st.renderer.as_mut() {
            r.configure(&self.area.pango_context(), &prefs.family, prefs.size, device_scale(&self.area));
        }
        drop(st);
        self.push_grid();
        self.area.queue_draw();
        self.restart_blink();
    }

    pub fn copy(&self) {
        if let Some(t) = self.session().and_then(|s| s.selection_text()) {
            self.area.clipboard().set_text(&t);
        }
    }

    pub fn paste(&self) {
        let me = self.clone();
        self.area.clipboard().read_text_async(None::<&gtk::gio::Cancellable>, move |r| {
            if let (Ok(Some(t)), Some(s)) = (r, me.session()) {
                s.paste(&t);
            }
        });
    }

    fn update_pill(&self) {
        let st = self.state.borrow();
        let (text, busy) = match &st.status {
            TerminalStatus::Live => (None, false),
            TerminalStatus::Connecting => (Some("Connecting…".to_owned()), true),
            TerminalStatus::Reconnecting => (Some("Reconnecting…".to_owned()), true),
            TerminalStatus::Exited { code } => {
                (Some(if *code == 0 { "Process exited".into() } else { format!("Process exited with code {code}") }), false)
            }
            TerminalStatus::Ended { reason } => (Some(reason.clone()), false),
        };
        self.pill.set_visible(text.is_some());
        self.pill_label.set_label(text.as_deref().unwrap_or(""));
        self.pill_spinner.set_visible(busy);
        self.pill_spinner.set_spinning(busy);
    }

    /// Our grid size in cells, or None before the first layout.
    fn push_grid(&self) {
        let st = self.state.borrow();
        let (Some(s), Some(r)) = (&st.session, &st.renderer) else { return };
        let (w, h) = (self.area.width() as f64, self.area.height() as f64);
        if w < 10.0 {
            return;
        }
        let (c, rows) = r.grid(w, h, PAD);
        s.set_device_grid(c, rows, (r.metrics.cell_w * r.scale) as u32, (r.metrics.cell_h * r.scale) as u32);
    }

    /// How much a mirrored terminal (sized by another client) is shrunk to fit.
    fn scale(&self) -> f64 {
        let st = self.state.borrow();
        let (Some(s), Some(r)) = (&st.session, &st.renderer) else { return 1.0 };
        if s.has_control() {
            return 1.0;
        }
        let (cols, rows) = s.size();
        let sx = (self.area.width() as f64 - PAD * 2.0) / (cols as f64 * r.metrics.cell_w);
        let sy = (self.area.height() as f64 - PAD * 2.0) / (rows as f64 * r.metrics.cell_h);
        sx.min(sy).min(1.0)
    }

    fn cell_at(&self, x: f64, y: f64) -> (u16, u16) {
        let k = self.scale();
        let st = self.state.borrow();
        let (Some(r), Some(s)) = (&st.renderer, &st.session) else { return (0, 0) };
        let (cols, rows) = s.size();
        let c = ((x - PAD) / k / r.metrics.cell_w).max(0.0) as u16;
        let row = ((y - PAD) / k / r.metrics.cell_h).max(0.0) as u16;
        (c.min(cols.saturating_sub(1)), row.min(rows.saturating_sub(1)))
    }

    fn renderer_ready(&self) {
        let mut st = self.state.borrow_mut();
        if st.renderer.is_none() {
            let p = st.prefs.clone();
            st.renderer = Some(Renderer::new(&self.area.pango_context(), &p.family, p.size, device_scale(&self.area)));
        }
    }

    /// Start (or keep) the cursor blinking, for as long as GTK's blink timeout.
    fn restart_blink(&self) {
        let settings = gtk::Settings::default();
        let enabled = settings.as_ref().is_none_or(|s| s.is_gtk_cursor_blink()) && self.state.borrow().prefs.blink;
        let mut st = self.state.borrow_mut();
        st.cursor_on = true;
        if !enabled || !self.area.has_focus() {
            if let Some(t) = st.blink_timer.take() {
                t.remove();
            }
            drop(st);
            self.area.queue_draw();
            return;
        }
        let timeout = settings.as_ref().map(|s| s.gtk_cursor_blink_timeout()).unwrap_or(10).max(1) as u64;
        st.blink_until = Instant::now() + Duration::from_secs(timeout);
        if st.blink_timer.is_some() {
            return;
        }
        let period = settings.as_ref().map(|s| s.gtk_cursor_blink_time()).unwrap_or(1200).max(200) as u64 / 2;
        let me = self.clone();
        st.blink_timer = Some(glib::timeout_add_local(Duration::from_millis(period), move || {
            let mut st = me.state.borrow_mut();
            let done = Instant::now() > st.blink_until || !me.area.has_focus();
            st.cursor_on = done || !st.cursor_on;
            if done {
                st.blink_timer = None;
            }
            drop(st);
            me.area.queue_draw();
            if done { glib::ControlFlow::Break } else { glib::ControlFlow::Continue }
        }));
    }

    fn wire(&self, rx: async_channel::Receiver<Msg>) {
        let me = self.clone();
        self.area.set_draw_func(move |area, cr, w, h| me.draw(area, cr, w as f64, h as f64));

        let me = self.clone();
        self.area.connect_resize(move |_, _, _| {
            me.renderer_ready();
            me.push_grid();
        });
        // Moving to a screen with another scale changes the pixel grid.
        let me = self.clone();
        self.area.connect_scale_factor_notify(move |a| {
            let mut st = me.state.borrow_mut();
            let p = st.prefs.clone();
            if let Some(r) = st.renderer.as_mut() {
                r.configure(&a.pango_context(), &p.family, p.size, device_scale(a));
            }
            drop(st);
            me.push_grid();
        });

        let me = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(m) = rx.recv().await {
                match m {
                    Msg::Frame => me.area.queue_draw(),
                    Msg::Status(s) => {
                        me.state.borrow_mut().status = s;
                        me.update_pill();
                    }
                    Msg::Clipboard(t) => me.area.clipboard().set_text(&t),
                }
            }
        });

        self.wire_keys();
        self.wire_mouse();
    }

    fn draw(&self, area: &gtk::DrawingArea, cr: &gtk::cairo::Context, w: f64, h: f64) {
        self.renderer_ready();
        let k = self.scale();
        let st = self.state.borrow();
        let Some(sess) = st.session.clone() else { return };
        let frame = sess.frame();
        let focused = area.has_focus();
        let r = st.renderer.as_ref().unwrap();
        let ctx = area.pango_context();
        r.draw(cr, &ctx, &frame, w, h, PAD, k, focused, st.cursor_on);
        let show_bar = frame.scrollbar.total > frame.scrollbar.len
            && (!frame.scrollbar.at_bottom() || st.scrolled_at.is_some_and(|t| t.elapsed() < SCROLLBAR_LINGER) || st.mouse.scrollbar);
        if show_bar {
            let (y, thumb) = thumb(&frame.scrollbar, h);
            set(cr, mix(frame.foreground, frame.background, 0.55));
            rounded(cr, w - 7.0, y, 4.0, thumb);
            let _ = cr.fill();
        }
        // Where the input method's candidate window should appear, and what it's composing.
        if let Some(c) = &frame.cursor {
            let (x, y) = (PAD + c.col as f64 * r.metrics.cell_w * k, PAD + c.row as f64 * r.metrics.cell_h * k);
            self.im.set_cursor_location(&gdk::Rectangle::new(x as i32, y as i32, r.metrics.cell_w as i32, (r.metrics.cell_h * k) as i32));
            if !st.preedit.is_empty() {
                let layout = pango::Layout::new(&ctx);
                let desc = pango::FontDescription::from_string(&format!("{}, monospace {}px", r.family, r.font_size.round()));
                layout.set_font_description(Some(&desc));
                layout.set_text(&st.preedit);
                let attrs = pango::AttrList::new();
                attrs.insert(pango::AttrInt::new_underline(pango::Underline::Single));
                layout.set_attributes(Some(&attrs));
                let (lw, _) = layout.pixel_size();
                set(cr, frame.background);
                cr.rectangle(x, y, lw as f64, r.metrics.cell_h);
                let _ = cr.fill();
                set(cr, frame.foreground);
                cr.move_to(x, y + (r.metrics.cell_h - layout.pixel_size().1 as f64) / 2.0);
                pangocairo::functions::show_layout(cr, &layout);
            }
        }
        drop(st);
        let mut st = self.state.borrow_mut();
        st.scrollbar = frame.scrollbar;
        st.alt_screen = frame.alternate_screen;
        if show_bar && frame.scrollbar.at_bottom() && !st.mouse.scrollbar {
            // Redraw once more when the scrollbar should fade.
            let a = area.clone();
            glib::timeout_add_local_once(SCROLLBAR_LINGER, move || a.queue_draw());
        }
    }

    fn wire_keys(&self) {
        let im = self.im.clone();
        im.set_client_widget(Some(&self.area));
        let me = self.clone();
        im.connect_commit(move |_, text| {
            me.state.borrow_mut().preedit.clear();
            me.typed();
            if let Some(s) = me.session() {
                s.send_text(text);
            }
        });
        let me = self.clone();
        im.connect_preedit_changed(move |im| {
            let (text, _, _) = im.preedit_string();
            me.state.borrow_mut().preedit = text.to_string();
            me.area.queue_draw();
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_im_context(Some(&im));
        let me = self.clone();
        keys.connect_key_pressed(move |ctl, keyval, code, mods| {
            let Some(s) = me.session() else { return glib::Propagation::Proceed };
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let alt = mods.contains(gdk::ModifierType::ALT_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            let sup = mods.contains(gdk::ModifierType::SUPER_MASK);
            if sup || keyval.to_unicode().is_none() && named(keyval).is_none() {
                return glib::Propagation::Proceed;
            }
            me.typed();
            // Scrollback, without leaving the keyboard.
            if shift && !ctrl && !alt {
                let page = s.size().1 as i32 - 1;
                match keyval {
                    gdk::Key::Page_Up => return me.scroll_by(-page),
                    gdk::Key::Page_Down => return me.scroll_by(page),
                    gdk::Key::Home if !me.state.borrow().alt_screen => return me.scroll_by(i32::MIN / 2),
                    gdk::Key::End if !me.state.borrow().alt_screen => return me.scroll_by(i32::MAX / 2),
                    _ => {}
                }
            }
            if ctrl && shift && !alt {
                match keyval {
                    gdk::Key::Up => {
                        s.jump_prompt(-1);
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::Down => {
                        s.jump_prompt(1);
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            if let Some(k) = named(keyval) {
                s.send_key(k, ctrl, alt, shift, false);
                return glib::Propagation::Stop;
            }
            if ctrl || alt {
                // With a non-Latin layout, Ctrl+C should still be ^C: use the key's Latin meaning.
                let ch = keyval.to_lower().to_unicode().filter(|c| c.is_ascii() && !c.is_control()).or_else(|| latin(ctl, code));
                if let Some(ch) = ch {
                    s.send_key(Key::Char(ch), ctrl, alt, shift, false);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        self.area.add_controller(keys);

        let focus = gtk::EventControllerFocus::new();
        let me = self.clone();
        focus.connect_enter(move |_| {
            me.im.focus_in();
            me.root.add_css_class("focused");
            me.restart_blink();
            if let Some(s) = me.session() {
                s.focus(true);
            }
            let cb = me.state.borrow().on_focus.clone();
            if let Some(cb) = cb {
                cb();
            }
        });
        let me = self.clone();
        focus.connect_leave(move |_| {
            me.im.focus_out();
            me.root.remove_css_class("focused");
            me.restart_blink();
            if let Some(s) = me.session() {
                s.focus(false);
            }
        });
        self.area.add_controller(focus);
    }

    /// Typing hides the mouse pointer and keeps the cursor solid.
    fn typed(&self) {
        self.area.set_cursor_from_name(Some("none"));
        self.restart_blink();
    }

    fn scroll_by(&self, rows: i32) -> glib::Propagation {
        if let Some(s) = self.session() {
            s.scroll(rows);
            self.state.borrow_mut().scrolled_at = Some(Instant::now());
        }
        glib::Propagation::Stop
    }

    /// Whether mouse events go to the program (it asked for them and Shift isn't held).
    fn reporting(&self, mods: gdk::ModifierType) -> bool {
        !mods.contains(gdk::ModifierType::SHIFT_MASK) && self.session().is_some_and(|s| s.mouse_tracking())
    }

    fn in_scrollbar(&self, x: f64) -> bool {
        let st = self.state.borrow();
        x > self.area.width() as f64 - 14.0 && st.scrollbar.total > st.scrollbar.len
    }

    /// Scroll so the scrollbar thumb's center sits at `y`.
    fn drag_scrollbar(&self, y: f64) {
        let sb = self.state.borrow().scrollbar;
        let h = self.area.height() as f64;
        let (_, thumb) = thumb(&sb, h);
        let track = (h - PAD * 2.0 - thumb).max(1.0);
        let frac = ((y - PAD - thumb / 2.0) / track).clamp(0.0, 1.0);
        let target = (frac * (sb.total - sb.len) as f64).round() as i64;
        if let Some(s) = self.session() {
            s.scroll((target - sb.offset as i64) as i32);
        }
        self.state.borrow_mut().scrolled_at = Some(Instant::now());
    }

    fn wire_mouse(&self) {
        let click = gtk::GestureClick::builder().button(0).build();
        let me = self.clone();
        click.connect_pressed(move |g, n, x, y| {
            me.area.grab_focus();
            let Some(s) = me.session() else { return };
            let button = g.current_button();
            let mods = g.current_event_state();
            let (c, r) = me.cell_at(x, y);
            if button == 1 && me.in_scrollbar(x) {
                me.state.borrow_mut().mouse.scrollbar = true;
                me.drag_scrollbar(y);
                return;
            }
            if button == 1 && n == 1 && mods.contains(gdk::ModifierType::CONTROL_MASK) && me.open_link(c, r) {
                return;
            }
            if me.reporting(mods) {
                let name = button_name(button);
                let mut st = me.state.borrow_mut();
                if !st.mouse.held.contains(&name) {
                    st.mouse.held.push(name);
                }
                st.mouse.last_cell = Some((c, r));
                let held = st.mouse.held.clone();
                drop(st);
                s.mouse("press", name, c, r, &held);
                g.set_state(gtk::EventSequenceState::Claimed);
                return;
            }
            match button {
                1 => {
                    match n {
                        2 => {
                            s.select_word(c, r);
                        }
                        3 => {
                            s.select_line(r);
                        }
                        _ => {
                            let anchor = me.state.borrow().sel_anchor;
                            if mods.contains(gdk::ModifierType::SHIFT_MASK)
                                && let Some(a) = anchor
                            {
                                s.select_range(a, (c, r));
                            } else {
                                s.clear_selection();
                                me.state.borrow_mut().sel_anchor = Some((c, r));
                            }
                        }
                    }
                    if n > 1 {
                        me.copy_primary();
                    }
                    me.state.borrow_mut().mouse.selecting = true;
                }
                2 => me.paste_primary(),
                3 => me.context_menu(x, y),
                _ => {}
            }
        });
        let me = self.clone();
        click.connect_released(move |g, _, x, y| {
            let button = g.current_button();
            let mut st = me.state.borrow_mut();
            st.mouse.scrollbar = false;
            st.mouse.selecting = false;
            let name = button_name(button);
            if let Some(i) = st.mouse.held.iter().position(|b| *b == name) {
                st.mouse.held.remove(i);
                let held = st.mouse.held.clone();
                drop(st);
                let (c, r) = me.cell_at(x, y);
                if let Some(s) = me.session() {
                    s.mouse("release", name, c, r, &held);
                }
            } else if button == 1 {
                drop(st);
                me.copy_primary();
            }
        });
        self.area.add_controller(click);

        let drag = gtk::GestureDrag::new();
        let me = self.clone();
        drag.connect_drag_update(move |g, dx, dy| {
            let Some((sx, sy)) = g.start_point() else { return };
            let (x, y) = (sx + dx, sy + dy);
            let Some(s) = me.session() else { return };
            if me.state.borrow().mouse.scrollbar {
                me.drag_scrollbar(y);
                return;
            }
            let (c, r) = me.cell_at(x, y);
            let mut st = me.state.borrow_mut();
            if !st.mouse.held.is_empty() {
                if st.mouse.last_cell != Some((c, r)) {
                    st.mouse.last_cell = Some((c, r));
                    let held = st.mouse.held.clone();
                    drop(st);
                    s.mouse("motion", "unknown", c, r, &held);
                }
                return;
            }
            if !st.mouse.selecting || dx.abs() + dy.abs() < 3.0 {
                return;
            }
            let anchor = st.sel_anchor;
            drop(st);
            // Past the top or bottom edge, scroll to keep selecting.
            if y < 0.0 {
                s.scroll(-1);
            } else if y > me.area.height() as f64 {
                s.scroll(1);
            }
            if let Some(a) = anchor {
                s.select_range(a, (c, r));
            }
        });
        self.area.add_controller(drag);

        let motion = gtk::EventControllerMotion::new();
        let me = self.clone();
        motion.connect_motion(move |m, x, y| {
            let tracking = me.reporting(m.current_event_state());
            me.area.set_cursor_from_name(Some(if tracking || me.in_scrollbar(x) { "default" } else { "text" }));
            if !tracking || !me.state.borrow().mouse.held.is_empty() {
                return;
            }
            let cell = me.cell_at(x, y);
            let mut st = me.state.borrow_mut();
            if st.mouse.last_cell != Some(cell) {
                st.mouse.last_cell = Some(cell);
                drop(st);
                if let Some(s) = me.session() {
                    s.mouse("motion", "unknown", cell.0, cell.1, &[]);
                }
            }
        });
        self.area.add_controller(motion);

        // Scrolling: history locally, wheel events for programs that track the
        // mouse, and arrow keys for full-screen programs that don't (less, man).
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::KINETIC);
        let me = self.clone();
        scroll.connect_scroll(move |ctl, _dx, dy| {
            let Some(s) = me.session() else { return glib::Propagation::Proceed };
            let mods = ctl.current_event_state();
            if mods.contains(gdk::ModifierType::CONTROL_MASK) {
                // Ctrl+scroll zooms, like the rest of the desktop.
                let action = if dy < 0.0 { "app.font-bigger" } else { "app.font-smaller" };
                let _ = me.area.activate_action(action, None);
                return glib::Propagation::Stop;
            }
            let lines = if ctl.unit() == gdk::ScrollUnit::Surface {
                let h = me.state.borrow().renderer.as_ref().map(|r| r.metrics.cell_h).unwrap_or(16.0);
                dy / h
            } else {
                dy * 3.0
            };
            let mut st = me.state.borrow_mut();
            st.scroll_rem += lines;
            let rows = st.scroll_rem.trunc();
            st.scroll_rem -= rows;
            let alt = st.alt_screen;
            drop(st);
            if rows == 0.0 {
                return glib::Propagation::Stop;
            }
            if me.reporting(mods) {
                let (c, r) = me.state.borrow().mouse.last_cell.unwrap_or((0, 0));
                s.wheel(c, r, rows as f32);
            } else if alt {
                let key = if rows < 0.0 { Key::Up } else { Key::Down };
                for _ in 0..rows.abs() as usize {
                    s.send_key(key, false, false, false, false);
                }
            } else {
                me.scroll_by(rows as i32);
            }
            glib::Propagation::Stop
        });
        self.area.add_controller(scroll);

        // Dropping files types their paths, quoted for the shell; dropping text pastes it.
        let drop = gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
        drop.set_types(&[gdk::FileList::static_type(), glib::Type::STRING]);
        let me = self.clone();
        drop.connect_drop(move |_, value, _, _| {
            let Some(s) = me.session() else { return false };
            let text = if let Ok(files) = value.get::<gdk::FileList>() {
                files.files().iter().filter_map(|f| f.path()).map(|p| shell_quote(&p.to_string_lossy())).collect::<Vec<_>>().join(" ")
            } else if let Ok(t) = value.get::<String>() {
                t
            } else {
                return false;
            };
            s.paste(&text);
            me.area.grab_focus();
            true
        });
        self.area.add_controller(drop);
    }

    /// Ctrl+click on a URL opens it.
    fn open_link(&self, col: u16, row: u16) -> bool {
        let Some(s) = self.session() else { return false };
        s.select_line(row);
        let line = s.selection_text().unwrap_or_default();
        s.clear_selection();
        let Some(url) = url_at(&line, col as usize) else { return false };
        let root = self.area.root().and_downcast::<gtk::Window>();
        gtk::UriLauncher::new(&url).launch(root.as_ref(), None::<&gtk::gio::Cancellable>, |_| {});
        true
    }

    /// X11-style: whatever is selected is in the primary selection.
    fn copy_primary(&self) {
        let Some(t) = self.session().and_then(|s| s.selection_text()) else { return };
        self.area.primary_clipboard().set_text(&t);
        if self.state.borrow().prefs.copy_on_select {
            self.area.clipboard().set_text(&t);
        }
    }

    fn paste_primary(&self) {
        let me = self.clone();
        self.area.primary_clipboard().read_text_async(None::<&gtk::gio::Cancellable>, move |r| {
            if let (Ok(Some(t)), Some(s)) = (r, me.session()) {
                s.paste(&t);
            }
        });
    }

    fn context_menu(&self, x: f64, y: f64) {
        let has_sel = self.session().and_then(|s| s.selection_text()).is_some();
        let control = self.session().is_some_and(|s| s.has_control());
        let menu = gtk::gio::Menu::new();
        let edit = gtk::gio::Menu::new();
        edit.append(Some("Copy"), Some("term.copy"));
        edit.append(Some("Paste"), Some("term.paste"));
        edit.append(Some("Copy All Text"), Some("term.copy-all"));
        menu.append_section(None, &edit);
        let panes = gtk::gio::Menu::new();
        panes.append(Some("Split Right"), Some("app.split-right"));
        panes.append(Some("Split Down"), Some("app.split-down"));
        panes.append(Some("Zoom Pane"), Some("app.zoom-pane"));
        panes.append(Some(if control { "Give Size Back" } else { "Fit to This Window" }), Some("term.fit"));
        menu.append_section(None, &panes);
        let close = gtk::gio::Menu::new();
        close.append(Some("Close Pane"), Some("app.close-pane"));
        menu.append_section(None, &close);

        let group = gtk::gio::SimpleActionGroup::new();
        let act = |name: &str, enabled: bool, f: Box<dyn Fn(&TermView)>| {
            let a = gtk::gio::SimpleAction::new(name, None);
            a.set_enabled(enabled);
            let me = self.clone();
            a.connect_activate(move |_, _| f(&me));
            group.add_action(&a);
        };
        act("copy", has_sel, Box::new(|t| t.copy()));
        act("paste", true, Box::new(|t| t.paste()));
        act(
            "copy-all",
            true,
            Box::new(|t| {
                if let Some(s) = t.session() {
                    t.area.clipboard().set_text(&s.all_text());
                }
            }),
        );
        act(
            "fit",
            true,
            Box::new(|t| {
                if let Some(s) = t.session() {
                    if s.has_control() {
                        s.release_control();
                    } else {
                        s.take_control();
                        t.push_grid();
                    }
                }
                t.area.queue_draw();
            }),
        );
        self.root.insert_action_group("term", Some(&group));
        let pop = gtk::PopoverMenu::from_model(Some(&menu));
        pop.set_parent(&self.root);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.set_has_arrow(false);
        pop.set_halign(gtk::Align::Start);
        pop.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        pop.popup();
    }
}

/// The device pixels behind each logical pixel, fractional scales included.
fn device_scale(w: &impl IsA<gtk::Widget>) -> f64 {
    w.native().and_then(|n| n.surface()).map(|s| s.scale()).unwrap_or_else(|| w.scale_factor() as f64)
}

fn thumb(sb: &Scrollbar, h: f64) -> (f64, f64) {
    let track = h - PAD * 2.0;
    let thumb = (track * sb.len as f64 / sb.total.max(1) as f64).max(28.0).min(track);
    let range = sb.total.saturating_sub(sb.len).max(1) as f64;
    (PAD + (track - thumb) * sb.offset as f64 / range, thumb)
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+=:@%,".contains(c)) {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The http(s) or file URL covering column `col` of a line, if any.
fn url_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let stop = |c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '`' | '|');
    let mut start = col.min(chars.len().checked_sub(1)?);
    if stop(chars[start]) {
        return None;
    }
    while start > 0 && !stop(chars[start - 1]) {
        start -= 1;
    }
    let mut end = start;
    while end < chars.len() && !stop(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    let begin = ["https://", "http://", "file://"].iter().filter_map(|p| word.find(p)).min()?;
    let url = word[begin..].trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '!', '?']);
    (url.len() > 8).then(|| url.to_owned())
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64) {
    let r = w / 2.0;
    cr.new_sub_path();
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 0.0);
    cr.arc(x + r, y + h - r, r, 0.0, std::f64::consts::PI);
    cr.close_path();
}

fn button_name(b: u32) -> &'static str {
    match b {
        1 => "left",
        2 => "middle",
        3 => "right",
        8 => "four",
        9 => "five",
        _ => "unknown",
    }
}

/// The Latin letter on this physical key, for shortcuts typed with another layout.
fn latin(ctl: &gtk::EventControllerKey, code: u32) -> Option<char> {
    let display = ctl.widget()?.display();
    display
        .map_keycode(code)?
        .into_iter()
        .filter(|(k, _)| k.level() == 0)
        .find_map(|(_, kv)| kv.to_unicode().filter(|c| c.is_ascii_graphic()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_urls() {
        let l = "see (https://example.com/a?b=1). or http://x.io";
        assert_eq!(super::url_at(l, 10).as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(super::url_at(l, 2), None);
        assert_eq!(super::url_at(l, 44).as_deref(), Some("http://x.io"));
    }

    #[test]
    fn quotes_paths() {
        assert_eq!(super::shell_quote("/tmp/a.txt"), "/tmp/a.txt");
        assert_eq!(super::shell_quote("/tmp/it's here"), "'/tmp/it'\\''s here'");
    }
}

fn named(k: gdk::Key) -> Option<Key> {
    use gdk::Key as G;
    Some(match k {
        G::Return | G::KP_Enter | G::ISO_Enter => Key::Enter,
        G::Tab | G::ISO_Left_Tab | G::KP_Tab => Key::Tab,
        G::BackSpace => Key::Backspace,
        G::Escape => Key::Escape,
        G::Up | G::KP_Up => Key::Up,
        G::Down | G::KP_Down => Key::Down,
        G::Left | G::KP_Left => Key::Left,
        G::Right | G::KP_Right => Key::Right,
        G::Home | G::KP_Home => Key::Home,
        G::End | G::KP_End => Key::End,
        G::Page_Up | G::KP_Page_Up => Key::PageUp,
        G::Page_Down | G::KP_Page_Down => Key::PageDown,
        G::Delete | G::KP_Delete => Key::Delete,
        G::Insert | G::KP_Insert => Key::Insert,
        _ => {
            use glib::translate::IntoGlib;
            let (f, f1) = (k.into_glib(), G::F1.into_glib());
            if (f1..=G::F24.into_glib()).contains(&f) {
                return Some(Key::F((f - f1 + 1) as u8));
            }
            return None;
        }
    })
}
