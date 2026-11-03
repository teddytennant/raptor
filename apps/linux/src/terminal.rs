//! A GTK terminal widget backed by a `shrew_core::TerminalSession`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;
use gtk::{gdk, glib};
use shrew_core::{Key, Shrew, TerminalListener, TerminalSession, TerminalStatus};

use crate::render::Renderer;

const PAD: f64 = 6.0;

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

struct State {
    session: Option<Arc<TerminalSession>>,
    renderer: Option<Renderer>,
    font_size: f64,
    status: TerminalStatus,
    scroll_rem: f64,
    cursor_on: bool,
    sel_anchor: Option<(u16, u16)>,
    last_grid: (u16, u16),
    on_focus: Option<Rc<dyn Fn()>>,
}

/// The terminal surface plus a status pill overlay.
#[derive(Clone)]
pub struct TermView {
    pub root: gtk::Overlay,
    area: gtk::DrawingArea,
    pill: gtk::Label,
    state: Rc<RefCell<State>>,
}

impl TermView {
    pub fn new(core: &Shrew, server: &str, session: &str, block: &str, take_size: bool, font_size: f64) -> Self {
        let area = gtk::DrawingArea::builder().hexpand(true).vexpand(true).focusable(true).can_focus(true).build();
        area.add_css_class("shrew-terminal");
        let pill = gtk::Label::builder().halign(gtk::Align::Center).valign(gtk::Align::Start).margin_top(10).visible(false).build();
        pill.add_css_class("shrew-pill");
        let root = gtk::Overlay::builder().child(&area).build();
        root.add_css_class("shrew-pane");
        // The focus ring floats above the terminal drawing and never takes input.
        let ring = gtk::Box::builder().hexpand(true).vexpand(true).can_target(false).build();
        ring.add_css_class("shrew-ring");
        root.add_overlay(&ring);
        root.add_overlay(&pill);

        let (tx, rx) = async_channel::bounded(64);
        let sess = core.open_terminal(server, session, block, Arc::new(Bridge(tx))).ok();
        if take_size
            && let Some(s) = &sess {
                s.take_control();
            }
        let state = Rc::new(RefCell::new(State {
            session: sess,
            renderer: None,
            font_size,
            status: TerminalStatus::Connecting,
            scroll_rem: 0.0,
            cursor_on: true,
            sel_anchor: None,
            last_grid: (0, 0),
            on_focus: None,
        }));
        let this = Self { root, area, pill, state };
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
    pub fn set_theme(&self, theme: Option<shrew_core::ColorTheme>) {
        if let Some(s) = self.session() {
            s.set_theme(theme);
        }
        self.area.queue_draw();
    }

    pub fn session(&self) -> Option<Arc<TerminalSession>> {
        self.state.borrow().session.clone()
    }

    pub fn shutdown(&self) {
        if let Some(s) = self.state.borrow_mut().session.take() {
            s.close();
        }
    }

    pub fn set_font_size(&self, size: f64) {
        let mut st = self.state.borrow_mut();
        st.font_size = size;
        if let Some(r) = st.renderer.as_mut() {
            r.set_font_size(&self.area.pango_context(), size);
        }
        drop(st);
        self.push_grid();
        self.area.queue_draw();
    }

    fn update_pill(&self) {
        let st = self.state.borrow();
        let text = match &st.status {
            TerminalStatus::Live => None,
            TerminalStatus::Connecting => Some("Connecting…".to_owned()),
            TerminalStatus::Reconnecting => Some("Reconnecting…".to_owned()),
            TerminalStatus::Exited { code } => Some(if *code == 0 { "Process exited".into() } else { format!("Process exited ({code})") }),
            TerminalStatus::Ended { reason } => Some(reason.clone()),
        };
        self.pill.set_visible(text.is_some());
        self.pill.set_label(text.as_deref().unwrap_or(""));
    }

    fn push_grid(&self) {
        let st = self.state.borrow();
        let (Some(s), Some(r)) = (&st.session, &st.renderer) else { return };
        let (w, h) = (self.area.width() as f64, self.area.height() as f64);
        if w < 10.0 {
            return;
        }
        let (c, rows) = r.grid(w, h, PAD);
        let scale = self.area.scale_factor() as f64;
        s.set_device_grid(c, rows, (r.metrics.cell_w * scale) as u32, (r.metrics.cell_h * scale) as u32);
    }

    fn scale(&self) -> f64 {
        let st = self.state.borrow();
        let (Some(s), Some(r)) = (&st.session, &st.renderer) else { return 1.0 };
        if s.has_control() {
            return 1.0;
        }
        let (cols, _) = s.size();
        (self.area.width() as f64 / (cols as f64 * r.metrics.cell_w + PAD * 2.0)).min(1.0)
    }

    fn cell_at(&self, x: f64, y: f64) -> (u16, u16) {
        let st = self.state.borrow();
        let Some(r) = &st.renderer else { return (0, 0) };
        let s = self.scale();
        (((x - PAD) / s / r.metrics.cell_w).max(0.0) as u16, ((y - PAD) / s / r.metrics.cell_h).max(0.0) as u16)
    }

    fn wire(&self, rx: async_channel::Receiver<Msg>) {
        // Drawing.
        let me = self.clone();
        self.area.set_draw_func(move |area, cr, w, h| {
            let mut st = me.state.borrow_mut();
            if st.renderer.is_none() {
                st.renderer = Some(Renderer::new(&area.pango_context(), st.font_size));
            }
            let Some(sess) = st.session.clone() else { return };
            let frame = sess.frame();
            let focused = area.has_focus();
            let cursor_on = st.cursor_on || !focused;
            let renderer = st.renderer.as_ref().unwrap();
            let scale =
                if sess.has_control() { 1.0 } else { (w as f64 / (frame.cols as f64 * renderer.metrics.cell_w + PAD * 2.0)).min(1.0) };
            renderer.draw(cr, &area.pango_context(), &frame, w as f64, h as f64, PAD, scale, focused, cursor_on);
        });

        // Resize → tell the core our grid.
        let me = self.clone();
        self.area.connect_resize(move |_, _, _| {
            let first = me.state.borrow().renderer.is_none();
            if first {
                let fs = me.state.borrow().font_size;
                me.state.borrow_mut().renderer = Some(Renderer::new(&me.area.pango_context(), fs));
            }
            me.push_grid();
        });

        // Messages from the engine.
        let me = self.clone();
        glib::spawn_future_local(async move {
            while let Ok(m) = rx.recv().await {
                match m {
                    Msg::Frame => {
                        me.state.borrow_mut().cursor_on = true;
                        me.area.queue_draw();
                    }
                    Msg::Status(s) => {
                        me.state.borrow_mut().status = s;
                        me.update_pill();
                    }
                    Msg::Clipboard(t) => me.area.clipboard().set_text(&t),
                }
            }
        });

        // Cursor blink.
        let me = self.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(530), move || {
            if me.area.root().is_none() {
                return glib::ControlFlow::Break;
            }
            let mut st = me.state.borrow_mut();
            st.cursor_on = !st.cursor_on;
            drop(st);
            me.area.queue_draw();
            glib::ControlFlow::Continue
        });

        // Keyboard + input methods.
        let im = gtk::IMMulticontext::new();
        im.set_client_widget(Some(&self.area));
        let me = self.clone();
        im.connect_commit(move |_, text| {
            if let Some(s) = me.session() {
                s.send_text(text);
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_im_context(Some(&im));
        let me = self.clone();
        keys.connect_key_pressed(move |ctl, keyval, _code, mods| {
            let Some(s) = me.session() else { return glib::Propagation::Proceed };
            let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
            let alt = mods.contains(gdk::ModifierType::ALT_MASK);
            let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
            // App shortcuts: Ctrl+Shift+C/V copy/paste, Ctrl+=/- font size.
            if ctrl && shift {
                match keyval.to_lower() {
                    gdk::Key::c => {
                        if let Some(t) = s.selection_text() {
                            me.area.clipboard().set_text(&t);
                        }
                        return glib::Propagation::Stop;
                    }
                    gdk::Key::v => {
                        me.paste();
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            if ctrl && (keyval == gdk::Key::equal || keyval == gdk::Key::plus) {
                let f = me.state.borrow().font_size + 1.0;
                me.set_font_size(f);
                return glib::Propagation::Stop;
            }
            if ctrl && keyval == gdk::Key::minus {
                let f = me.state.borrow().font_size - 1.0;
                me.set_font_size(f);
                return glib::Propagation::Stop;
            }
            if let Some(k) = named(keyval) {
                s.send_key(k, ctrl, alt, shift, false);
                return glib::Propagation::Stop;
            }
            if (ctrl || alt)
                && let Some(ch) = keyval.to_lower().to_unicode().filter(|c| !c.is_control()) {
                    s.send_key(Key::Char(ch), ctrl, alt, shift, false);
                    return glib::Propagation::Stop;
                }
            // Let the IM context turn it into text.
            let _ = ctl;
            glib::Propagation::Proceed
        });
        self.area.add_controller(keys);
        let me = self.clone();
        let im2 = im.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(move |_| {
            im2.focus_in();
            me.root.add_css_class("focused");
            me.area.queue_draw();
            let cb = me.state.borrow().on_focus.clone();
            if let Some(cb) = cb {
                cb();
            }
        });
        let me = self.clone();
        focus.connect_leave(move |_| {
            im.focus_out();
            me.root.remove_css_class("focused");
            me.area.queue_draw();
        });
        self.area.add_controller(focus);

        // Click to focus; double-click selects a word; drag selects.
        let click = gtk::GestureClick::new();
        let me = self.clone();
        click.connect_pressed(move |g, n, x, y| {
            me.area.grab_focus();
            let (c, r) = me.cell_at(x, y);
            let Some(s) = me.session() else { return };
            match n {
                2 => {
                    s.select_word(c, r);
                }
                3 => {
                    s.select_line(r);
                }
                _ => {
                    s.clear_selection();
                    me.state.borrow_mut().sel_anchor = Some((c, r));
                    if s.mouse_tracking() {
                        s.click(c, r);
                    }
                }
            }
            if g.current_button() == 2 {
                me.paste_primary();
            }
        });
        self.area.add_controller(click);
        let drag = gtk::GestureDrag::new();
        let me = self.clone();
        drag.connect_drag_update(move |g, dx, dy| {
            let Some((sx, sy)) = g.start_point() else { return };
            if dx.abs() + dy.abs() < 4.0 {
                return;
            }
            let anchor = me.state.borrow().sel_anchor;
            if let (Some(a), Some(s)) = (anchor, me.session()) {
                let (c, r) = me.cell_at(sx + dx, sy + dy);
                s.select_range(a, (c, r));
            }
        });
        let me = self.clone();
        drag.connect_drag_end(move |_, _, _| {
            // X11-style: selecting copies to the primary selection.
            if let Some(t) = me.session().and_then(|s| s.selection_text()) {
                me.area.primary_clipboard().set_text(&t);
            }
        });
        self.area.add_controller(drag);

        // Right-click menu.
        let menu_click = gtk::GestureClick::builder().button(3).build();
        let me = self.clone();
        menu_click.connect_pressed(move |_, _, x, y| me.context_menu(x, y));
        self.area.add_controller(menu_click);

        // Scrolling: history locally, or wheel events for full-screen programs.
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::KINETIC);
        let me = self.clone();
        scroll.connect_scroll(move |ctl, _dx, dy| {
            let Some(s) = me.session() else { return glib::Propagation::Proceed };
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
            drop(st);
            if rows != 0.0 {
                if s.mouse_tracking() {
                    let (c, r) = me.cell_at(me.area.width() as f64 / 2.0, me.area.height() as f64 / 2.0);
                    s.wheel(c, r, rows as f32);
                } else {
                    s.scroll(rows as i32);
                }
            }
            glib::Propagation::Stop
        });
        self.area.add_controller(scroll);

        let _ = self.state.borrow().last_grid;
    }

    fn paste(&self) {
        let me = self.clone();
        self.area.clipboard().read_text_async(None::<&gtk::gio::Cancellable>, move |r| {
            if let (Ok(Some(t)), Some(s)) = (r, me.session()) {
                s.paste(&t);
            }
        });
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
        let pop = gtk::Popover::new();
        let b = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let mk = |label: &str| {
            let btn = gtk::Button::with_label(label);
            btn.add_css_class("flat");
            btn.set_halign(gtk::Align::Fill);
            b.append(&btn);
            btn
        };
        let me = self.clone();
        let p = pop.clone();
        mk("Copy").connect_clicked(move |_| {
            if let Some(t) = me.session().and_then(|s| s.selection_text()) {
                me.area.clipboard().set_text(&t);
            }
            p.popdown();
        });
        let me = self.clone();
        let p = pop.clone();
        mk("Paste").connect_clicked(move |_| {
            me.paste();
            p.popdown();
        });
        let me = self.clone();
        let p = pop.clone();
        mk("Copy All").connect_clicked(move |_| {
            if let Some(s) = me.session() {
                me.area.clipboard().set_text(&s.all_text());
            }
            p.popdown();
        });
        let me = self.clone();
        let p = pop.clone();
        let has = self.session().is_some_and(|s| s.has_control());
        mk(if has { "Give Size Back" } else { "Fit to This Window" }).connect_clicked(move |_| {
            if let Some(s) = me.session() {
                if s.has_control() {
                    s.release_control();
                } else {
                    s.take_control();
                    me.push_grid();
                }
            }
            p.popdown();
        });
        pop.set_child(Some(&b));
        pop.set_parent(&self.area);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.set_has_arrow(false);
        pop.connect_closed(|p| p.unparent());
        pop.popup();
    }
}

fn named(k: gdk::Key) -> Option<Key> {
    use gdk::Key as G;
    Some(match k {
        G::Return | G::KP_Enter => Key::Enter,
        G::Tab | G::ISO_Left_Tab => Key::Tab,
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
        G::F1 => Key::F(1),
        G::F2 => Key::F(2),
        G::F3 => Key::F(3),
        G::F4 => Key::F(4),
        G::F5 => Key::F(5),
        G::F6 => Key::F(6),
        G::F7 => Key::F(7),
        G::F8 => Key::F(8),
        G::F9 => Key::F(9),
        G::F10 => Key::F(10),
        G::F11 => Key::F(11),
        G::F12 => Key::F(12),
        _ => return None,
    })
}
