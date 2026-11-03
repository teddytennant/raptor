//! A terminal a UI is showing: its stream, its emulator, and its input queue.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::json;
use shrew_client::{DataEvent, DataStream, KeyPress, Mods};
use shrew_term::{Frame, Rgb, Terminal};
use tokio::sync::{mpsc, watch};

use crate::server::ServerEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalStatus {
    Connecting,
    Live,
    Reconnecting,
    /// The program exited; the block is still open.
    Exited {
        code: i32,
    },
    /// The block closed or the stream can't continue.
    Ended {
        reason: String,
    },
}

pub trait TerminalListener: Send + Sync {
    /// New content is ready; call [`TerminalSession::frame`] on the next display refresh.
    fn frame_ready(&self);
    fn status_changed(&self, status: TerminalStatus);
    /// A program copied text (OSC 52) or the user finished a selection remotely.
    fn clipboard(&self, text: String);
    /// The remote terminal size changed (cols, rows).
    fn size_changed(&self, cols: u16, rows: u16);
}

/// Named keys a touch UI or hardware keyboard can send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
    /// A character key, e.g. `Char('c')` with ctrl for ^C.
    Char(char),
}

impl Key {
    fn press(self, mods: Mods) -> KeyPress {
        let (code, text): (String, String) = match self {
            Key::Enter => ("enter".into(), "\r".into()),
            Key::Tab => ("tab".into(), "\t".into()),
            Key::Backspace => ("backspace".into(), String::new()),
            Key::Escape => ("escape".into(), String::new()),
            Key::Up => ("arrow_up".into(), String::new()),
            Key::Down => ("arrow_down".into(), String::new()),
            Key::Left => ("arrow_left".into(), String::new()),
            Key::Right => ("arrow_right".into(), String::new()),
            Key::Home => ("home".into(), String::new()),
            Key::End => ("end".into(), String::new()),
            Key::PageUp => ("page_up".into(), String::new()),
            Key::PageDown => ("page_down".into(), String::new()),
            Key::Delete => ("delete".into(), String::new()),
            Key::Insert => ("insert".into(), String::new()),
            Key::F(n) => (format!("f{}", n.clamp(1, 24)), String::new()),
            Key::Char(c) => {
                let text = if mods.ctrl || mods.alt || mods.meta { String::new() } else { c.to_string() };
                (shrew_client::char_code(c), text)
            }
        };
        let mods = Mods { shift: mods.shift || matches!(self, Key::Char(c) if c.is_ascii_uppercase()), ..mods };
        KeyPress { key: code, mods, text }
    }
}

fn plain_bytes(key: Key, ctrl: bool, alt: bool, shift: bool, meta: bool) -> Option<String> {
    if alt || meta {
        return None;
    }
    match key {
        Key::Enter if !ctrl && !shift => Some("\r".into()),
        Key::Tab if !ctrl && !shift => Some("\t".into()),
        Key::Backspace if !ctrl && !shift => Some("\x7f".into()),
        Key::Char(c) if ctrl && c.is_ascii_alphabetic() => Some(((c.to_ascii_lowercase() as u8 - b'a' + 1) as char).to_string()),
        Key::Char(c) if !ctrl => Some(if shift { c.to_uppercase().collect() } else { c.to_string() }),
        _ => None,
    }
}

/// A color theme the user picked in the app. While set, it wins over the
/// server's theme so every terminal looks the same.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorTheme {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

pub const DEFAULT_FG: Rgb = Rgb::new(0xe6, 0xed, 0xf3);
pub const DEFAULT_BG: Rgb = Rgb::new(0x0d, 0x11, 0x17);
pub const DEFAULT_CURSOR: Rgb = Rgb::new(0x34, 0xd3, 0x99);

fn apply(t: &mut Terminal, th: &ColorTheme) {
    t.set_default_colors(Some(th.foreground), Some(th.background), Some(th.cursor));
    t.set_ansi_palette(Some(&th.ansi));
}

enum Input {
    Text(String),
    Key(KeyPress),
    Paste(String),
}

pub struct TerminalSession {
    server: Arc<ServerEntry>,
    pub session_id: String,
    pub block_id: String,
    term: Mutex<Terminal>,
    status: Mutex<TerminalStatus>,
    pending: AtomicBool,
    listener: Arc<dyn TerminalListener>,
    input: mpsc::UnboundedSender<Input>,
    stop: watch::Sender<bool>,
    /// The device's own grid size, for "fit to this screen".
    device: Mutex<Option<(u16, u16, u32, u32)>>,
    /// Bumped on every grid change; a pending resize only fires if it's still current.
    resize_gen: Arc<AtomicU64>,
    control: AtomicBool,
    forced: Mutex<Option<ColorTheme>>,
    /// Methods are called from UI threads, so spawn on the engine's runtime explicitly.
    rt: tokio::runtime::Handle,
}

impl TerminalSession {
    pub(crate) fn open(server: Arc<ServerEntry>, session_id: String, block_id: String, listener: Arc<dyn TerminalListener>) -> Arc<Self> {
        let (input, input_rx) = mpsc::unbounded_channel();
        let mut term = Terminal::new(80, 24);
        term.set_scrollback_limit(if cfg!(any(target_os = "ios", target_os = "android")) { 4 << 20 } else { 16 << 20 });
        let this = Arc::new(Self {
            server,
            session_id,
            block_id,
            term: Mutex::new(term),
            status: Mutex::new(TerminalStatus::Connecting),
            pending: AtomicBool::new(false),
            listener,
            input,
            stop: watch::channel(false).0,
            device: Mutex::new(None),
            resize_gen: Arc::new(AtomicU64::new(0)),
            control: AtomicBool::new(false),
            forced: Mutex::new(None),
            rt: tokio::runtime::Handle::current(),
        });
        this.rt.spawn(this.clone().stream_task());
        this.rt.spawn(this.clone().input_task(input_rx));
        this
    }

    pub fn status(&self) -> TerminalStatus {
        self.status.lock().clone()
    }

    fn set_status(&self, s: TerminalStatus) {
        let mut cur = self.status.lock();
        if *cur != s {
            *cur = s.clone();
            drop(cur);
            self.listener.status_changed(s);
        }
    }

    fn dirty(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            self.listener.frame_ready();
        }
    }

    /// Build the frame to draw. Clears the "frame ready" latch.
    pub fn frame(&self) -> Frame {
        self.pending.store(false, Ordering::Release);
        self.term.lock().frame()
    }

    pub fn size(&self) -> (u16, u16) {
        self.term.lock().size()
    }

    pub fn title(&self) -> Option<String> {
        self.term.lock().title()
    }

    pub fn close(&self) {
        let _ = self.stop.send(true);
        if self.control.swap(false, Ordering::AcqRel) {
            let (server, sid, bid) = (self.server.clone(), self.session_id.clone(), self.block_id.clone());
            self.rt.spawn(async move {
                if let Some(c) = server.connection() {
                    let _ = c.disown_block(&sid, &bid).await;
                }
            });
        }
    }

    async fn stream_task(self: Arc<Self>) {
        let mut stop = self.stop.subscribe();
        // Get a ticket (needs a live, attached control connection).
        let ticket = loop {
            if *stop.borrow() {
                return;
            }
            match self.server.wait_connection(Duration::from_secs(30)).await {
                Some(c) => match c.data_ticket(&self.session_id, &self.block_id).await {
                    Ok(t) => break t,
                    Err(e) if e.api_code() == Some("not_found") || e.api_code() == Some("session_not_found") => {
                        self.set_status(TerminalStatus::Ended { reason: "This terminal no longer exists".into() });
                        return;
                    }
                    Err(e) => {
                        // Not attached yet (conflict) or a hiccup: try shortly.
                        tracing::debug!("ticket: {e}");
                        tokio::time::sleep(Duration::from_millis(400)).await;
                    }
                },
                None => self.set_status(TerminalStatus::Reconnecting),
            }
        };
        let endpoint = match shrew_client::Endpoint::parse(&self.server.profile.lock().address) {
            Ok(e) => e,
            Err(e) => return self.set_status(TerminalStatus::Ended { reason: e.to_string() }),
        };
        let (ds, mut rx) = DataStream::open(endpoint, ticket);
        loop {
            tokio::select! {
                _ = stop.changed() => { drop(ds); return; }
                ev = rx.recv() => {
                    let Some(ev) = ev else { return };
                    match ev {
                        DataEvent::Started => self.set_status(TerminalStatus::Connecting),
                        DataEvent::Snapshot(s) => {
                            let r = {
                                let mut t = self.term.lock();
                                let r = t.restore(&s);
                                // Shrew's palette unless the server sends a theme (which follows).
                                t.set_default_colors(Some(DEFAULT_FG), Some(DEFAULT_BG), Some(DEFAULT_CURSOR));
                                if let Some(th) = *self.forced.lock() {
                                    apply(&mut t, &th);
                                }
                                r
                            };
                            if let Err(e) = r { tracing::warn!("restore: {e}"); }
                            let (c, r) = self.size();
                            self.listener.size_changed(c, r);
                            self.set_status(TerminalStatus::Live);
                            self.dirty();
                        }
                        DataEvent::Output(o) => {
                            self.term.lock().write_vt(&o);
                            self.dirty();
                        }
                        DataEvent::Resize { cols, rows } => {
                            let changed = {
                                let mut t = self.term.lock();
                                let (oc, or) = t.size();
                                if (oc, or) != (cols as u16, rows as u16) {
                                    t.resize(cols as u16, rows as u16, 0, 0);
                                    true
                                } else { false }
                            };
                            if changed { self.listener.size_changed(cols as u16, rows as u16); self.dirty(); }
                        }
                        DataEvent::Theme { .. } if self.forced.lock().is_some() => {}
                        DataEvent::Theme { foreground, background, cursor, .. } => {
                            let p = |s: &Option<String>, d: Rgb| s.as_deref().and_then(Rgb::parse_hex).or(Some(d));
                            self.term.lock().set_default_colors(p(&foreground, DEFAULT_FG), p(&background, DEFAULT_BG), p(&cursor, DEFAULT_CURSOR));
                            self.dirty();
                        }
                        DataEvent::ChildExited { exit_code } => self.set_status(TerminalStatus::Exited { code: exit_code }),
                        DataEvent::Reconnecting => self.set_status(TerminalStatus::Reconnecting),
                        DataEvent::Resumed => self.set_status(TerminalStatus::Live),
                        DataEvent::NeedsTicket => {
                            self.set_status(TerminalStatus::Reconnecting);
                            let this = self.clone();
                            let ticket = loop {
                                if *stop.borrow() { return; }
                                if let Some(c) = this.server.wait_connection(Duration::from_secs(30)).await { match c.data_ticket(&this.session_id, &this.block_id).await {
                                    Ok(t) => break t,
                                    Err(e) if e.api_code() == Some("not_found") || e.api_code() == Some("session_not_found") => {
                                        return this.set_status(TerminalStatus::Ended { reason: "This terminal closed".into() });
                                    }
                                    Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
                                } }
                            };
                            ds.restart(ticket);
                        }
                        DataEvent::Ended { reason } => {
                            let reason = if reason.contains("block closed") { "This terminal closed".to_owned() } else { reason };
                            return self.set_status(TerminalStatus::Ended { reason });
                        }
                    }
                }
            }
        }
    }

    async fn input_task(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<Input>) {
        while let Some(first) = rx.recv().await {
            // Merge queued text so fast typing over slow links costs one round trip.
            let mut batch = vec![first];
            while let Ok(more) = rx.try_recv() {
                match (batch.last_mut(), more) {
                    (Some(Input::Text(t)), Input::Text(m)) => t.push_str(&m),
                    (_, m) => batch.push(m),
                }
            }
            let Some(conn) = self.server.wait_connection(Duration::from_secs(10)).await else { continue };
            for item in batch {
                let r = match &item {
                    Input::Text(t) => conn.write(&self.session_id, &self.block_id, t).await,
                    Input::Key(k) => conn.key(&self.session_id, &self.block_id, k).await,
                    Input::Paste(p) => conn.paste(&self.session_id, &self.block_id, p).await,
                };
                if let Err(e) = r {
                    tracing::warn!("input: {e}");
                }
            }
        }
    }

    /// Type text (soft keyboard, IME commit, dictation).
    pub fn send_text(&self, text: &str) {
        if !text.is_empty() {
            self.snap_to_bottom();
            let _ = self.input.send(Input::Text(text.to_owned()));
        }
    }

    pub fn send_key(&self, key: Key, ctrl: bool, alt: bool, shift: bool, meta: bool) {
        self.snap_to_bottom();
        // Keys whose bytes don't depend on terminal modes ride along with typed
        // text, so "command + Enter" is a single round trip.
        if let Some(t) = plain_bytes(key, ctrl, alt, shift, meta) {
            let _ = self.input.send(Input::Text(t));
            return;
        }
        let _ = self.input.send(Input::Key(key.press(Mods { shift, ctrl, alt, meta })));
    }

    pub fn paste(&self, text: &str) {
        if !text.is_empty() {
            self.snap_to_bottom();
            let _ = self.input.send(Input::Paste(text.to_owned()));
        }
    }

    fn snap_to_bottom(&self) {
        let mut t = self.term.lock();
        if !t.scrollbar().at_bottom() {
            t.scroll_to_bottom();
            drop(t);
            self.dirty();
        }
    }

    /// Scroll local history by rows (negative = up).
    pub fn scroll(&self, rows: i32) {
        self.term.lock().scroll(rows as isize);
        self.dirty();
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_to_bottom();
        self.dirty();
    }

    pub fn jump_prompt(&self, delta: i32) {
        self.term.lock().scroll_prompts(delta as isize);
        self.dirty();
    }

    pub fn select_word(&self, col: u16, row: u16) -> bool {
        let ok = self.term.lock().select_word(col, row);
        self.dirty();
        ok
    }

    pub fn select_line(&self, row: u16) -> bool {
        let ok = self.term.lock().select_line(row);
        self.dirty();
        ok
    }

    pub fn select_range(&self, from: (u16, u16), to: (u16, u16)) -> bool {
        let ok = self.term.lock().select(from, to);
        self.dirty();
        ok
    }

    pub fn clear_selection(&self) {
        self.term.lock().clear_selection();
        self.dirty();
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term.lock().selection_text()
    }

    pub fn all_text(&self) -> String {
        self.term.lock().all_text()
    }

    /// Tell the core how big this device's view is, in cells and pixels.
    /// If this device has control, the remote terminal is resized to match.
    pub fn set_device_grid(&self, cols: u16, rows: u16, cell_w: u32, cell_h: u32) {
        let prev = self.device.lock().replace((cols, rows, cell_w, cell_h));
        if self.control.load(Ordering::Acquire) && prev.map(|p| (p.0, p.1)) != Some((cols, rows)) {
            self.push_resize_settled();
        }
    }

    /// Force a theme (or `None` to go back to the server's colors).
    pub fn set_theme(&self, theme: Option<ColorTheme>) {
        *self.forced.lock() = theme;
        let mut t = self.term.lock();
        match theme {
            Some(th) => apply(&mut t, &th),
            None => {
                t.set_ansi_palette(None);
                t.set_default_colors(Some(DEFAULT_FG), Some(DEFAULT_BG), Some(DEFAULT_CURSOR));
            }
        }
        drop(t);
        self.dirty();
    }

    pub fn has_control(&self) -> bool {
        self.control.load(Ordering::Acquire)
    }

    /// Own the block so the remote terminal uses this device's size.
    pub fn take_control(self: &Arc<Self>) {
        self.control.store(true, Ordering::Release);
        let this = self.clone();
        self.rt.spawn(async move {
            if let Some(c) = this.server.wait_connection(Duration::from_secs(10)).await {
                if let Err(e) = c.own_block(&this.session_id, &this.block_id, true).await {
                    tracing::warn!("own_block: {e}");
                }
                this.push_resize();
            }
        });
    }

    /// Give the size back to whoever had it (e.g. the desktop app).
    pub fn release_control(self: &Arc<Self>) {
        if self.control.swap(false, Ordering::AcqRel) {
            let this = self.clone();
            self.rt.spawn(async move {
                if let Some(c) = this.server.connection() {
                    let _ = c.disown_block(&this.session_id, &this.block_id).await;
                }
            });
        }
    }

    /// Resize once the grid stops changing. While the keyboard slides in, the
    /// view's height changes every frame; sending each step would make the
    /// program redraw a dozen times (visible stutter and reflow). 120 ms of
    /// quiet is shorter than a person notices, longer than an animation frame.
    fn push_resize_settled(&self) {
        let ticket = self.resize_gen.fetch_add(1, Ordering::AcqRel) + 1;
        let latest = self.resize_gen.clone();
        let device = *self.device.lock();
        let (server, sid, bid) = (self.server.clone(), self.session_id.clone(), self.block_id.clone());
        self.rt.spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            if latest.load(Ordering::Acquire) != ticket {
                return; // a newer size superseded this one
            }
            let Some((cols, rows, cw, ch)) = device else { return };
            if let Some(c) = server.connection() {
                let px = (cw > 0 && ch > 0).then_some((cw, ch));
                if let Err(e) = c.resize(&sid, &bid, cols as u32, rows as u32, px).await {
                    tracing::warn!("resize: {e}");
                }
            }
        });
    }

    fn push_resize(&self) {
        let Some((cols, rows, cw, ch)) = *self.device.lock() else { return };
        let (server, sid, bid) = (self.server.clone(), self.session_id.clone(), self.block_id.clone());
        self.rt.spawn(async move {
            if let Some(c) = server.connection() {
                let px = (cw > 0 && ch > 0).then_some((cw, ch));
                if let Err(e) = c.resize(&sid, &bid, cols as u32, rows as u32, px).await {
                    tracing::warn!("resize: {e}");
                }
            }
        });
    }

    /// Ask the remote side for the full text (including history the stream didn't carry).
    pub async fn remote_text(&self) -> Option<String> {
        let c = self.server.connection()?;
        let (sid, bid) = (self.session_id.clone(), self.block_id.clone());
        self.rt.spawn(async move { c.screen_text(&sid, &bid, Some(4 << 20)).await.ok() }).await.ok().flatten()
    }

    /// Send a mouse click at a cell, for programs that track the mouse.
    pub fn click(&self, col: u16, row: u16) {
        let (server, sid, bid) = (self.server.clone(), self.session_id.clone(), self.block_id.clone());
        self.rt.spawn(async move {
            let Some(c) = server.connection() else { return };
            for (action, held) in [("press", json!({"left": true})), ("release", serde_json::Value::Null)] {
                let _ = c
                    .call(
                        "block.event",
                        json!({"session_id": sid, "block_id": bid, "event": {"type": "mouse",
                            "payload": {"action": action, "button": "left", "buttons": held, "x": col, "y": row}}}),
                    )
                    .await;
            }
        });
    }

    /// Scroll wheel for full-screen programs (vim, less, htop). Positive is down.
    pub fn wheel(&self, col: u16, row: u16, lines: f32) {
        let (server, sid, bid) = (self.server.clone(), self.session_id.clone(), self.block_id.clone());
        self.rt.spawn(async move {
            if let Some(c) = server.connection() {
                let _ = c
                    .call(
                        "block.event",
                        json!({"session_id": sid, "block_id": bid, "event": {"type": "wheel",
                            "payload": {"delta_x": 0, "delta_y": lines, "delta_mode": "line", "x": col, "y": row}}}),
                    )
                    .await;
            }
        });
    }

    pub fn mouse_tracking(&self) -> bool {
        self.term.lock().mouse_tracking()
    }

    pub(crate) fn on_clipboard(&self, text: String) {
        self.listener.clipboard(text);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}
