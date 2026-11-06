//! Raptor for Linux: GTK 4 + libadwaita, on the shared Rust engine.

mod dialogs;
mod fonts;
mod palette;
mod panes;
mod render;
mod sidebar;
mod terminal;
mod themes;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use raptor_core::{AgentState, AppState, Config, Connectivity, Listener, Notification, NotificationKind, Raptor, SessionState};

use panes::PaneGrid;
use terminal::{Prefs, TermView};

const APP_ID: &str = "dev.raptor.Raptor";

enum Event {
    State,
    Notify(Notification),
}

struct Bridge(async_channel::Sender<Event>);
impl Listener for Bridge {
    fn state_changed(&self) {
        let _ = self.0.try_send(Event::State);
    }
    fn notify(&self, n: Notification) {
        let _ = self.0.try_send(Event::Notify(n));
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Selection {
    pub server: String,
    pub session: String,
    pub window: Option<String>,
}

/// A tab: one of the session's windows.
struct Page {
    window: String,
    page: adw::TabPage,
    grid: PaneGrid,
}

pub struct Ui {
    pub core: Arc<Raptor>,
    pub window: adw::ApplicationWindow,
    sidebar: gtk::ListBox,
    sidebar_key: String,
    switcher: Switcher,
    tabs: adw::TabView,
    pages: Vec<Page>,
    stack: gtk::Stack,
    empty: adw::StatusPage,
    empty_button: gtk::Button,
    toasts: adw::ToastOverlay,
    /// Header buttons that only make sense with a session open.
    session_buttons: Vec<gtk::Widget>,
    pub selected: Option<Selection>,
    /// The server's active window when we last looked, to follow other clients' switches.
    server_active: Option<String>,
    /// A pane focused here that the server hasn't confirmed yet.
    pending_focus: Option<(String, Instant)>,
    /// Open terminals keyed by block id, reused across layout changes.
    pub terms: HashMap<String, TermView>,
    last_focused: Option<String>,
    pub prefs: Prefs,
    pub theme_id: String,
    theme_css: gtk::CssProvider,
    find_bar: gtk::SearchBar,
    find_entry: gtk::SearchEntry,
}

thread_local! {
    /// True while the UI changes the tab view itself, so its signals are ignored.
    static SYNCING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn syncing() -> bool {
    SYNCING.with(|s| s.get())
}

fn set_syncing(on: bool) {
    SYNCING.with(|s| s.set(on));
}

pub type Shared = Rc<RefCell<Ui>>;

/// The header's session button, Rex style: the session, its server, and a dot
/// when something elsewhere wants you. It opens the session list.
struct Switcher {
    button: gtk::MenuButton,
    title: gtk::Label,
    subtitle: gtk::Label,
    icon: gtk::Image,
    dot: gtk::Box,
}

impl Switcher {
    fn set(&self, title: &str, subtitle: &str, local: bool) {
        self.title.set_label(title);
        self.subtitle.set_label(subtitle);
        self.icon.set_icon_name(Some(if local { "computer-symbolic" } else { "network-server-symbolic" }));
    }
}

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::HANDLES_OPEN).build();
    app.connect_activate(|app| {
        if let Some(w) = app.active_window() {
            w.present();
        } else {
            build(app);
        }
    });
    app.connect_open(|app, _, _| app.activate());
    app.run_with_args::<&str>(&[])
}

fn data_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("RAPTOR_DATA_DIR") {
        return d.into();
    }
    let dir = glib::user_data_dir().join("raptor");
    // Keep the servers someone saved back when this app was called Shrew.
    let old = glib::user_data_dir().join("shrew/shrew.json");
    if !dir.exists() && old.exists() && std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::copy(&old, dir.join("raptor.json"));
    }
    dir
}

fn setting_str(core: &Raptor, key: &str) -> Option<String> {
    core.setting(key).and_then(|v| v.as_str().map(str::to_owned))
}

fn setting_f64(core: &Raptor, key: &str) -> Option<f64> {
    core.setting(key).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

fn setting_bool(core: &Raptor, key: &str, default: bool) -> bool {
    core.setting(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

pub const DEFAULT_FONT_SIZE: f64 = 14.0;

fn build(app: &adw::Application) {
    fonts::register();
    load_css();
    let (tx, rx) = async_channel::unbounded();
    let device = glib::host_name().to_string();
    let core = Arc::new(Raptor::new(
        Config { data_dir: Some(data_dir()), platform: "linux".into(), device_label: Some(device), ..Default::default() },
        Arc::new(Bridge(tx)),
    ));
    if core.state().servers.is_empty() {
        core.add_local_server_if_present();
    }
    if let Ok(addr) = std::env::var("RAPTOR_ADD_SERVER")
        && !core.state().servers.iter().any(|s| s.address == addr)
    {
        let _ = core.add_server("", &addr);
    }

    // ---- session switcher ----
    let sidebar = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).build();
    sidebar.add_css_class("navigation-sidebar");
    let side_scroll = gtk::ScrolledWindow::builder()
        .child(&sidebar)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(460)
        .build();
    let switch_search = gtk::SearchEntry::builder().placeholder_text("Switch to a session").build();
    let switch_new = gtk::Button::builder().label("New Session").action_name("app.new-session").build();
    let switch_add = gtk::Button::builder().label("Add Server…").action_name("app.add-server").build();
    for b in [&switch_new, &switch_add] {
        b.add_css_class("flat");
    }
    let switch_foot = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    switch_foot.append(&switch_new);
    switch_foot.append(&gtk::Box::builder().hexpand(true).build());
    switch_foot.append(&switch_add);
    let switch_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    switch_box.append(&switch_search);
    switch_box.append(&side_scroll);
    switch_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    switch_box.append(&switch_foot);
    switch_box.set_size_request(340, -1);
    let switch_pop = gtk::Popover::builder().child(&switch_box).build();
    switch_pop.add_css_class("raptor-switch-pop");
    let switch_icon = gtk::Image::from_icon_name("computer-symbolic");
    let switch_dot = dot("dot-idle");
    switch_dot.set_halign(gtk::Align::End);
    switch_dot.set_valign(gtk::Align::Start);
    switch_dot.set_visible(false);
    let icon_box = gtk::Overlay::builder().child(&switch_icon).build();
    icon_box.add_overlay(&switch_dot);
    let switch_title = label("Raptor", &["raptor-switch-title"]);
    let switch_sub = label("", &["caption", "dim-label"]);
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 0);
    titles.set_valign(gtk::Align::Center);
    titles.append(&switch_title);
    titles.append(&switch_sub);
    let switch_inner = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    switch_inner.append(&icon_box);
    switch_inner.append(&titles);
    switch_inner.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
    let switcher = gtk::MenuButton::builder().child(&switch_inner).popover(&switch_pop).tooltip_text("Switch Session (Ctrl+Shift+S)").build();
    switcher.add_css_class("flat");
    switcher.add_css_class("raptor-switcher");
    sidebar::install_filter(&sidebar, &switch_search, &switch_pop);
    let app_menu = gtk::MenuButton::builder().icon_name("open-menu-symbolic").menu_model(&app_menu()).tooltip_text("Main Menu").build();
    app_menu.set_primary(true);

    // ---- content ----
    let new_win = gtk::Button::from_icon_name("tab-new-symbolic");
    new_win.set_tooltip_text(Some("New Tab"));
    new_win.set_action_name(Some("app.new-window"));
    let split_h = gtk::Button::from_icon_name("view-dual-symbolic");
    split_h.set_tooltip_text(Some("Split Right"));
    split_h.set_action_name(Some("app.split-right"));
    let menu_btn = gtk::MenuButton::builder().icon_name("view-more-symbolic").tooltip_text("Session Menu").menu_model(&session_menu()).build();
    let palette_btn = gtk::Button::from_icon_name("system-search-symbolic");
    palette_btn.set_tooltip_text(Some("Commands"));
    palette_btn.set_action_name(Some("app.palette"));
    let content_header = adw::HeaderBar::new();
    content_header.pack_start(&switcher);
    content_header.pack_start(&new_win);
    content_header.pack_end(&app_menu);
    content_header.pack_end(&menu_btn);
    content_header.pack_end(&palette_btn);
    content_header.pack_end(&split_h);

    let tabs = adw::TabView::new();
    tabs.set_shortcuts(
        adw::TabViewShortcuts::CONTROL_PAGE_UP
            | adw::TabViewShortcuts::CONTROL_PAGE_DOWN
            | adw::TabViewShortcuts::CONTROL_SHIFT_PAGE_UP
            | adw::TabViewShortcuts::CONTROL_SHIFT_PAGE_DOWN
            | adw::TabViewShortcuts::ALT_DIGITS
            | adw::TabViewShortcuts::ALT_ZERO,
    );
    tabs.set_menu_model(Some(&tab_menu()));
    let tab_bar = adw::TabBar::builder().view(&tabs).autohide(true).hexpand(true).build();
    tab_bar.add_css_class("inline");
    content_header.set_title_widget(Some(&tab_bar));

    let empty_button = gtk::Button::builder().halign(gtk::Align::Center).build();
    empty_button.add_css_class("pill");
    empty_button.add_css_class("suggested-action");
    let empty = adw::StatusPage::builder().icon_name("utilities-terminal-symbolic").child(&empty_button).build();
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&tabs, Some("panes"));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&stack));
    let content_view = adw::ToolbarView::new();
    content_view.add_top_bar(&content_header);
    let find_entry = gtk::SearchEntry::builder().placeholder_text("Find in terminal").width_chars(28).build();
    let find_older = gtk::Button::from_icon_name("go-up-symbolic");
    find_older.set_tooltip_text(Some("Find Older (Enter)"));
    let find_newer = gtk::Button::from_icon_name("go-down-symbolic");
    find_newer.set_tooltip_text(Some("Find Newer (Shift+Enter)"));
    let find_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    find_box.append(&find_entry);
    find_box.append(&find_older);
    find_box.append(&find_newer);
    let find_bar = gtk::SearchBar::builder().child(&find_box).show_close_button(true).build();
    find_bar.connect_entry(&find_entry);
    content_view.add_top_bar(&find_bar);
    content_view.set_content(Some(&toasts));
    let (w, h) = (setting_f64(&core, "window_width").unwrap_or(1200.0), setting_f64(&core, "window_height").unwrap_or(780.0));
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Raptor")
        .default_width(w as i32)
        .default_height(h as i32)
        .width_request(360)
        .height_request(294)
        .content(&content_view)
        .build();
    if setting_bool(&core, "window_maximized", false) {
        window.maximize();
    }
    // Narrow windows keep the split and palette in the session menu.
    let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(adw::BreakpointConditionLengthType::MaxWidth, 560.0, adw::LengthUnit::Sp));
    bp.add_setter(&split_h, "visible", Some(&false.to_value()));
    bp.add_setter(&palette_btn, "visible", Some(&false.to_value()));
    window.add_breakpoint(bp);

    let theme_id = setting_str(&core, "theme").unwrap_or_else(|| "tokyo-night".into());
    let theme_css = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &theme_css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    let prefs = Prefs {
        family: setting_str(&core, "font_family").unwrap_or_else(|| render::DEFAULT_FAMILY.into()),
        size: setting_f64(&core, "font_size").unwrap_or(DEFAULT_FONT_SIZE),
        blink: setting_bool(&core, "cursor_blink", true),
        copy_on_select: setting_bool(&core, "copy_on_select", false),
    };
    let ui: Shared = Rc::new(RefCell::new(Ui {
        core: core.clone(),
        window: window.clone(),
        sidebar: sidebar.clone(),
        sidebar_key: String::new(),
        switcher: Switcher { button: switcher.clone(), title: switch_title, subtitle: switch_sub, icon: switch_icon, dot: switch_dot },
        tabs: tabs.clone(),
        pages: Vec::new(),
        stack,
        empty,
        empty_button: empty_button.clone(),
        toasts,
        session_buttons: vec![new_win.upcast(), split_h.upcast(), palette_btn.upcast(), menu_btn.upcast()],
        selected: None,
        server_active: None,
        pending_focus: None,
        terms: HashMap::new(),
        last_focused: None,
        prefs,
        theme_id: theme_id.clone(),
        theme_css,
        find_bar: find_bar.clone(),
        find_entry: find_entry.clone(),
    }));
    install_find(&ui, &find_entry, &find_older, &find_newer);
    apply_theme(&ui, &theme_id);

    let u = ui.clone();
    sidebar.connect_row_activated(move |_, row| {
        let name = row.widget_name();
        if let Some(rest) = name.strip_prefix("session|") {
            let mut parts = rest.splitn(3, '|');
            let (srv, ses, blk) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next());
            select(&u, srv.to_owned(), ses.to_owned(), blk.map(str::to_owned));
        }
    });
    let u = ui.clone();
    empty_button.connect_clicked(move |b| {
        let name = b.widget_name();
        if name == "add" {
            dialogs::server_dialog(&u, None);
        } else if let Some(srv) = name.strip_prefix("new|") {
            new_session(&u, srv.to_owned());
        }
    });
    wire_tabs(&ui);
    install_actions(app, &ui);

    // App shortcuts win over the terminal: dispatch them before the key reaches it.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (a, w) = (app.clone(), window.clone());
    keys.connect_key_pressed(move |_, kv, _, mods| {
        let m = mods & (gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK | gdk::ModifierType::ALT_MASK);
        if w.visible_dialog().is_some() || (m.is_empty() && kv != gdk::Key::F11) {
            return glib::Propagation::Proceed;
        }
        let accel = gtk::accelerator_name(kv.to_lower(), m);
        for name in a.actions_for_accel(&accel) {
            let Some(name) = name.strip_prefix("app.") else { continue };
            if a.is_action_enabled(name) && a.action_parameter_type(name).is_none() {
                a.activate_action(name, None);
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);

    let u = ui.clone();
    window.connect_close_request(move |w| {
        let core = u.borrow().core.clone();
        if !w.is_maximized() {
            let (w, h) = w.default_size();
            core.set_setting("window_width", (w as f64).into());
            core.set_setting("window_height", (h as f64).into());
        }
        core.set_setting("window_maximized", w.is_maximized().into());
        glib::Propagation::Proceed
    });
    let u = ui.clone();
    window.connect_is_active_notify(move |w| {
        // Coming back to the window counts as having seen what's on screen.
        if w.is_active()
            && let Some(sel) = u.borrow().selected.clone()
        {
            w.application().unwrap().withdraw_notification(&sel.session);
        }
    });

    // Engine events → UI. State changes come in bursts; redraw once per burst.
    let u = ui.clone();
    let app2 = app.clone();
    glib::spawn_future_local(async move {
        while let Ok(ev) = rx.recv().await {
            let mut dirty = matches!(ev, Event::State);
            let mut notes = Vec::new();
            if let Event::Notify(n) = ev {
                notes.push(n);
            }
            while let Ok(more) = rx.try_recv() {
                match more {
                    Event::State => dirty = true,
                    Event::Notify(n) => notes.push(n),
                }
            }
            if dirty {
                refresh(&u);
            }
            for n in notes {
                notify(&app2, &u, n);
            }
        }
    });

    watch_wake(&core);
    refresh(&ui);
    window.present();
    open_from_env(&ui);
    screenshot_mode(&ui);
}

/// After a suspend or a network change, connections may be dead without knowing it yet.
fn watch_wake(core: &Arc<Raptor>) {
    let c = core.clone();
    gio::NetworkMonitor::default().connect_network_changed(move |_, _| c.verify_connections());
    let c = core.clone();
    glib::spawn_future_local(async move {
        let Ok(bus) = gio::bus_get_future(gio::BusType::System).await else { return };
        let sub = bus.subscribe_to_signal(
            Some("org.freedesktop.login1"),
            Some("org.freedesktop.login1.Manager"),
            Some("PrepareForSleep"),
            Some("/org/freedesktop/login1"),
            None,
            gio::DBusSignalFlags::NONE,
            move |sig| {
                if sig.parameters.get::<(bool,)>().is_some_and(|(sleeping,)| !sleeping) {
                    c.verify_connections();
                }
            },
        );
        // Lasts for the life of the app.
        std::mem::forget(sub);
    });
}

fn load_css() {
    let css = gtk::CssProvider::new();
    css.load_from_string(
        r#"
        .raptor-pill { border-radius: 999px; padding: 6px 14px; font-size: 0.9em; font-weight: 600; box-shadow: 0 2px 8px alpha(black, 0.25); }
        .raptor-dot { min-width: 8px; min-height: 8px; border-radius: 99px; }
        .dot-online, .dot-done { background: #34d399; }
        .dot-connecting { background: #fbbf24; }
        .dot-offline, .dot-error { background: #f43f5e; }
        .dot-idle { background: #6b7280; }
        .dot-working { background: #38bdf8; }
        .dot-blocked { background: #f59e0b; }
        .raptor-section { font-size: 0.78em; font-weight: 700; letter-spacing: 0.04em; opacity: 0.6; margin: 14px 10px 4px 10px; }
        .raptor-server { font-weight: 700; }
        .raptor-mono { font-family: "JetBrains Mono", monospace; font-size: 0.85em; }
        .raptor-needs { background: alpha(#f59e0b, 0.10); border-radius: 10px; }
        .raptor-count { font-size: 0.8em; min-width: 18px; }
        .raptor-switcher > button { padding: 2px 8px; }
        .raptor-switch-title { font-weight: 700; }
        .raptor-switch-pop > contents { padding: 8px; }
        panegrid > .raptor-divider { background: transparent; }
        panegrid > .raptor-divider.dragging { background: alpha(@accent_bg_color, 0.5); }
        "#,
    );
    gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

pub fn dot(class: &str) -> gtk::Box {
    let d = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    d.add_css_class("raptor-dot");
    d.add_css_class(class);
    d.set_valign(gtk::Align::Center);
    d
}

pub fn agent_class(s: Option<AgentState>) -> &'static str {
    match s {
        Some(AgentState::Working) => "dot-working",
        Some(AgentState::Blocked) => "dot-blocked",
        Some(AgentState::Done) => "dot-done",
        Some(AgentState::Error) => "dot-error",
        _ => "dot-idle",
    }
}

pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::builder().label(text).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// Rebuild the sidebar and the content from the latest state.
pub fn refresh(ui: &Shared) {
    let state = ui.borrow().core.state();
    sidebar::rebuild(ui, &state);
    {
        let u = ui.borrow();
        let here = u.selected.as_ref().map(|s| s.session.clone());
        let elsewhere = state
            .attention()
            .into_iter()
            .filter(|a| Some(&a.session_id) != here.as_ref())
            .filter_map(|a| a.block.agent.as_ref().map(|x| x.state))
            .filter(|s| *s != AgentState::Idle)
            .max_by_key(|s| match s {
                AgentState::Blocked => 4,
                AgentState::Error => 3,
                AgentState::Working => 2,
                AgentState::Done => 1,
                AgentState::Idle => 0,
            });
        let d = &u.switcher.dot;
        d.set_visible(elsewhere.is_some());
        d.set_css_classes(&["raptor-dot", agent_class(elsewhere)]);
    }
    update_content(ui, &state);
}

pub fn toast(ui: &Shared, text: &str) {
    ui.borrow().toasts.add_toast(adw::Toast::new(text));
}

/// Run a server call and show a toast if it fails.
pub fn spawn_call<F>(ui: &Shared, what: &'static str, f: F)
where
    F: std::future::Future<Output = Result<(), raptor_core::CoreError>> + 'static,
{
    let u = ui.clone();
    glib::spawn_future_local(async move {
        if let Err(e) = f.await {
            toast(&u, &format!("Couldn't {what}: {e}"));
        }
    });
}

pub fn select(ui: &Shared, server: String, session: String, block: Option<String>) {
    {
        let mut u = ui.borrow_mut();
        let state = u.core.state();
        let ses = state.server(&server).and_then(|s| s.sessions.iter().find(|x| x.id == session));
        let window = block
            .as_ref()
            .and_then(|b| ses?.windows.iter().find(|w| w.blocks.iter().any(|x| &x.id == b)).map(|w| w.id.clone()));
        let same = u.selected.as_ref().is_some_and(|s| s.server == server && s.session == session);
        if !same {
            u.server_active = ses.and_then(|s| s.active_window_id.clone());
        }
        let window = window.or_else(|| if same { u.selected.as_ref().and_then(|s| s.window.clone()) } else { None });
        u.selected = Some(Selection { server: server.clone(), session: session.clone(), window });
        u.switcher.button.popdown();
        u.core.set_setting("last_session", format!("{server}|{session}").into());
        u.window.application().unwrap().withdraw_notification(&session);
    }
    if let Some(b) = block {
        let core = ui.borrow().core.clone();
        let (s2, b2) = (session.clone(), b.clone());
        spawn_call(ui, "focus that pane", async move { core.focus_block(&server, &s2, &b2).await });
        ui.borrow_mut().pending_focus = Some((b, Instant::now()));
    }
    refresh(ui);
}

pub fn new_session(ui: &Shared, server: String) {
    let core = ui.borrow().core.clone();
    let u = ui.clone();
    glib::spawn_future_local(async move {
        match core.create_session(&server, None, None).await {
            Ok((ses, _)) => select(&u, server, ses, None),
            Err(e) => toast(&u, &format!("Couldn't create a session: {e}")),
        }
    });
}

fn current_session(ui: &Shared, state: &AppState) -> Option<SessionState> {
    let sel = ui.borrow().selected.clone()?;
    state.server(&sel.server)?.sessions.iter().find(|x| x.id == sel.session).cloned()
}

fn show_empty(ui: &Shared, state: &AppState) {
    let mut u = ui.borrow_mut();
    u.stack.set_visible_child_name("empty");
    let pick = if state.servers.is_empty() { "No servers" } else { "Pick a session" };
    u.switcher.set(if state.servers.is_empty() { "Raptor" } else { "No Session" }, pick, true);
    u.window.set_title(Some("Raptor"));
    let online = state.servers.iter().find(|s| matches!(s.connectivity, Connectivity::Online));
    let (icon, title, desc, button) = if state.servers.is_empty() {
        (
            "network-server-symbolic",
            "Connect to a Rex Server",
            "Raptor shows the sessions on any Rex server you can reach: this computer, or a machine on your network or tailnet.",
            Some(("add".to_owned(), "Add Server…")),
        )
    } else if let Some(s) = online {
        let none = state.servers.iter().all(|s| s.sessions.is_empty());
        (
            "utilities-terminal-symbolic",
            if none { "No Sessions Yet" } else { "Pick a Session" },
            "Sessions keep running on the server, whether or not Raptor is open.",
            Some((format!("new|{}", s.id), "New Session")),
        )
    } else if state.servers.iter().any(|s| matches!(s.connectivity, Connectivity::Connecting | Connectivity::Idle)) {
        ("network-transmit-receive-symbolic", "Connecting…", "", None)
    } else {
        ("network-offline-symbolic", "Can't Reach Your Servers", "Raptor keeps trying in the background. Check that the Rex server is running and reachable.", None)
    };
    u.empty.set_icon_name(Some(icon));
    u.empty.set_title(title);
    u.empty.set_description(Some(desc));
    u.empty_button.set_visible(button.is_some());
    if let Some((name, text)) = button {
        u.empty_button.set_widget_name(&name);
        u.empty_button.set_label(text);
    }
    set_session_actions(&u.window, false);
    for b in &u.session_buttons {
        b.set_visible(false);
    }
    set_syncing(true);
    for p in std::mem::take(&mut u.pages) {
        u.tabs.close_page(&p.page);
    }
    set_syncing(false);
    for (_, t) in u.terms.drain() {
        t.shutdown();
    }
}

fn set_session_actions(window: &adw::ApplicationWindow, on: bool) {
    let Some(app) = window.application() else { return };
    for name in SESSION_ACTIONS {
        if let Some(a) = app.lookup_action(name).and_downcast::<gio::SimpleAction>() {
            a.set_enabled(on);
        }
    }
}

const SESSION_ACTIONS: &[&str] = &[
    "new-window",
    "split-right",
    "split-down",
    "close-pane",
    "zoom-pane",
    "next-pane",
    "prev-pane",
    "focus-left",
    "focus-right",
    "focus-up",
    "focus-down",
    "rename-session",
    "end-session",
    "palette",
    "copy",
    "paste",
];

fn window_title(w: &raptor_core::WindowState, i: usize) -> String {
    let default = w.label.is_empty() || w.label.starts_with("Window ") || w.label.parse::<u32>().is_ok();
    if !default {
        return w.label.clone();
    }
    let focused = w.blocks.iter().find(|b| Some(&b.id) == w.focused_block_id.as_ref()).or(w.blocks.first());
    focused.map(|b| b.display_name()).unwrap_or_else(|| format!("Window {}", i + 1))
}

fn update_content(ui: &Shared, state: &AppState) {
    let Some(session) = current_session(ui, state) else {
        let had = ui.borrow_mut().selected.take().is_some();
        if had {
            toast(ui, "That session ended");
        }
        show_empty(ui, state);
        return;
    };
    if session.windows.is_empty() {
        show_empty(ui, state);
        return;
    }
    let mut u = ui.borrow_mut();
    let mut sel = u.selected.clone().unwrap();
    u.stack.set_visible_child_name("panes");
    set_session_actions(&u.window, true);
    for b in &u.session_buttons {
        b.set_visible(true);
    }

    // Follow the server when another client (or a shortcut) switches windows.
    if u.server_active != session.active_window_id {
        u.server_active = session.active_window_id.clone();
        sel.window = session.active_window_id.clone();
    }
    let win = sel
        .window
        .clone()
        .filter(|w| session.windows.iter().any(|x| &x.id == w))
        .or(session.active_window_id.clone())
        .filter(|w| session.windows.iter().any(|x| &x.id == w))
        .unwrap_or_else(|| session.windows[0].id.clone());
    sel.window = Some(win.clone());
    u.selected = Some(sel.clone());

    // Tabs: one per window, in the server's order.
    set_syncing(true);
    let gone: Vec<usize> = (0..u.pages.len()).rev().filter(|i| !session.windows.iter().any(|w| w.id == u.pages[*i].window)).collect();
    for i in gone {
        let p = u.pages.remove(i);
        u.tabs.close_page(&p.page);
    }
    for (i, w) in session.windows.iter().enumerate() {
        let page = match u.pages.iter().position(|p| p.window == w.id) {
            Some(at) => u.pages[at].page.clone(),
            None => {
                let grid = PaneGrid::default();
                let (core, srv, ses) = (u.core.clone(), sel.server.clone(), sel.session.clone());
                // One resize in flight at a time, always ending on the latest ratio.
                let pending: Rc<RefCell<Option<(raptor_core::SplitState, f64)>>> = Rc::default();
                let busy = Rc::new(std::cell::Cell::new(false));
                grid.connect_resize(move |s, ratio| {
                    pending.replace(Some((s.clone(), ratio)));
                    if busy.replace(true) {
                        return;
                    }
                    let (core, srv, ses, pending, busy) = (core.clone(), srv.clone(), ses.clone(), pending.clone(), busy.clone());
                    glib::spawn_future_local(async move {
                        while let Some((s, r)) = pending.take() {
                            let _ = core.resize_pane(&srv, &ses, &s.before, &s.after, s.horizontal, r).await;
                        }
                        busy.set(false);
                    });
                });
                let page = u.tabs.insert(&grid, i as i32);
                u.pages.push(Page { window: w.id.clone(), page: page.clone(), grid });
                page
            }
        };
        if u.tabs.page_position(&page) != i as i32 {
            u.tabs.reorder_page(&page, i as i32);
        }
        page.set_title(&window_title(w, i));
        let focused = w.blocks.iter().find(|b| Some(&b.id) == w.focused_block_id.as_ref());
        page.set_tooltip(&focused.map(|b| raptor_core::short_path(&b.pwd)).unwrap_or_default());
        let agent = w.blocks.iter().filter_map(|b| b.agent.as_ref().map(|a| a.state)).max();
        page.set_loading(agent == Some(AgentState::Working));
        page.set_needs_attention(matches!(agent, Some(AgentState::Blocked | AgentState::Error)) || w.blocks.iter().any(|b| b.has_bell));
        page.set_indicator_icon(match agent {
            Some(AgentState::Blocked) => Some(gio::ThemedIcon::new("dialog-question-symbolic").upcast::<gio::Icon>()),
            Some(AgentState::Error) => Some(gio::ThemedIcon::new("dialog-error-symbolic").upcast()),
            Some(AgentState::Done) => Some(gio::ThemedIcon::new("object-select-symbolic").upcast()),
            _ => None,
        }
        .as_ref());
    }
    u.pages.sort_by_key(|p| session.windows.iter().position(|w| w.id == p.window));
    if let Some(p) = u.pages.iter().find(|p| p.window == win)
        && u.tabs.selected_page().as_ref() != Some(&p.page)
    {
        u.tabs.set_selected_page(&p.page);
    }
    set_syncing(false);

    // Terminals for every pane of every window, so switching tabs is instant.
    let keep: std::collections::HashSet<&str> = session.windows.iter().flat_map(|w| w.blocks.iter().map(|b| b.id.as_str())).collect();
    let stale: Vec<String> = u.terms.keys().filter(|k| !keep.contains(k.as_str())).cloned().collect();
    for k in stale {
        if let Some(t) = u.terms.remove(&k) {
            t.root.unparent();
            t.shutdown();
        }
    }
    let theme = themes::by_id(&u.theme_id).color_theme();
    let mut fresh = Vec::new();
    for w in &session.windows {
        for b in &w.blocks {
            if u.terms.contains_key(&b.id) {
                continue;
            }
            let t = TermView::new(&u.core, &sel.server, &sel.session, &b.id, b.owner.is_none() || b.owned_by_me, u.prefs.clone());
            t.set_theme(theme);
            let (uu, bid) = (ui.clone(), b.id.clone());
            t.connect_focus(move || {
                let (uu, bid) = (uu.clone(), bid.clone());
                glib::idle_add_local_once(move || pane_focused(&uu, &bid));
            });
            u.terms.insert(b.id.clone(), t);
            fresh.push(b.id.clone());
        }
    }
    for w in &session.windows {
        let Some(page) = u.pages.iter().find(|p| p.window == w.id) else { continue };
        let zoomed = w.zoomed_block_id.as_ref().filter(|z| w.blocks.iter().any(|b| &b.id == *z));
        let panes = w
            .blocks
            .iter()
            .filter(|b| zoomed.is_none_or(|z| *z == b.id))
            .filter_map(|b| {
                let rect = if zoomed.is_some() { [0.0, 0.0, 1.0, 1.0] } else { b.rect };
                u.terms.get(&b.id).map(|t| (t.root.clone().upcast::<gtk::Widget>(), rect))
            })
            .collect::<Vec<_>>();
        let multi = panes.len() > 1;
        page.grid.set_panes(panes);
        page.grid.set_splits(if zoomed.is_some() { Vec::new() } else { w.splits.clone() });
        if multi { page.grid.add_css_class("multi") } else { page.grid.remove_css_class("multi") }
    }

    // Titles.
    let window = session.windows.iter().find(|w| w.id == win).unwrap();
    let focused = window.blocks.iter().find(|b| Some(&b.id) == window.focused_block_id.as_ref()).or(window.blocks.first());
    let sub = focused
        .map(|b| if b.pwd.is_empty() { b.display_name() } else { format!("{} · {}", b.display_name(), raptor_core::short_path(&b.pwd)) })
        .unwrap_or_default();
    let srv = state.server(&sel.server);
    let server_name = srv.map(|s| if s.is_local { "This Computer".to_owned() } else { s.name.clone() }).unwrap_or_default();
    u.switcher.set(&session.label, &server_name, srv.is_none_or(|s| s.is_local));
    u.switcher.button.set_tooltip_text(Some(&format!("{sub}\nSwitch Session (Ctrl+Shift+S)")));
    u.window.set_title(Some(&format!("{} – Raptor", session.label)));

    // Keyboard focus follows the server's focused pane, unless we just moved it ourselves.
    if u.pending_focus.as_ref().is_some_and(|(b, t)| Some(b) == window.focused_block_id.as_ref() || t.elapsed() > Duration::from_secs(2)) {
        u.pending_focus = None;
    }
    let target = window.focused_block_id.clone().or(focused.map(|b| b.id.clone()));
    let want = target.and_then(|b| u.terms.get(&b).cloned());
    let typing_here = u.terms.values().any(|t| t.has_focus());
    // Never take the keyboard from someone typing in a text field (find, rename).
    let other_focus = GtkWindowExt::focus(&u.window).is_some_and(|f| f.is::<gtk::Text>() || (!f.is::<gtk::DrawingArea>() && !fresh.is_empty()));
    drop(u);
    if let Some(t) = want
        && ui.borrow().pending_focus.is_none()
        && !t.has_focus()
        && (typing_here || !other_focus)
        && ui.borrow().window.is_active()
    {
        glib::idle_add_local_once(move || t.grab_focus());
    }
}

/// A pane took keyboard focus here (click, Ctrl+Tab); make it the server's focus too.
fn pane_focused(ui: &Shared, block: &str) {
    let mut u = ui.borrow_mut();
    u.last_focused = Some(block.to_owned());
    let Some(sel) = u.selected.clone() else { return };
    let state = u.core.state();
    // Focus can land here late, after the pane's session was switched away from.
    let Some(w) = state
        .server(&sel.server)
        .and_then(|s| s.sessions.iter().find(|x| x.id == sel.session))
        .and_then(|s| s.windows.iter().find(|w| w.blocks.iter().any(|b| b.id == block)))
    else {
        return;
    };
    if w.focused_block_id.as_deref() == Some(block) {
        return;
    }
    u.pending_focus = Some((block.to_owned(), Instant::now()));
    let (core, b) = (u.core.clone(), block.to_owned());
    drop(u);
    spawn_call(ui, "focus that pane", async move { core.focus_block(&sel.server, &sel.session, &b).await });
}

fn wire_tabs(ui: &Shared) {
    let tabs = ui.borrow().tabs.clone();
    let u = ui.clone();
    tabs.connect_selected_page_notify(move |tv| {
        if syncing() {
            return;
        }
        let Some(page) = tv.selected_page() else { return };
        let mut b = u.borrow_mut();
        let Some(win) = b.pages.iter().find(|p| p.page == page).map(|p| p.window.clone()) else { return };
        let Some(sel) = b.selected.as_mut() else { return };
        if sel.window.as_ref() == Some(&win) {
            return;
        }
        sel.window = Some(win.clone());
        let sel = sel.clone();
        let core = b.core.clone();
        // Showing a window locally shouldn't fight the follow-the-server logic.
        b.server_active = Some(win.clone());
        drop(b);
        spawn_call(&u, "switch windows", async move { core.focus_window(&sel.server, &sel.session, &win).await });
        refresh(&u);
        let t = {
            let b = u.borrow();
            let state = b.core.state();
            let s = current_session(&u, &state);
            s.and_then(|s| s.windows.into_iter().find(|w| Some(&w.id) == b.selected.as_ref().and_then(|x| x.window.as_ref())))
                .and_then(|w| w.focused_block_id.or(w.blocks.first().map(|x| x.id.clone())))
                .and_then(|id| b.terms.get(&id).cloned())
        };
        if let Some(t) = t {
            glib::idle_add_local_once(move || t.grab_focus());
        }
    });
    let u = ui.clone();
    tabs.connect_close_page(move |tv, page| {
        if syncing() {
            tv.close_page_finish(page, true);
            return glib::Propagation::Stop;
        }
        tv.close_page_finish(page, false);
        let b = u.borrow();
        if let (Some(sel), Some(win)) = (b.selected.clone(), b.pages.iter().find(|p| &p.page == page).map(|p| p.window.clone())) {
            let core = b.core.clone();
            drop(b);
            spawn_call(&u, "close the window", async move { core.close_window(&sel.server, &sel.session, &win).await });
        }
        glib::Propagation::Stop
    });
    let u = ui.clone();
    tabs.connect_page_reordered(move |tv, page, pos| {
        if syncing() {
            return;
        }
        let b = u.borrow();
        let id_of = |p: &adw::TabPage| b.pages.iter().find(|x| &x.page == p).map(|x| x.window.clone());
        let (Some(sel), Some(win)) = (b.selected.clone(), id_of(page)) else { return };
        let before = (pos + 1 < tv.n_pages()).then(|| id_of(&tv.nth_page(pos + 1))).flatten();
        let core = b.core.clone();
        drop(b);
        spawn_call(&u, "move the window", async move { core.move_window(&sel.server, &sel.session, &win, before.as_deref()).await });
    });
    // Remember which tab a tab's menu was opened on.
    let u = ui.clone();
    tabs.connect_setup_menu(move |_, page| {
        let win = page.and_then(|p| u.borrow().pages.iter().find(|x| &x.page == p).map(|x| x.window.clone()));
        if let Some(w) = win {
            u.borrow().tabs.set_widget_name(&w);
        }
    });
}

/// The window a tab's context menu targets, else the visible one.
fn menu_window(ui: &Shared) -> Option<String> {
    let u = ui.borrow();
    let name = u.tabs.widget_name().to_string();
    if u.pages.iter().any(|p| p.window == name) { Some(name) } else { u.selected.as_ref()?.window.clone() }
}

fn split_focused(ui: &Shared, vertical: bool) {
    let Some((sel, _, Some(block))) = current_target(ui) else { return };
    let core = ui.borrow().core.clone();
    spawn_call(ui, "split the pane", async move { core.split(&sel.server, &sel.session, &block, vertical, None).await.map(drop) });
}

fn app_menu() -> gio::Menu {
    let m = gio::Menu::new();
    let a = gio::Menu::new();
    a.append(Some("New Session"), Some("app.new-session"));
    a.append(Some("Add Server…"), Some("app.add-server"));
    m.append_section(None, &a);
    let th = gio::Menu::new();
    for t in themes::THEMES {
        th.append(Some(t.name), Some(&format!("app.theme::{}", t.id)));
    }
    m.append_submenu(Some("Theme"), &th);
    let b = gio::Menu::new();
    b.append(Some("Preferences"), Some("app.preferences"));
    b.append(Some("Keyboard Shortcuts"), Some("app.shortcuts"));
    b.append(Some("About Raptor"), Some("app.about"));
    m.append_section(None, &b);
    m
}

fn session_menu() -> gio::Menu {
    let m = gio::Menu::new();
    let a = gio::Menu::new();
    a.append(Some("New Tab"), Some("app.new-window"));
    a.append(Some("Split Right"), Some("app.split-right"));
    a.append(Some("Split Down"), Some("app.split-down"));
    a.append(Some("Zoom Pane"), Some("app.zoom-pane"));
    a.append(Some("Close Pane"), Some("app.close-pane"));
    m.append_section(None, &a);
    let v = gio::Menu::new();
    v.append(Some("Find…"), Some("app.find"));
    v.append(Some("Bigger Text"), Some("app.font-bigger"));
    v.append(Some("Smaller Text"), Some("app.font-smaller"));
    v.append(Some("Reset Text Size"), Some("app.font-reset"));
    m.append_section(None, &v);
    let s = gio::Menu::new();
    s.append(Some("Rename Session…"), Some("app.rename-session"));
    s.append(Some("End Session…"), Some("app.end-session"));
    m.append_section(None, &s);
    m
}

fn tab_menu() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some("Rename Tab…"), Some("app.rename-window"));
    m.append(Some("Close Tab"), Some("app.close-window"));
    m
}

fn install_actions(app: &adw::Application, ui: &Shared) {
    let add = |name: &str, accels: &[&str], f: Box<dyn Fn(&Shared)>| {
        let a = gio::SimpleAction::new(name, None);
        let u = ui.clone();
        a.connect_activate(move |_, _| f(&u));
        app.add_action(&a);
        if !accels.is_empty() {
            app.set_accels_for_action(&format!("app.{name}"), accels);
        }
    };
    add("add-server", &[], Box::new(|u| dialogs::server_dialog(u, None)));
    add("switch-session", &["<Control><Shift>s"], Box::new(|u| u.borrow().switcher.button.popup()));
    add(
        "new-session",
        &["<Control><Shift>n"],
        Box::new(|u| {
            let b = u.borrow();
            let state = b.core.state();
            let srv = b
                .selected
                .as_ref()
                .map(|s| s.server.clone())
                .or_else(|| state.servers.iter().find(|s| matches!(s.connectivity, Connectivity::Online)).map(|s| s.id.clone()));
            drop(b);
            match srv {
                Some(s) => new_session(u, s),
                None => toast(u, "No server is online"),
            }
        }),
    );
    add(
        "new-window",
        &["<Control><Shift>t"],
        Box::new(|u| {
            let Some(sel) = u.borrow().selected.clone() else { return };
            let core = u.borrow().core.clone();
            spawn_call(u, "open a tab", async move { core.new_window(&sel.server, &sel.session, None).await });
        }),
    );
    add("split-right", &["<Control><Shift>o", "<Control><Shift>r"], Box::new(|u| split_focused(u, false)));
    add("split-down", &["<Control><Shift>e", "<Control><Shift>d"], Box::new(|u| split_focused(u, true)));
    add("close-pane", &["<Control><Shift>w"], Box::new(close_focused));
    add(
        "zoom-pane",
        &["<Control><Shift>Return"],
        Box::new(|u| {
            let Some((sel, _, Some(block))) = current_target(u) else { return };
            let core = u.borrow().core.clone();
            spawn_call(u, "zoom the pane", async move { core.zoom_block(&sel.server, &sel.session, &block).await });
        }),
    );
    add("next-pane", &["<Control>Tab"], Box::new(|u| cycle_pane(u, 1)));
    add("prev-pane", &["<Control><Shift>Tab", "<Control><Shift>ISO_Left_Tab"], Box::new(|u| cycle_pane(u, -1)));
    add("focus-left", &["<Control><Alt>Left"], Box::new(|u| focus_direction(u, -1, 0)));
    add("focus-right", &["<Control><Alt>Right"], Box::new(|u| focus_direction(u, 1, 0)));
    add("focus-up", &["<Control><Alt>Up"], Box::new(|u| focus_direction(u, 0, -1)));
    add("focus-down", &["<Control><Alt>Down"], Box::new(|u| focus_direction(u, 0, 1)));
    add("copy", &["<Control><Shift>c"], Box::new(|u| if let Some(t) = focused_term(u) { t.copy() }));
    add("paste", &["<Control><Shift>v"], Box::new(|u| if let Some(t) = focused_term(u) { t.paste() }));
    add("font-bigger", &["<Control>equal", "<Control>plus", "<Control><Shift>plus", "<Control>KP_Add"], Box::new(|u| set_font(u, Some(1.0))));
    add("font-smaller", &["<Control>minus", "<Control>KP_Subtract"], Box::new(|u| set_font(u, Some(-1.0))));
    add("font-reset", &["<Control>0", "<Control>KP_0"], Box::new(|u| set_font(u, None)));
    add("rename-session", &[], Box::new(dialogs::rename_session));
    add("end-session", &[], Box::new(dialogs::end_session));
    add("rename-window", &[], Box::new(|u| if let Some(w) = menu_window(u) { dialogs::rename_window(u, w) }));
    add(
        "close-window",
        &[],
        Box::new(|u| {
            let (Some(sel), Some(win)) = (u.borrow().selected.clone(), menu_window(u)) else { return };
            let core = u.borrow().core.clone();
            spawn_call(u, "close the tab", async move { core.close_window(&sel.server, &sel.session, &win).await });
        }),
    );
    add("palette", &["<Control><Shift>p"], Box::new(palette::open));
    add(
        "find",
        &["<Control><Shift>f"],
        Box::new(|u| {
            let (bar, entry) = (u.borrow().find_bar.clone(), u.borrow().find_entry.clone());
            bar.set_search_mode(true);
            entry.grab_focus();
            entry.select_region(0, -1);
        }),
    );
    add("preferences", &["<Control>comma"], Box::new(dialogs::preferences));
    add("shortcuts", &["<Control>question", "<Control><Shift>question"], Box::new(dialogs::shortcuts));
    add("about", &[], Box::new(dialogs::about));
    add(
        "fullscreen",
        &["F11"],
        Box::new(|u| {
            let w = u.borrow().window.clone();
            w.set_fullscreened(!w.is_fullscreen());
        }),
    );
    add("quit", &["<Control><Shift>q"], Box::new(|u| u.borrow().window.close()));

    let open = gio::SimpleAction::new("open-block", Some(glib::VariantTy::STRING));
    let u = ui.clone();
    open.connect_activate(move |_, v| {
        let Some(s) = v.and_then(|v| v.get::<String>()) else { return };
        let mut parts = s.splitn(3, '|');
        let (Some(srv), Some(ses), blk) = (parts.next(), parts.next(), parts.next()) else { return };
        u.borrow().window.present();
        select(&u, srv.to_owned(), ses.to_owned(), blk.filter(|b| !b.is_empty()).map(str::to_owned));
    });
    app.add_action(&open);

    let current = ui.borrow().theme_id.clone();
    let theme = gio::SimpleAction::new_stateful("theme", Some(glib::VariantTy::STRING), &current.to_variant());
    let u = ui.clone();
    theme.connect_activate(move |a, v| {
        if let Some(id) = v.and_then(|v| v.get::<String>()) {
            a.set_state(&id.to_variant());
            apply_theme(&u, &id);
        }
    });
    app.add_action(&theme);
    set_session_actions(&ui.borrow().window, false);
}

fn install_find(ui: &Shared, entry: &gtk::SearchEntry, older: &gtk::Button, newer: &gtk::Button) {
    let step = |ui: &Shared, older: bool| {
        let entry = ui.borrow().find_entry.clone();
        let Some(s) = focused_term(ui).and_then(|t| t.session()) else { return };
        let found = s.find(&entry.text(), older);
        if found || entry.text().is_empty() { entry.remove_css_class("error") } else { entry.add_css_class("error") }
    };
    let u = ui.clone();
    entry.connect_search_changed(move |e| {
        if let Some(s) = focused_term(&u).and_then(|t| t.session()) {
            s.reset_find();
        }
        e.remove_css_class("error");
        if !e.text().is_empty() {
            step(&u, true);
        }
    });
    let u = ui.clone();
    entry.connect_activate(move |_| step(&u, true));
    let u = ui.clone();
    entry.connect_previous_match(move |_| step(&u, true));
    let u = ui.clone();
    entry.connect_next_match(move |_| step(&u, false));
    let u = ui.clone();
    older.connect_clicked(move |_| step(&u, true));
    let u = ui.clone();
    newer.connect_clicked(move |_| step(&u, false));
    // Shift+Enter goes the other way.
    let keys = gtk::EventControllerKey::new();
    let u = ui.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        if mods.contains(gdk::ModifierType::SHIFT_MASK) && matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
            step(&u, false);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    entry.add_controller(keys);
    let u = ui.clone();
    ui.borrow().find_bar.connect_search_mode_enabled_notify(move |bar| {
        if bar.is_search_mode() {
            return;
        }
        if let Some(t) = focused_term(&u) {
            if let Some(s) = t.session() {
                s.reset_find();
            }
            t.grab_focus();
        }
    });
}

fn focused_term(ui: &Shared) -> Option<TermView> {
    let shown = current_target(ui).and_then(|(_, _, b)| b);
    let u = ui.borrow();
    u.terms
        .values()
        .find(|t| t.has_focus())
        .or_else(|| u.last_focused.as_ref().and_then(|b| u.terms.get(b)))
        .or_else(|| shown.and_then(|b| u.terms.get(&b)))
        .cloned()
}

pub fn set_font(ui: &Shared, delta: Option<f64>) {
    let mut u = ui.borrow_mut();
    u.prefs.size = delta.map(|d| (u.prefs.size + d).clamp(7.0, 40.0)).unwrap_or(DEFAULT_FONT_SIZE);
    let size = u.prefs.size;
    u.core.set_setting("font_size", size.into());
    apply_prefs(&u);
}

pub fn apply_prefs(u: &Ui) {
    for t in u.terms.values() {
        t.set_prefs(&u.prefs);
    }
}

pub fn apply_theme(ui: &Shared, id: &str) {
    let mut u = ui.borrow_mut();
    let t = themes::by_id(id);
    u.theme_id = t.id.to_owned();
    u.theme_css.load_from_string(&t.css());
    adw::StyleManager::default().set_color_scheme(if t.is_light() { adw::ColorScheme::ForceLight } else { adw::ColorScheme::ForceDark });
    u.core.set_setting("theme", t.id.into());
    for v in u.terms.values() {
        v.set_theme(t.color_theme());
    }
    if let Some(a) = u.window.application().and_then(|a| a.lookup_action("theme")).and_downcast::<gio::SimpleAction>() {
        a.set_state(&t.id.to_variant());
    }
}

fn notify(app: &adw::Application, ui: &Shared, n: Notification) {
    let u = ui.borrow();
    let looking = u.window.is_active() && u.selected.as_ref().is_some_and(|s| s.server == n.server_id && s.session == n.session_id);
    if looking || n.kind == NotificationKind::Bell || !setting_bool(&u.core, "notifications", true) {
        return;
    }
    let title = match n.kind {
        NotificationKind::AgentBlocked => format!("{} needs you", n.title),
        NotificationKind::AgentDone => format!("{} finished", n.title),
        NotificationKind::AgentError => format!("{} hit an error", n.title),
        _ => n.title.clone(),
    };
    let g = gio::Notification::new(&title);
    g.set_body(Some(&n.body));
    g.set_icon(&gio::ThemedIcon::new(APP_ID));
    if n.kind == NotificationKind::AgentBlocked {
        g.set_priority(gio::NotificationPriority::High);
    }
    let target = format!("{}|{}|{}", n.server_id, n.session_id, n.block_id);
    g.set_default_action_and_target_value("app.open-block", Some(&target.to_variant()));
    g.add_button_with_target_value("Open", "app.open-block", Some(&target.to_variant()));
    app.send_notification(Some(&n.session_id), &g);
}

/// RAPTOR_OPEN=<session label>: open that session as soon as it shows up (automation,
/// launchers). Without it, reopen the session that was open last time.
fn open_from_env(ui: &Shared) {
    let want = std::env::var("RAPTOR_OPEN").ok();
    let last = setting_str(&ui.borrow().core, "last_session");
    if want.is_none() && last.is_none() {
        return;
    }
    let u = ui.clone();
    let tries = Rc::new(RefCell::new(0));
    glib::timeout_add_local(Duration::from_millis(200), move || {
        *tries.borrow_mut() += 1;
        if u.borrow().selected.is_some() {
            return glib::ControlFlow::Break;
        }
        let state = u.borrow().core.state();
        let hit = state.servers.iter().find_map(|s| {
            s.sessions
                .iter()
                .find(|x| match &want {
                    Some(w) => &x.label == w || &x.id == w,
                    None => last.as_deref() == Some(&format!("{}|{}", s.id, x.id)),
                })
                .map(|x| (s.id.clone(), x.id.clone()))
        });
        match hit {
            Some((srv, ses)) => {
                select(&u, srv, ses, None);
                glib::ControlFlow::Break
            }
            None if *tries.borrow() > 50 => glib::ControlFlow::Break,
            None => glib::ControlFlow::Continue,
        }
    });
}

/// RAPTOR_SCREENSHOT=out.png: open the first session, save the window to a PNG, quit.
/// Used for headless checks (e.g. GDK_BACKEND=broadway) so nothing pops up on a desktop.
fn screenshot_mode(ui: &Shared) {
    let Ok(path) = std::env::var("RAPTOR_SCREENSHOT") else { return };
    let u = ui.clone();
    glib::timeout_add_local_once(Duration::from_secs(4), move || {
        let state = u.borrow().core.state();
        let pick = std::env::var("RAPTOR_SCREENSHOT_SESSION").ok();
        if let Some((srv, ses)) = state
            .servers
            .iter()
            .flat_map(|s| s.sessions.iter().map(move |x| (s, x)))
            .find(|(_, x)| pick.as_ref().is_none_or(|p| &x.label == p))
        {
            select(&u, srv.id.clone(), ses.id.clone(), None);
        }
        let u2 = u.clone();
        glib::timeout_add_local_once(Duration::from_secs(4), move || {
            let win = u2.borrow().window.clone();
            let paintable = gtk::WidgetPaintable::new(Some(&win));
            let (w, h) = (win.width(), win.height());
            let snap = gtk::Snapshot::new();
            paintable.snapshot(&snap, w as f64, h as f64);
            if let (Some(node), Some(native)) = (snap.to_node(), win.native()) {
                let tex = native.renderer().unwrap().render_texture(&node, None);
                let _ = tex.save_to_png(&path);
                println!("wrote {path}");
            }
            win.application().unwrap().quit();
        });
    });
}

/// The selected session's visible window and the block to act on: the pane with
/// keyboard focus, else the window's focused block.
pub fn current_target(ui: &Shared) -> Option<(Selection, Option<raptor_core::WindowState>, Option<String>)> {
    let u = ui.borrow();
    let sel = u.selected.clone()?;
    let state = u.core.state();
    let s = state.server(&sel.server)?.sessions.iter().find(|x| x.id == sel.session)?.clone();
    let w = s
        .windows
        .iter()
        .find(|w| Some(&w.id) == sel.window.as_ref().or(s.active_window_id.as_ref()))
        .or(s.windows.first())
        .cloned();
    let typing = u.terms.iter().find(|(_, t)| t.has_focus()).map(|(id, _)| id.clone());
    let block = typing
        .filter(|b| w.as_ref().is_some_and(|w| w.blocks.iter().any(|x| &x.id == b)))
        .or_else(|| w.as_ref().and_then(|w| w.focused_block_id.clone().or(w.blocks.first().map(|b| b.id.clone()))));
    Some((sel, w, block))
}

/// Panes of the visible window, as (block, rect) in layout order.
fn visible_panes(ui: &Shared) -> (Option<String>, Vec<(String, [f64; 4])>) {
    let Some((_, w, block)) = current_target(ui) else { return (None, Vec::new()) };
    let Some(w) = w else { return (block, Vec::new()) };
    if let Some(z) = &w.zoomed_block_id {
        return (block, vec![(z.clone(), [0.0, 0.0, 1.0, 1.0])]);
    }
    (block, w.blocks.iter().map(|b| (b.id.clone(), b.rect)).collect())
}

fn focus_pane(ui: &Shared, id: &str) {
    let t = ui.borrow().terms.get(id).cloned();
    if let Some(t) = t {
        t.grab_focus();
    }
}

fn cycle_pane(ui: &Shared, dir: i32) {
    let (block, panes) = visible_panes(ui);
    if panes.len() < 2 {
        return;
    }
    let i = block.and_then(|b| panes.iter().position(|(x, _)| *x == b)).unwrap_or(0) as i32;
    let next = panes[((i + dir).rem_euclid(panes.len() as i32)) as usize].0.clone();
    focus_pane(ui, &next);
}

/// Focus the nearest pane in a direction, by geometry.
fn focus_direction(ui: &Shared, dx: i32, dy: i32) {
    let (Some(block), panes) = visible_panes(ui) else { return };
    let Some(&(_, r)) = panes.iter().find(|(b, _)| *b == block) else { return };
    let (cx, cy) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    let best = panes
        .iter()
        .filter(|(b, _)| *b != block)
        .filter(|(_, o)| match (dx, dy) {
            (1, _) => o[0] >= r[0] + r[2] - 0.001,
            (-1, _) => o[0] + o[2] <= r[0] + 0.001,
            (_, 1) => o[1] >= r[1] + r[3] - 0.001,
            _ => o[1] + o[3] <= r[1] + 0.001,
        })
        .min_by(|(_, a), (_, b)| {
            let d = |o: &[f64; 4]| ((o[0] + o[2] / 2.0 - cx).powi(2) + (o[1] + o[3] / 2.0 - cy).powi(2)).sqrt();
            d(a).total_cmp(&d(b))
        });
    if let Some((b, _)) = best {
        focus_pane(ui, b);
    }
}

fn close_focused(ui: &Shared) {
    let Some((sel, _, Some(block))) = current_target(ui) else { return };
    let core = ui.borrow().core.clone();
    spawn_call(ui, "close the pane", async move { core.close_block(&sel.server, &sel.session, &block).await });
}
