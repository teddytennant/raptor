//! Swift and Kotlin bindings for the Shrew engine (UniFFI).
//!
//! Types here mirror `shrew_core` in FFI-friendly shapes: colors are `0xAARRGGBB`
//! integers and text styles are bit flags, so a full terminal frame crosses the
//! boundary cheaply on every display refresh.

use std::path::PathBuf;
use std::sync::Arc;

use shrew_core as core;

uniffi::setup_scaffolding!();

// ---- errors -------------------------------------------------------------------

#[derive(Debug, uniffi::Error)]
pub enum ShrewError {
    UnknownServer,
    NotConnected,
    InvalidAddress { detail: String },
    Server { detail: String },
}

impl std::fmt::Display for ShrewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownServer => f.write_str("Unknown server"),
            Self::NotConnected => f.write_str("Not connected"),
            Self::InvalidAddress { detail } | Self::Server { detail } => f.write_str(detail),
        }
    }
}

impl std::error::Error for ShrewError {}

impl From<core::CoreError> for ShrewError {
    fn from(e: core::CoreError) -> Self {
        match e {
            core::CoreError::UnknownServer => Self::UnknownServer,
            core::CoreError::NotConnected => Self::NotConnected,
            core::CoreError::InvalidAddress(detail) => Self::InvalidAddress { detail },
            core::CoreError::Server(detail) => Self::Server { detail },
        }
    }
}

type Result<T> = std::result::Result<T, ShrewError>;

// ---- state records ------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AgentState {
    Idle,
    Done,
    Working,
    Error,
    Blocked,
}

impl From<core::AgentState> for AgentState {
    fn from(s: core::AgentState) -> Self {
        match s {
            core::AgentState::Idle => Self::Idle,
            core::AgentState::Done => Self::Done,
            core::AgentState::Working => Self::Working,
            core::AgentState::Error => Self::Error,
            core::AgentState::Blocked => Self::Blocked,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AgentStatus {
    pub state: AgentState,
    pub waiting_for: Option<String>,
    pub app: String,
    pub title: String,
    pub message: String,
    pub progress: Option<u8>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Block {
    pub id: String,
    pub name: String,
    pub label: String,
    pub title: String,
    pub pwd: String,
    pub short_pwd: String,
    pub process: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub floating: bool,
    pub owned_by_other: bool,
    pub owned_by_me: bool,
    pub exited: Option<i32>,
    pub agent: Option<AgentStatus>,
    pub progress: Option<u8>,
    pub has_bell: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Window {
    pub id: String,
    pub label: String,
    pub focused_block_id: Option<String>,
    pub zoomed_block_id: Option<String>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Session {
    pub id: String,
    pub label: String,
    pub active_window_id: Option<String>,
    pub windows: Vec<Window>,
    pub clients: Vec<String>,
    pub agent_state: Option<AgentState>,
    pub block_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Connectivity {
    Idle,
    Connecting,
    Online,
    Offline { error: String, retry_in_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub address: String,
    pub connectivity: Connectivity,
    pub latency_ms: Option<u32>,
    pub version: Option<String>,
    pub host_name: Option<String>,
    pub is_local: bool,
    pub sessions: Vec<Session>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AttentionItem {
    pub server_id: String,
    pub server_name: String,
    pub session_id: String,
    pub session_label: String,
    pub window_id: String,
    pub block: Block,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AppState {
    pub servers: Vec<Server>,
    /// Blocks whose programs report a status, most urgent first.
    pub attention: Vec<AttentionItem>,
}

fn block(b: &core::BlockState, me_connected: bool) -> Block {
    let _ = me_connected;
    Block {
        id: b.id.clone(),
        name: b.display_name(),
        label: b.label.clone(),
        title: b.title.clone(),
        pwd: b.pwd.clone(),
        short_pwd: if b.pwd.is_empty() { String::new() } else { core::short_path(&b.pwd) },
        process: b.process.clone(),
        x: b.rect[0],
        y: b.rect[1],
        w: b.rect[2],
        h: b.rect[3],
        floating: b.floating,
        owned_by_other: b.owner.is_some() && !b.owned_by_me,
        owned_by_me: b.owned_by_me,
        exited: b.exited,
        agent: b.agent.as_ref().map(|a| AgentStatus {
            state: a.state.into(),
            waiting_for: a.waiting_for.clone(),
            app: a.app.clone(),
            title: a.title.clone(),
            message: a.message.clone(),
            progress: a.progress,
            updated_at: a.updated_at.clone(),
        }),
        progress: b.progress,
        has_bell: b.has_bell,
    }
}

impl From<core::AppState> for AppState {
    fn from(s: core::AppState) -> Self {
        let attention = s
            .attention()
            .into_iter()
            .map(|a| AttentionItem {
                server_id: a.server_id,
                server_name: a.server_name,
                session_id: a.session_id,
                session_label: a.session_label,
                window_id: a.window_id,
                block: block(&a.block, true),
            })
            .collect();
        let servers = s
            .servers
            .iter()
            .map(|srv| Server {
                id: srv.id.clone(),
                name: srv.name.clone(),
                address: srv.address.clone(),
                connectivity: match &srv.connectivity {
                    core::Connectivity::Idle => Connectivity::Idle,
                    core::Connectivity::Connecting => Connectivity::Connecting,
                    core::Connectivity::Online => Connectivity::Online,
                    core::Connectivity::Offline { error, retry_in_ms } => {
                        Connectivity::Offline { error: error.clone(), retry_in_ms: *retry_in_ms }
                    }
                },
                latency_ms: srv.latency_ms,
                version: srv.version.clone(),
                host_name: srv.host_name.clone(),
                is_local: srv.is_local,
                sessions: srv
                    .sessions
                    .iter()
                    .map(|se| Session {
                        id: se.id.clone(),
                        label: se.label.clone(),
                        active_window_id: se.active_window_id.clone(),
                        agent_state: se.agent_state().map(Into::into),
                        block_count: se.block_count() as u32,
                        clients: se.clients.clone(),
                        windows: se
                            .windows
                            .iter()
                            .map(|w| Window {
                                id: w.id.clone(),
                                label: w.label.clone(),
                                focused_block_id: w.focused_block_id.clone(),
                                zoomed_block_id: w.zoomed_block_id.clone(),
                                blocks: w.blocks.iter().map(|b| block(b, true)).collect(),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();
        AppState { servers, attention }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NotificationKind {
    AgentBlocked,
    AgentDone,
    AgentError,
    Program,
    Bell,
    ProcessExited,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Notification {
    pub kind: NotificationKind,
    pub server_id: String,
    pub session_id: String,
    pub block_id: String,
    pub title: String,
    pub body: String,
}

impl From<core::Notification> for Notification {
    fn from(n: core::Notification) -> Self {
        Self {
            kind: match n.kind {
                core::NotificationKind::AgentBlocked => NotificationKind::AgentBlocked,
                core::NotificationKind::AgentDone => NotificationKind::AgentDone,
                core::NotificationKind::AgentError => NotificationKind::AgentError,
                core::NotificationKind::Program => NotificationKind::Program,
                core::NotificationKind::Bell => NotificationKind::Bell,
                core::NotificationKind::ProcessExited => NotificationKind::ProcessExited,
            },
            server_id: n.server_id,
            session_id: n.session_id,
            block_id: n.block_id,
            title: n.title,
            body: n.body,
        }
    }
}

// ---- frames -------------------------------------------------------------------

pub const STYLE_BOLD: u16 = 1;
pub const STYLE_ITALIC: u16 = 1 << 1;
pub const STYLE_FAINT: u16 = 1 << 2;
pub const STYLE_STRIKE: u16 = 1 << 3;
pub const STYLE_OVERLINE: u16 = 1 << 4;
pub const STYLE_SELECTED: u16 = 1 << 5;
/// Bits 8..11 hold the underline kind: 0 none, 1 single, 2 double, 3 curly, 4 dotted, 5 dashed.
pub const STYLE_UNDERLINE_SHIFT: u16 = 8;

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Run {
    pub col: u16,
    pub cells: u16,
    pub text: String,
    /// Cell width of each grapheme in `text` (1 or 2).
    pub widths: Vec<u8>,
    /// 0xAARRGGBB
    pub fg: u32,
    pub bg: Option<u32>,
    pub underline_color: Option<u32>,
    pub style: u16,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Line {
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
    HollowBlock,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub shape: CursorShape,
    pub blinking: bool,
    pub password: bool,
    pub wide: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    pub background: u32,
    pub foreground: u32,
    pub cursor_color: Option<u32>,
    pub cursor: Option<Cursor>,
    pub lines: Vec<Line>,
    pub scroll_total: u64,
    pub scroll_offset: u64,
    pub scroll_len: u64,
    pub alternate_screen: bool,
    pub dark: bool,
}

impl From<core::Frame> for Frame {
    fn from(f: core::Frame) -> Self {
        let lines = f
            .lines
            .into_iter()
            .map(|r| Line {
                runs: r
                    .runs
                    .into_iter()
                    .map(|run| {
                        let mut style = 0u16;
                        if run.bold {
                            style |= STYLE_BOLD;
                        }
                        if run.italic {
                            style |= STYLE_ITALIC;
                        }
                        if run.faint {
                            style |= STYLE_FAINT;
                        }
                        if run.strikethrough {
                            style |= STYLE_STRIKE;
                        }
                        if run.overline {
                            style |= STYLE_OVERLINE;
                        }
                        if run.selected {
                            style |= STYLE_SELECTED;
                        }
                        let ul: u16 = match run.underline {
                            core::Underline::None => 0,
                            core::Underline::Single => 1,
                            core::Underline::Double => 2,
                            core::Underline::Curly => 3,
                            core::Underline::Dotted => 4,
                            core::Underline::Dashed => 5,
                        };
                        style |= ul << STYLE_UNDERLINE_SHIFT;
                        Run {
                            col: run.col,
                            cells: run.cells,
                            text: run.text,
                            widths: run.widths,
                            fg: run.fg.to_u32(),
                            bg: run.bg.map(|c| c.to_u32()),
                            underline_color: run.underline_color.map(|c| c.to_u32()),
                            style,
                        }
                    })
                    .collect(),
            })
            .collect();
        Frame {
            cols: f.cols,
            rows: f.rows,
            background: f.background.to_u32(),
            foreground: f.foreground.to_u32(),
            cursor_color: f.cursor_color.map(|c| c.to_u32()),
            cursor: f.cursor.map(|c| Cursor {
                col: c.col,
                row: c.row,
                shape: match c.style {
                    core::CursorStyle::Block => CursorShape::Block,
                    core::CursorStyle::Bar => CursorShape::Bar,
                    core::CursorStyle::Underline => CursorShape::Underline,
                    core::CursorStyle::HollowBlock => CursorShape::HollowBlock,
                },
                blinking: c.blinking,
                password: c.password,
                wide: c.wide,
            }),
            lines,
            scroll_total: f.scrollbar.total,
            scroll_offset: f.scrollbar.offset,
            scroll_len: f.scrollbar.len,
            alternate_screen: f.alternate_screen,
            dark: f.background.luminance() < 0.4,
        }
    }
}

// ---- listeners ----------------------------------------------------------------

#[uniffi::export(with_foreign)]
pub trait AppListener: Send + Sync {
    fn state_changed(&self);
    fn notify(&self, notification: Notification);
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum TerminalStatus {
    Connecting,
    Live,
    Reconnecting,
    Exited { code: i32 },
    Ended { reason: String },
}

impl From<core::TerminalStatus> for TerminalStatus {
    fn from(s: core::TerminalStatus) -> Self {
        match s {
            core::TerminalStatus::Connecting => Self::Connecting,
            core::TerminalStatus::Live => Self::Live,
            core::TerminalStatus::Reconnecting => Self::Reconnecting,
            core::TerminalStatus::Exited { code } => Self::Exited { code },
            core::TerminalStatus::Ended { reason } => Self::Ended { reason },
        }
    }
}

#[uniffi::export(with_foreign)]
pub trait TerminalListener: Send + Sync {
    fn frame_ready(&self);
    fn status_changed(&self, status: TerminalStatus);
    fn clipboard(&self, text: String);
    fn size_changed(&self, cols: u16, rows: u16);
}

struct AppBridge(Arc<dyn AppListener>);
impl core::Listener for AppBridge {
    fn state_changed(&self) {
        self.0.state_changed()
    }
    fn notify(&self, n: core::Notification) {
        self.0.notify(n.into())
    }
}

struct TermBridge(Arc<dyn TerminalListener>);
impl core::TerminalListener for TermBridge {
    fn frame_ready(&self) {
        self.0.frame_ready()
    }
    fn status_changed(&self, s: core::TerminalStatus) {
        self.0.status_changed(s.into())
    }
    fn clipboard(&self, t: String) {
        self.0.clipboard(t)
    }
    fn size_changed(&self, c: u16, r: u16) {
        self.0.size_changed(c, r)
    }
}

// ---- app ----------------------------------------------------------------------

#[derive(uniffi::Object)]
pub struct ShrewApp {
    core: core::Shrew,
}

#[uniffi::export]
impl ShrewApp {
    /// `platform`: ios, android, linux, macos… `device_label`: shown to other clients.
    #[uniffi::constructor]
    pub fn new(data_dir: Option<String>, platform: String, device_label: Option<String>, listener: Arc<dyn AppListener>) -> Arc<Self> {
        let platform = if platform == "android" { "other".to_owned() } else { platform };
        let cfg = core::Config { data_dir: data_dir.map(PathBuf::from), platform, device_label, ..Default::default() };
        Arc::new(Self { core: core::Shrew::new(cfg, Arc::new(AppBridge(listener))) })
    }

    pub fn state(&self) -> AppState {
        self.core.state().into()
    }

    pub fn add_server(&self, name: String, address: String) -> Result<String> {
        Ok(self.core.add_server(&name, &address)?)
    }

    pub fn update_server(&self, server_id: String, name: String, address: String) -> Result<()> {
        Ok(self.core.update_server(&server_id, &name, &address)?)
    }

    pub fn remove_server(&self, server_id: String) {
        self.core.remove_server(&server_id)
    }

    pub fn move_server(&self, from: u32, to: u32) {
        self.core.move_server(from as usize, to as usize)
    }

    pub fn reconnect(&self, server_id: String) {
        self.core.reconnect(&server_id)
    }

    pub fn set_foreground(&self, foreground: bool) {
        self.core.set_foreground(foreground)
    }

    pub fn setting(&self, key: String) -> Option<String> {
        self.core.setting(&key).map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
    }

    pub fn set_setting(&self, key: String, value: String) {
        self.core.set_setting(&key, serde_json::Value::String(value))
    }

    /// Check an address before saving it. Returns the server's Rex version.
    pub async fn probe(&self, address: String) -> Result<String> {
        Ok(self.core.probe(&address).await?)
    }

    pub async fn create_session(&self, server_id: String, label: Option<String>) -> Result<String> {
        Ok(self.core.create_session(&server_id, label.as_deref(), None).await?.0)
    }

    pub async fn destroy_session(&self, server_id: String, session_id: String) -> Result<()> {
        Ok(self.core.destroy_session(&server_id, &session_id).await?)
    }

    pub async fn rename_session(&self, server_id: String, session_id: String, label: String) -> Result<()> {
        Ok(self.core.rename_session(&server_id, &session_id, &label).await?)
    }

    pub async fn new_window(&self, server_id: String, session_id: String) -> Result<()> {
        Ok(self.core.new_window(&server_id, &session_id, None).await?)
    }

    pub async fn close_window(&self, server_id: String, session_id: String, window_id: String) -> Result<()> {
        Ok(self.core.close_window(&server_id, &session_id, &window_id).await?)
    }

    pub async fn rename_window(&self, server_id: String, session_id: String, window_id: String, label: String) -> Result<()> {
        Ok(self.core.rename_window(&server_id, &session_id, &window_id, &label).await?)
    }

    pub async fn focus_window(&self, server_id: String, session_id: String, window_id: String) -> Result<()> {
        Ok(self.core.focus_window(&server_id, &session_id, &window_id).await?)
    }

    pub async fn focus_block(&self, server_id: String, session_id: String, block_id: String) -> Result<()> {
        Ok(self.core.focus_block(&server_id, &session_id, &block_id).await?)
    }

    pub async fn split(&self, server_id: String, session_id: String, block_id: String, vertical: bool) -> Result<Option<String>> {
        Ok(self.core.split(&server_id, &session_id, &block_id, vertical, None).await?)
    }

    pub async fn close_block(&self, server_id: String, session_id: String, block_id: String) -> Result<()> {
        Ok(self.core.close_block(&server_id, &session_id, &block_id).await?)
    }

    pub async fn zoom_block(&self, server_id: String, session_id: String, block_id: String) -> Result<()> {
        Ok(self.core.zoom_block(&server_id, &session_id, &block_id).await?)
    }

    pub async fn preview_text(&self, server_id: String, session_id: String, block_id: String) -> Result<String> {
        Ok(self.core.preview_text(&server_id, &session_id, &block_id, 64 * 1024).await?)
    }

    /// Answer an agent without opening its terminal, e.g. "1" or "y".
    pub async fn quick_reply(&self, server_id: String, session_id: String, block_id: String, text: String) -> Result<()> {
        Ok(self.core.send_text(&server_id, &session_id, &block_id, &text).await?)
    }

    pub fn open_terminal(
        &self,
        server_id: String,
        session_id: String,
        block_id: String,
        listener: Arc<dyn TerminalListener>,
    ) -> Result<Arc<Terminal>> {
        let t = self.core.open_terminal(&server_id, &session_id, &block_id, Arc::new(TermBridge(listener)))?;
        Ok(Arc::new(Terminal { inner: t }))
    }
}

// ---- terminal -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
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
    F { n: u8 },
    Char { c: String },
}

impl Key {
    fn into_core(self) -> Option<core::Key> {
        Some(match self {
            Key::Enter => core::Key::Enter,
            Key::Tab => core::Key::Tab,
            Key::Backspace => core::Key::Backspace,
            Key::Escape => core::Key::Escape,
            Key::Up => core::Key::Up,
            Key::Down => core::Key::Down,
            Key::Left => core::Key::Left,
            Key::Right => core::Key::Right,
            Key::Home => core::Key::Home,
            Key::End => core::Key::End,
            Key::PageUp => core::Key::PageUp,
            Key::PageDown => core::Key::PageDown,
            Key::Delete => core::Key::Delete,
            Key::Insert => core::Key::Insert,
            Key::F { n } => core::Key::F(n),
            Key::Char { c } => core::Key::Char(c.chars().next()?),
        })
    }
}

#[derive(uniffi::Object)]
pub struct Terminal {
    inner: Arc<core::TerminalSession>,
}

#[uniffi::export]
impl Terminal {
    pub fn session_id(&self) -> String {
        self.inner.session_id.clone()
    }

    pub fn block_id(&self) -> String {
        self.inner.block_id.clone()
    }

    pub fn status(&self) -> TerminalStatus {
        self.inner.status().into()
    }

    /// Call once per display refresh after `frame_ready`.
    pub fn frame(&self) -> Frame {
        self.inner.frame().into()
    }

    pub fn cols(&self) -> u16 {
        self.inner.size().0
    }

    pub fn rows(&self) -> u16 {
        self.inner.size().1
    }

    pub fn title(&self) -> Option<String> {
        self.inner.title()
    }

    pub fn send_text(&self, text: String) {
        self.inner.send_text(&text)
    }

    pub fn send_key(&self, key: Key, ctrl: bool, alt: bool, shift: bool, meta: bool) {
        if let Some(k) = key.into_core() {
            self.inner.send_key(k, ctrl, alt, shift, meta)
        }
    }

    pub fn paste(&self, text: String) {
        self.inner.paste(&text)
    }

    pub fn scroll(&self, rows: i32) {
        self.inner.scroll(rows)
    }

    pub fn scroll_to_bottom(&self) {
        self.inner.scroll_to_bottom()
    }

    pub fn jump_prompt(&self, delta: i32) {
        self.inner.jump_prompt(delta)
    }

    pub fn select_word(&self, col: u16, row: u16) -> bool {
        self.inner.select_word(col, row)
    }

    pub fn select_line(&self, row: u16) -> bool {
        self.inner.select_line(row)
    }

    pub fn select_range(&self, from_col: u16, from_row: u16, to_col: u16, to_row: u16) -> bool {
        self.inner.select_range((from_col, from_row), (to_col, to_row))
    }

    pub fn clear_selection(&self) {
        self.inner.clear_selection()
    }

    pub fn selection_text(&self) -> Option<String> {
        self.inner.selection_text()
    }

    pub fn all_text(&self) -> String {
        self.inner.all_text()
    }

    pub fn mouse_tracking(&self) -> bool {
        self.inner.mouse_tracking()
    }

    /// This device's grid, so "fit to screen" knows what size to ask for.
    pub fn set_device_grid(&self, cols: u16, rows: u16, cell_width_px: u32, cell_height_px: u32) {
        self.inner.set_device_grid(cols, rows, cell_width_px, cell_height_px)
    }

    pub fn has_control(&self) -> bool {
        self.inner.has_control()
    }

    pub fn take_control(&self) {
        self.inner.take_control()
    }

    pub fn release_control(&self) {
        self.inner.release_control()
    }

    /// Click a cell, for programs that track the mouse.
    pub fn tap(&self, col: u16, row: u16) {
        self.inner.click(col, row)
    }

    /// Force a color theme (0xAARRGGBB colors; `ansi` must have 16 entries).
    pub fn set_theme(&self, foreground: u32, background: u32, cursor: u32, ansi: Vec<u32>) {
        let c = |v: u32| core::Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8);
        let mut a = [core::Rgb::default(); 16];
        for (i, v) in ansi.iter().take(16).enumerate() {
            a[i] = c(*v);
        }
        self.inner.set_theme(Some(core::ColorTheme { foreground: c(foreground), background: c(background), cursor: c(cursor), ansi: a }));
    }

    /// Go back to the colors the server (or the program) sets.
    pub fn use_server_colors(&self) {
        self.inner.set_theme(None)
    }

    /// Wheel scroll for full-screen programs; positive is down.
    pub fn wheel(&self, col: u16, row: u16, lines: f32) {
        self.inner.wheel(col, row, lines)
    }

    /// True when the program uses the alternate screen (vim, less, htop):
    /// scroll gestures should go to the program, not local history.
    pub fn wants_wheel(&self) -> bool {
        self.inner.mouse_tracking()
    }

    /// Stop streaming and give back the size if we held it.
    pub fn shutdown(&self) {
        self.inner.close()
    }
}
