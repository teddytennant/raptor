//! Shrew for Linux: GTK 4 + libadwaita, on the shared Rust engine.

mod render;
mod terminal;
mod themes;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use shrew_core::{AgentState, AppState, Config, Connectivity, Listener, Notification, NotificationKind, Shrew};

use terminal::TermView;

const APP_ID: &str = "dev.shrew.Shrew";

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
struct Selection {
    server: String,
    session: String,
    window: Option<String>,
}

struct Ui {
    core: Arc<Shrew>,
    window: adw::ApplicationWindow,
    sidebar: gtk::ListBox,
    split: adw::NavigationSplitView,
    content_title: adw::WindowTitle,
    tabs: gtk::Box,
    panes: gtk::Fixed,
    stack: gtk::Stack,
    selected: Option<Selection>,
    /// Open terminals keyed by block id, reused across layout changes.
    terms: HashMap<String, TermView>,
    last_layout: Vec<(String, [f64; 4])>,
    font_size: f64,
    theme_id: String,
    theme_css: gtk::CssProvider,
}

type Shared = Rc<RefCell<Ui>>;

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build);
    app.run_with_args::<&str>(&[])
}

fn data_dir() -> std::path::PathBuf {
    if let Ok(d) = std::env::var("SHREW_DATA_DIR") {
        return d.into();
    }
    glib::user_data_dir().join("shrew")
}

fn build(app: &adw::Application) {
    load_css();
    let (tx, rx) = async_channel::unbounded();
    let device = glib::host_name().to_string();
    let core = Arc::new(Shrew::new(
        Config { data_dir: Some(data_dir()), platform: "linux".into(), device_label: Some(device), ..Default::default() },
        Arc::new(Bridge(tx)),
    ));
    if core.state().servers.is_empty() {
        core.add_local_server_if_present();
    }
    if let Ok(addr) = std::env::var("SHREW_ADD_SERVER")
        && !core.state().servers.iter().any(|s| s.address == addr) {
            let _ = core.add_server("", &addr);
        }

    // ---- sidebar ----
    let sidebar = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).build();
    sidebar.add_css_class("navigation-sidebar");
    let side_scroll = gtk::ScrolledWindow::builder().child(&sidebar).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    let add_btn = gtk::Button::from_icon_name("list-add-symbolic");
    add_btn.set_tooltip_text(Some("Add server"));
    let side_header = adw::HeaderBar::new();
    side_header.set_title_widget(Some(&adw::WindowTitle::new("Shrew", "")));
    side_header.pack_end(&add_btn);
    let side_view = adw::ToolbarView::new();
    side_view.add_top_bar(&side_header);
    side_view.set_content(Some(&side_scroll));
    let side_page = adw::NavigationPage::builder().title("Shrew").child(&side_view).build();

    // ---- content ----
    let content_title = adw::WindowTitle::new("Shrew", "");
    let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    tabs.add_css_class("shrew-tabs");
    let new_win = gtk::Button::from_icon_name("tab-new-symbolic");
    new_win.set_tooltip_text(Some("New window"));
    let split_h = gtk::Button::from_icon_name("view-dual-symbolic");
    split_h.set_tooltip_text(Some("Split right"));
    let menu_btn = gtk::MenuButton::builder().icon_name("view-more-symbolic").build();
    let palette_btn = gtk::Button::from_icon_name("system-search-symbolic");
    palette_btn.set_tooltip_text(Some("Command palette (Ctrl+Shift+P)"));
    let content_header = adw::HeaderBar::new();
    content_header.set_title_widget(Some(&content_title));
    content_header.pack_start(&tabs);
    content_header.pack_end(&menu_btn);
    content_header.pack_end(&palette_btn);
    content_header.pack_end(&split_h);
    content_header.pack_end(&new_win);

    let panes = gtk::Fixed::new();
    panes.set_hexpand(true);
    panes.set_vexpand(true);
    panes.add_css_class("shrew-panes");
    let empty = adw::StatusPage::builder()
        .icon_name("utilities-terminal-symbolic")
        .title("Pick a session")
        .description("Your sessions keep running on the server, whether or not Shrew is open.")
        .build();
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&panes, Some("panes"));
    let content_view = adw::ToolbarView::new();
    content_view.add_top_bar(&content_header);
    content_view.set_content(Some(&stack));
    let content_page = adw::NavigationPage::builder().title("Session").child(&content_view).build();

    let split = adw::NavigationSplitView::builder().sidebar(&side_page).content(&content_page).min_sidebar_width(260.0).build();
    let window =
        adw::ApplicationWindow::builder().application(app).title("Shrew").default_width(1200).default_height(780).content(&split).build();

    let theme_id = core.setting("theme").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| "shrew".into());
    let theme_css = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &theme_css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    let font_size = core.setting("font_size").and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or(v.as_f64())).unwrap_or(12.0);
    let ui: Shared = Rc::new(RefCell::new(Ui {
        core: core.clone(),
        window: window.clone(),
        sidebar: sidebar.clone(),
        split,
        content_title,
        tabs,
        panes: panes.clone(),
        stack,
        selected: None,
        terms: HashMap::new(),
        last_layout: Vec::new(),
        font_size,
        theme_id: theme_id.clone(),
        theme_css,
    }));
    apply_theme(&ui, &theme_id);

    // Re-place panes whenever the content area changes size.
    let u = ui.clone();
    let last = Rc::new(RefCell::new((0, 0)));
    panes.add_tick_callback(move |w, _| {
        let size = (w.width(), w.height());
        if *last.borrow() != size {
            *last.borrow_mut() = size;
            place_panes(&u);
        }
        glib::ControlFlow::Continue
    });

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
    add_btn.connect_clicked(move |_| add_server_dialog(&u));
    let u = ui.clone();
    new_win.connect_clicked(move |_| {
        let ui = u.borrow();
        if let Some(sel) = ui.selected.clone() {
            let core = ui.core.clone();
            glib::spawn_future_local(async move {
                let _ = core.new_window(&sel.server, &sel.session, None).await;
            });
        }
    });
    let u = ui.clone();
    split_h.connect_clicked(move |_| split_focused(&u, false));
    menu_btn.set_menu_model(Some(&session_menu()));
    install_actions(app, &ui);
    let u = ui.clone();
    palette_btn.connect_clicked(move |_| open_palette(&u));

    // Window-level shortcuts, caught before the terminal sees the key.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let u = ui.clone();
    keys.connect_key_pressed(move |_, key, _, mods| {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        if !ctrl {
            return glib::Propagation::Proceed;
        }
        match key {
            k if shift && k.to_lower() == gdk::Key::p => open_palette(&u),
            gdk::Key::Tab => cycle_pane(&u, 1),
            gdk::Key::ISO_Left_Tab => cycle_pane(&u, -1),
            gdk::Key::Page_Down => cycle_window(&u, 1),
            gdk::Key::Page_Up => cycle_window(&u, -1),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    window.add_controller(keys);

    // Engine events → UI.
    let u = ui.clone();
    let app2 = app.clone();
    glib::spawn_future_local(async move {
        while let Ok(ev) = rx.recv().await {
            match ev {
                Event::State => refresh(&u),
                Event::Notify(n) => notify(&app2, &u, n),
            }
        }
    });

    refresh(&ui);
    window.present();
    open_from_env(&ui);
    screenshot_mode(&ui);
}

fn load_css() {
    let css = gtk::CssProvider::new();
    css.load_from_string(
        r#"
        .shrew-pill { border-radius: 999px; padding: 5px 12px; font-size: 0.9em; }
        .shrew-dot { min-width: 8px; min-height: 8px; border-radius: 99px; }
        .dot-online, .dot-done { background: #34d399; }
        .dot-connecting { background: #fbbf24; }
        .dot-offline, .dot-error { background: #f43f5e; }
        .dot-idle { background: #6b7280; }
        .dot-working { background: #38bdf8; }
        .dot-blocked { background: #f59e0b; }
        .shrew-section { font-size: 0.78em; font-weight: 700; letter-spacing: 0.04em; opacity: 0.6; margin: 14px 10px 4px 10px; }
        .shrew-server { font-weight: 700; }
        .shrew-mono { font-family: "JetBrains Mono", monospace; font-size: 0.85em; }
        .shrew-needs { background: alpha(#f59e0b, 0.10); border-radius: 10px; }
        .shrew-tabs button { padding: 2px 10px; }
        .shrew-tabs button.active { background: alpha(@accent_bg_color, 0.25); }
        "#,
    );
    gtk::style_context_add_provider_for_display(&gdk::Display::default().unwrap(), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

fn dot(class: &str) -> gtk::Box {
    let d = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    d.add_css_class("shrew-dot");
    d.add_css_class(class);
    d.set_valign(gtk::Align::Center);
    d
}

fn agent_class(s: Option<AgentState>) -> &'static str {
    match s {
        Some(AgentState::Working) => "dot-working",
        Some(AgentState::Blocked) => "dot-blocked",
        Some(AgentState::Done) => "dot-done",
        Some(AgentState::Error) => "dot-error",
        _ => "dot-idle",
    }
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::builder().label(text).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// Rebuild the sidebar and the content from the latest state.
fn refresh(ui: &Shared) {
    let state = ui.borrow().core.state();
    rebuild_sidebar(ui, &state);
    update_content(ui, &state);
}

fn rebuild_sidebar(ui: &Shared, state: &AppState) {
    let u = ui.borrow();
    let list = u.sidebar.clone();
    let selected = u.selected.clone();
    drop(u);
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    let section = |t: &str| {
        let r = gtk::ListBoxRow::builder().selectable(false).activatable(false).build();
        r.set_child(Some(&label(t, &["shrew-section"])));
        r
    };

    let needs: Vec<_> = state
        .attention()
        .into_iter()
        .filter(|a| a.block.agent.as_ref().is_some_and(|x| matches!(x.state, AgentState::Blocked | AgentState::Error)))
        .collect();
    if !needs.is_empty() {
        list.append(&section("NEEDS YOU"));
        for a in needs {
            let ag = a.block.agent.clone().unwrap();
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("session|{}|{}|{}", a.server_id, a.session_id, a.block.id));
            row.add_css_class("shrew-needs");
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            b.set_margin_top(6);
            b.set_margin_bottom(6);
            b.set_margin_start(6);
            b.append(&dot(agent_class(Some(ag.state))));
            let v = gtk::Box::new(gtk::Orientation::Vertical, 1);
            let who = if ag.app.is_empty() { a.block.display_name() } else { ag.app.clone() };
            v.append(&label(&who, &["heading"]));
            let msg = if ag.message.is_empty() { format!("{} · {}", a.server_name, a.session_label) } else { ag.message.clone() };
            v.append(&label(&msg, &["caption", "dim-label"]));
            b.append(&v);
            row.set_child(Some(&b));
            list.append(&row);
        }
    }

    for srv in &state.servers {
        let head = gtk::ListBoxRow::builder().selectable(false).activatable(false).build();
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        hb.set_margin_top(14);
        hb.set_margin_start(6);
        hb.set_margin_end(6);
        let (cls, status) = match &srv.connectivity {
            Connectivity::Online => ("dot-online", srv.latency_ms.map(|l| format!("Online · {l} ms")).unwrap_or_else(|| "Online".into())),
            Connectivity::Connecting => ("dot-connecting", "Connecting…".into()),
            Connectivity::Offline { error, .. } => ("dot-offline", error.clone()),
            Connectivity::Idle => ("dot-idle", "Paused".into()),
        };
        let v = gtk::Box::new(gtk::Orientation::Vertical, 0);
        v.set_hexpand(true);
        v.append(&label(&srv.name, &["shrew-server"]));
        let sb = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        sb.append(&dot(cls));
        sb.append(&label(&status, &["caption", "dim-label"]));
        v.append(&sb);
        hb.append(&v);
        if matches!(srv.connectivity, Connectivity::Online) {
            let nb = gtk::Button::from_icon_name("list-add-symbolic");
            nb.add_css_class("flat");
            nb.set_tooltip_text(Some("New session"));
            let (u, sid) = (ui.clone(), srv.id.clone());
            nb.connect_clicked(move |_| {
                let core = u.borrow().core.clone();
                let (u, sid) = (u.clone(), sid.clone());
                glib::spawn_future_local(async move {
                    if let Ok((ses, _)) = core.create_session(&sid, None, None).await {
                        select(&u, sid, ses, None);
                    }
                });
            });
            hb.append(&nb);
        } else if matches!(srv.connectivity, Connectivity::Offline { .. }) {
            let rb = gtk::Button::from_icon_name("view-refresh-symbolic");
            rb.add_css_class("flat");
            let (core, sid) = (ui.borrow().core.clone(), srv.id.clone());
            rb.connect_clicked(move |_| core.reconnect(&sid));
            hb.append(&rb);
        }
        head.set_child(Some(&hb));
        list.append(&head);

        for s in &srv.sessions {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("session|{}|{}", srv.id, s.id));
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            b.set_margin_top(6);
            b.set_margin_bottom(6);
            b.set_margin_start(8);
            let v = gtk::Box::new(gtk::Orientation::Vertical, 1);
            v.set_hexpand(true);
            v.append(&label(&s.label, &[]));
            let active = s.windows.iter().find(|w| Some(&w.id) == s.active_window_id.as_ref()).or(s.windows.first());
            let focused = active.and_then(|w| w.blocks.iter().find(|b| Some(&b.id) == w.focused_block_id.as_ref()).or(w.blocks.first()));
            let detail = match focused {
                Some(b) if !b.pwd.is_empty() => format!("{} · {}", b.display_name(), shrew_core::short_path(&b.pwd)),
                Some(b) => b.display_name(),
                None => String::new(),
            };
            v.append(&label(&detail, &["caption", "dim-label", "shrew-mono"]));
            b.append(&v);
            if let Some(st) = s.agent_state().filter(|s| *s != AgentState::Idle) {
                b.append(&dot(agent_class(Some(st))));
            }
            let count = gtk::Label::new(Some(&format!("{}", s.windows.len())));
            count.add_css_class("dim-label");
            count.add_css_class("caption");
            count.set_margin_end(6);
            b.append(&count);
            row.set_child(Some(&b));
            list.append(&row);
            if selected.as_ref().is_some_and(|x| x.server == srv.id && x.session == s.id) {
                list.select_row(Some(&row));
            }
        }
    }
}

fn select(ui: &Shared, server: String, session: String, block: Option<String>) {
    {
        let mut u = ui.borrow_mut();
        let state = u.core.state();
        let window = block.as_ref().and_then(|b| {
            state
                .server(&server)?
                .sessions
                .iter()
                .find(|s| s.id == session)?
                .windows
                .iter()
                .find(|w| w.blocks.iter().any(|x| &x.id == b))
                .map(|w| w.id.clone())
        });
        u.selected = Some(Selection { server: server.clone(), session: session.clone(), window });
        u.split.set_show_content(true);
    }
    if let Some(b) = block {
        let core = ui.borrow().core.clone();
        glib::spawn_future_local(async move {
            let _ = core.focus_block(&server, &session, &b).await;
        });
    }
    refresh(ui);
}

fn update_content(ui: &Shared, state: &AppState) {
    let mut u = ui.borrow_mut();
    let Some(sel) = u.selected.clone() else {
        u.stack.set_visible_child_name("empty");
        return;
    };
    let Some(session) = state.server(&sel.server).and_then(|s| s.sessions.iter().find(|x| x.id == sel.session)).cloned() else {
        u.selected = None;
        u.stack.set_visible_child_name("empty");
        return;
    };
    u.stack.set_visible_child_name("panes");
    let win_id = sel.window.clone().filter(|w| session.windows.iter().any(|x| &x.id == w)).or(session.active_window_id.clone());
    let window = session.windows.iter().find(|w| Some(&w.id) == win_id.as_ref()).or(session.windows.first()).cloned();
    let focused = window.as_ref().and_then(|w| w.blocks.iter().find(|b| Some(&b.id) == w.focused_block_id.as_ref()));
    u.content_title.set_title(&session.label);
    u.content_title.set_subtitle(
        &focused
            .map(|b| if b.pwd.is_empty() { b.display_name() } else { format!("{} · {}", b.display_name(), shrew_core::short_path(&b.pwd)) })
            .unwrap_or_default(),
    );

    // Window tabs.
    let tabs = u.tabs.clone();
    while let Some(c) = tabs.first_child() {
        tabs.remove(&c);
    }
    if session.windows.len() > 1 {
        for (i, w) in session.windows.iter().enumerate() {
            let name = if w.label.is_empty() || w.label.starts_with("Window ") { format!("{}", i + 1) } else { w.label.clone() };
            let b = gtk::Button::with_label(&name);
            b.add_css_class("flat");
            if Some(&w.id) == window.as_ref().map(|x| &x.id) {
                b.add_css_class("active");
            }
            let (ui2, wid) = (ui.clone(), w.id.clone());
            b.connect_clicked(move |_| {
                if let Some(s) = ui2.borrow_mut().selected.as_mut() {
                    s.window = Some(wid.clone());
                }
                refresh(&ui2);
            });
            tabs.append(&b);
        }
    }

    // Panes: reuse open terminals, open new ones, close removed ones.
    let blocks: Vec<_> = window
        .as_ref()
        .map(|w| {
            if let Some(z) = &w.zoomed_block_id {
                w.blocks.iter().filter(|b| &b.id == z).map(|b| (b.clone(), [0.0, 0.0, 1.0, 1.0])).collect()
            } else {
                w.blocks.iter().map(|b| (b.clone(), b.rect)).collect()
            }
        })
        .unwrap_or_default();
    let keep: std::collections::HashSet<String> = blocks.iter().map(|(b, _)| b.id.clone()).collect();
    let stale: Vec<String> = u.terms.keys().filter(|k| !keep.contains(*k)).cloned().collect();
    for k in stale {
        if let Some(t) = u.terms.remove(&k) {
            u.panes.remove(&t.root);
            t.shutdown();
        }
    }
    let single = blocks.len() == 1;
    for (b, _) in &blocks {
        if !u.terms.contains_key(&b.id) {
            let t = TermView::new(&u.core, &sel.server, &sel.session, &b.id, b.owner.is_none() || b.owned_by_me, u.font_size);
            t.set_theme(themes::by_id(&u.theme_id).color_theme());
            let (core, srv, ses, bid) = (u.core.clone(), sel.server.clone(), sel.session.clone(), b.id.clone());
            t.connect_focus(move || {
                // Clicking or cycling into a pane focuses it on the server too.
                let focused = core.state().server(&srv).and_then(|s| s.sessions.iter().find(|x| x.id == ses).cloned()).is_some_and(|s| {
                    s.windows.iter().any(|w| w.focused_block_id.as_deref() == Some(bid.as_str()))
                });
                if !focused {
                    let (core, srv, ses, bid) = (core.clone(), srv.clone(), ses.clone(), bid.clone());
                    glib::spawn_future_local(async move {
                        let _ = core.focus_block(&srv, &ses, &bid).await;
                    });
                }
            });
            u.panes.put(&t.root, 0.0, 0.0);
            if single || focused.is_some_and(|f| f.id == b.id) {
                let t2 = t.clone();
                glib::idle_add_local_once(move || t2.grab_focus());
            }
            u.terms.insert(b.id.clone(), t);
        }
    }
    u.last_layout = blocks.iter().map(|(b, r)| (b.id.clone(), *r)).collect();
    drop(u);
    place_panes(ui);
}

fn place_panes(ui: &Shared) {
    let u = ui.borrow();
    let (w, h) = (u.panes.width() as f64, u.panes.height() as f64);
    if w <= 0.0 {
        return;
    }
    let gap = 1.0;
    for (id, r) in &u.last_layout {
        if let Some(t) = u.terms.get(id) {
            let (x, y) = ((r[0] * w).round(), (r[1] * h).round());
            let (rw, rh) = (((r[0] + r[2]) * w).round() - x - gap, ((r[1] + r[3]) * h).round() - y - gap);
            u.panes.move_(&t.root, x, y);
            t.root.set_size_request(rw.max(20.0) as i32, rh.max(20.0) as i32);
        }
    }
}

fn split_focused(ui: &Shared, vertical: bool) {
    let u = ui.borrow();
    let Some(sel) = u.selected.clone() else { return };
    let state = u.core.state();
    let Some(s) = state.server(&sel.server).and_then(|s| s.sessions.iter().find(|x| x.id == sel.session)) else { return };
    let w = s.windows.iter().find(|w| Some(&w.id) == sel.window.as_ref().or(s.active_window_id.as_ref())).or(s.windows.first());
    let Some(block) = w.and_then(|w| w.focused_block_id.clone().or(w.blocks.first().map(|b| b.id.clone()))) else { return };
    let core = u.core.clone();
    glib::spawn_future_local(async move {
        let _ = core.split(&sel.server, &sel.session, &block, vertical, None).await;
    });
}

fn session_menu() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some("Command Palette"), Some("app.palette"));
    m.append(Some("Split Down"), Some("app.split-down"));
    m.append(Some("Close Pane"), Some("app.close-pane"));
    let th = gio::Menu::new();
    for t in themes::THEMES {
        th.append(Some(t.name), Some(&format!("app.theme::{}", t.id)));
    }
    m.append_submenu(Some("Theme"), &th);
    m.append(Some("Rename Session…"), Some("app.rename-session"));
    m.append(Some("Bigger Text"), Some("app.font-bigger"));
    m.append(Some("Smaller Text"), Some("app.font-smaller"));
    let danger = gio::Menu::new();
    danger.append(Some("End Session"), Some("app.end-session"));
    m.append_section(None, &danger);
    m
}

fn install_actions(app: &adw::Application, ui: &Shared) {
    let add = |name: &str, f: Box<dyn Fn(&Shared)>| {
        let a = gio::SimpleAction::new(name, None);
        let u = ui.clone();
        a.connect_activate(move |_, _| f(&u));
        app.add_action(&a);
    };
    add("split-down", Box::new(|u| split_focused(u, true)));
    add("font-bigger", Box::new(|u| set_font(u, 1.0)));
    add("font-smaller", Box::new(|u| set_font(u, -1.0)));
    add(
        "end-session",
        Box::new(|u| {
            let ui = u.borrow();
            if let Some(sel) = ui.selected.clone() {
                let core = ui.core.clone();
                glib::spawn_future_local(async move {
                    let _ = core.destroy_session(&sel.server, &sel.session).await;
                });
            }
        }),
    );
    add("rename-session", Box::new(rename_dialog));
    add("palette", Box::new(open_palette));
    add("close-pane", Box::new(close_focused));
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
    app.set_accels_for_action("app.split-down", &["<Control><Shift>d"]);
    app.set_accels_for_action("app.close-pane", &["<Control><Shift>w"]);
    app.set_accels_for_action("app.font-bigger", &["<Control>equal", "<Control>plus"]);
    app.set_accels_for_action("app.font-smaller", &["<Control>minus"]);
}

fn set_font(ui: &Shared, delta: f64) {
    let mut u = ui.borrow_mut();
    u.font_size = (u.font_size + delta).clamp(7.0, 32.0);
    let f = u.font_size;
    u.core.set_setting("font_size", f.to_string().into());
    for t in u.terms.values() {
        t.set_font_size(f);
    }
}

fn rename_dialog(ui: &Shared) {
    let u = ui.borrow();
    let Some(sel) = u.selected.clone() else { return };
    let current = u
        .core
        .state()
        .server(&sel.server)
        .and_then(|s| s.sessions.iter().find(|x| x.id == sel.session))
        .map(|s| s.label.clone())
        .unwrap_or_default();
    let entry = gtk::Entry::builder().text(&current).activates_default(true).build();
    let d = adw::AlertDialog::new(Some("Rename Session"), None);
    d.set_extra_child(Some(&entry));
    d.add_responses(&[("cancel", "Cancel"), ("rename", "Rename")]);
    d.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
    d.set_default_response(Some("rename"));
    let core = u.core.clone();
    d.connect_response(None, move |_, r| {
        if r == "rename" {
            let (core, sel, name) = (core.clone(), sel.clone(), entry.text().to_string());
            glib::spawn_future_local(async move {
                let _ = core.rename_session(&sel.server, &sel.session, &name).await;
            });
        }
    });
    d.present(Some(&u.window));
}

fn add_server_dialog(ui: &Shared) {
    let u = ui.borrow();
    let addr = gtk::Entry::builder().placeholder_text("studio.tail1234.ts.net or 100.64.0.2:7878").activates_default(true).build();
    addr.add_css_class("shrew-mono");
    let name = gtk::Entry::builder().placeholder_text("Name (optional)").activates_default(true).build();
    let status = gtk::Label::builder().label("We'll test the connection as you type.").xalign(0.0).wrap(true).build();
    status.add_css_class("dim-label");
    let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
    b.append(&addr);
    b.append(&name);
    b.append(&status);
    let d = adw::AlertDialog::new(Some("Add a Server"), Some("Anything you'd pass to rex -S."));
    d.set_extra_child(Some(&b));
    d.add_responses(&[("cancel", "Cancel"), ("add", "Add")]);
    d.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    d.set_default_response(Some("add"));

    // Debounced live probe.
    let generation = Rc::new(RefCell::new(0u64));
    let core = u.core.clone();
    let st = status.clone();
    addr.connect_changed(move |e| {
        let text = e.text().to_string();
        *generation.borrow_mut() += 1;
        let my = *generation.borrow();
        let (core, st, generation) = (core.clone(), st.clone(), generation.clone());
        glib::spawn_future_local(async move {
            glib::timeout_future(Duration::from_millis(500)).await;
            if *generation.borrow() != my || text.trim().is_empty() {
                return;
            }
            st.set_label("Connecting…");
            let r = core.probe(&text).await;
            if *generation.borrow() != my {
                return;
            }
            match r {
                Ok(v) => st.set_label(&format!("✓ Connected · Rex {v}")),
                Err(e) => st.set_label(&format!("✗ {e}")),
            }
        });
    });
    let core = u.core.clone();
    d.connect_response(None, move |_, r| {
        if r == "add" && !addr.text().trim().is_empty() {
            let _ = core.add_server(&name.text(), &addr.text());
        }
    });
    d.present(Some(&u.window));
}

fn notify(app: &adw::Application, ui: &Shared, n: Notification) {
    let u = ui.borrow();
    let looking = u.window.is_active() && u.selected.as_ref().is_some_and(|s| s.server == n.server_id && s.session == n.session_id);
    if looking || n.kind == NotificationKind::Bell {
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
    if n.kind == NotificationKind::AgentBlocked {
        g.set_priority(gio::NotificationPriority::High);
    }
    app.send_notification(Some(&format!("{}{}", n.server_id, n.block_id)), &g);
}

/// SHREW_OPEN=<session label>: open that session as soon as it shows up (automation, launchers).
fn open_from_env(ui: &Shared) {
    let Ok(want) = std::env::var("SHREW_OPEN") else { return };
    let u = ui.clone();
    let tries = Rc::new(RefCell::new(0));
    glib::timeout_add_local(Duration::from_millis(200), move || {
        *tries.borrow_mut() += 1;
        let state = u.borrow().core.state();
        let hit = state.servers.iter().find_map(|s| s.sessions.iter().find(|x| x.label == want || x.id == want).map(|x| (s.id.clone(), x.id.clone())));
        match hit {
            Some((srv, ses)) => {
                select(&u, srv, ses, None);
                glib::ControlFlow::Break
            }
            None if *tries.borrow() > 100 => glib::ControlFlow::Break,
            None => glib::ControlFlow::Continue,
        }
    });
}

/// SHREW_SCREENSHOT=out.png: open the first session, save the window to a PNG, quit.
/// Used for headless checks (e.g. GDK_BACKEND=broadway) so nothing pops up on a desktop.
fn screenshot_mode(ui: &Shared) {
    let Ok(path) = std::env::var("SHREW_SCREENSHOT") else { return };
    let u = ui.clone();
    glib::timeout_add_local_once(Duration::from_secs(4), move || {
        let state = u.borrow().core.state();
        let pick = std::env::var("SHREW_SCREENSHOT_SESSION").ok();
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

// ---- themes, pane focus, palette -----------------------------------------------

fn apply_theme(ui: &Shared, id: &str) {
    let mut u = ui.borrow_mut();
    let t = themes::by_id(id);
    u.theme_id = t.id.to_owned();
    u.theme_css.load_from_string(&t.css());
    adw::StyleManager::default().set_color_scheme(if t.is_light() { adw::ColorScheme::ForceLight } else { adw::ColorScheme::ForceDark });
    u.core.set_setting("theme", t.id.into());
    for v in u.terms.values() {
        v.set_theme(t.color_theme());
    }
}

/// The selected session's visible window and the block to act on: the pane with
/// keyboard focus, else the window's focused block.
fn current_target(ui: &Shared) -> Option<(Selection, Option<shrew_core::WindowState>, Option<String>)> {
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
    let block = typing.or_else(|| w.as_ref().and_then(|w| w.focused_block_id.clone().or(w.blocks.first().map(|b| b.id.clone()))));
    Some((sel, w, block))
}

fn cycle_pane(ui: &Shared, dir: i32) {
    let Some((_, _, block)) = current_target(ui) else { return };
    let u = ui.borrow();
    let ids: Vec<&String> = u.last_layout.iter().map(|(id, _)| id).collect();
    if ids.len() < 2 {
        return;
    }
    let i = block.and_then(|b| ids.iter().position(|x| **x == b)).unwrap_or(0) as i32;
    let next = ids[((i + dir).rem_euclid(ids.len() as i32)) as usize];
    if let Some(t) = u.terms.get(next) {
        t.grab_focus();
    }
}

fn cycle_window(ui: &Shared, dir: i32) {
    let Some((sel, w, _)) = current_target(ui) else { return };
    let state = ui.borrow().core.state();
    let Some(s) = state.server(&sel.server).and_then(|s| s.sessions.iter().find(|x| x.id == sel.session)) else { return };
    if s.windows.len() < 2 {
        return;
    }
    let i = w.and_then(|w| s.windows.iter().position(|x| x.id == w.id)).unwrap_or(0) as i32;
    let next = s.windows[((i + dir).rem_euclid(s.windows.len() as i32)) as usize].id.clone();
    if let Some(sel) = ui.borrow_mut().selected.as_mut() {
        sel.window = Some(next);
    }
    refresh(ui);
}

fn close_focused(ui: &Shared) {
    let Some((sel, _, Some(block))) = current_target(ui) else { return };
    let core = ui.borrow().core.clone();
    glib::spawn_future_local(async move {
        let _ = core.close_block(&sel.server, &sel.session, &block).await;
    });
}

struct Item {
    category: String,
    glyph: &'static str,
    title: String,
    subtitle: String,
    keywords: String,
    run: Rc<dyn Fn()>,
}

fn item(category: &str, glyph: &'static str, title: &str, run: impl Fn() + 'static) -> Item {
    Item { category: category.into(), glyph, title: title.into(), subtitle: String::new(), keywords: String::new(), run: Rc::new(run) }
}

fn local_items(ui: &Shared) -> Vec<Item> {
    let mut v = Vec::new();
    let Some((sel, _, block)) = current_target(ui) else { return v };
    let core = ui.borrow().core.clone();
    let act = |name: &'static str| {
        let u = ui.clone();
        move || {
            let w = u.borrow().window.clone();
            let _ = WidgetExt::activate_action(&w, &format!("app.{name}"), None);
        }
    };
    let u = ui.clone();
    v.push(item("Panes", "→", "Next Pane", move || cycle_pane(&u, 1)));
    let u = ui.clone();
    v.push(item("Panes", "←", "Previous Pane", move || cycle_pane(&u, -1)));
    let u = ui.clone();
    v.push(item("Panes", "▥", "Split Right", move || split_focused(&u, false)));
    let u = ui.clone();
    v.push(item("Panes", "▤", "Split Down", move || split_focused(&u, true)));
    let u = ui.clone();
    v.push(item("Panes", "×", "Close Pane", move || close_focused(&u)));
    if block.is_some() {
        let u = ui.clone();
        v.push(item("Panes", "⧉", "Copy All Text", move || {
            let (Some((_, _, Some(b))), u) = (current_target(&u), u.borrow()) else { return };
            if let Some(s) = u.terms.get(&b).and_then(|t| t.session()) {
                u.window.clipboard().set_text(&s.all_text());
            }
        }));
    }
    let (c, s2) = (core.clone(), sel.clone());
    v.push(item("Windows", "+", "New Window", move || {
        let (c, s2) = (c.clone(), s2.clone());
        glib::spawn_future_local(async move {
            let _ = c.new_window(&s2.server, &s2.session, None).await;
        });
    }));
    let u = ui.clone();
    v.push(item("Windows", "»", "Next Window", move || cycle_window(&u, 1)));
    let u = ui.clone();
    v.push(item("Windows", "«", "Previous Window", move || cycle_window(&u, -1)));
    let (c, u, srv) = (core.clone(), ui.clone(), sel.server.clone());
    v.push(item("Sessions", "+", "New Session", move || {
        let (c, u, srv) = (c.clone(), u.clone(), srv.clone());
        glib::spawn_future_local(async move {
            if let Ok((id, _)) = c.create_session(&srv, None, None).await {
                select(&u, srv, id, None);
            }
        });
    }));
    v.push(item("Sessions", "✎", "Rename Session…", act("rename-session")));
    let state = core.state();
    for s in state.server(&sel.server).map(|s| s.sessions.clone()).unwrap_or_default() {
        if s.id == sel.session {
            continue;
        }
        let (u, srv, id) = (ui.clone(), sel.server.clone(), s.id.clone());
        let mut it = item("Sessions", "▸", &format!("Go to {}", s.label), move || select(&u, srv.clone(), id.clone(), None));
        it.subtitle = s.windows.iter().flat_map(|w| w.blocks.iter()).next().map(|b| b.display_name()).unwrap_or_default();
        v.push(it);
    }
    let current = ui.borrow().theme_id.clone();
    for t in themes::THEMES {
        let u = ui.clone();
        let mut it = item("Themes", if t.id == current { "●" } else { "○" }, t.name, move || {
            let w = u.borrow().window.clone();
            let _ = WidgetExt::activate_action(&w, "app.theme", Some(&t.id.to_variant()));
        });
        it.keywords = "theme color".into();
        v.push(it);
    }
    v.push(item("View", "A", "Bigger Text", act("font-bigger")));
    v.push(item("View", "a", "Smaller Text", act("font-smaller")));
    v
}

fn palette_row(it: &Item) -> gtk::ListBoxRow {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let g = gtk::Label::new(Some(it.glyph));
    g.add_css_class("glyph");
    row.append(&g);
    let col = gtk::Box::new(gtk::Orientation::Vertical, 1);
    col.append(&label(&it.title, &[]));
    if !it.subtitle.is_empty() {
        col.append(&label(&it.subtitle, &["dim-label", "caption"]));
    }
    row.append(&col);
    gtk::ListBoxRow::builder().child(&row).build()
}

/// Everything you can do here, searchable: Shrew's commands plus the server's
/// own actions (its config's `rex.action`s and block actions like Clear Terminal).
fn open_palette(ui: &Shared) {
    let Some((sel, _, block)) = current_target(ui) else { return };
    let items: Rc<RefCell<Vec<Item>>> = Rc::new(RefCell::new(local_items(ui)));

    let search = gtk::SearchEntry::builder().placeholder_text("Run a command").hexpand(true).build();
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Browse).build();
    list.add_css_class("shrew-palette");
    list.add_css_class("navigation-sidebar");
    let scroll = gtk::ScrolledWindow::builder().child(&list).vexpand(true).hscrollbar_policy(gtk::PolicyType::Never).build();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    body.set_margin_start(12);
    body.set_margin_end(12);
    body.set_margin_bottom(12);
    body.append(&search);
    body.append(&scroll);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&body));
    let dialog = adw::Dialog::builder().title("Commands").content_width(520).content_height(480).child(&view).build();

    let add_rows = {
        let (list, items) = (list.clone(), items.clone());
        move |from: usize| {
            for (i, it) in items.borrow().iter().enumerate().skip(from) {
                let r = palette_row(it);
                r.set_widget_name(&i.to_string());
                list.append(&r);
            }
        }
    };
    add_rows(0);

    let matches = {
        let (items, search) = (items.clone(), search.clone());
        move |row: &gtk::ListBoxRow| {
            let q = search.text().to_lowercase();
            let Ok(i) = row.widget_name().parse::<usize>() else { return true };
            let items = items.borrow();
            let it = &items[i];
            let hay = format!("{} {} {} {}", it.title, it.subtitle, it.category, it.keywords).to_lowercase();
            q.split_whitespace().all(|w| hay.contains(w))
        }
    };
    let m = matches.clone();
    list.set_filter_func(move |row| m(row));
    let items2 = items.clone();
    list.set_header_func(move |row, before| {
        let cat = |r: &gtk::ListBoxRow| r.widget_name().parse::<usize>().ok().map(|i| items2.borrow()[i].category.clone());
        let this = cat(row);
        if before.and_then(cat) != this {
            let l = label(&this.unwrap_or_default().to_uppercase(), &["cat"]);
            l.set_margin_start(10);
            l.set_margin_top(10);
            l.set_margin_bottom(2);
            row.set_header(Some(&l));
        } else {
            row.set_header(None::<&gtk::Widget>);
        }
    });

    let run = {
        let (items, dialog) = (items.clone(), dialog.clone());
        move |row: &gtk::ListBoxRow| {
            let Ok(i) = row.widget_name().parse::<usize>() else { return };
            let f = items.borrow()[i].run.clone();
            dialog.close();
            f();
        }
    };
    let r = run.clone();
    list.connect_row_activated(move |_, row| r(row));
    let first_visible = {
        let list = list.clone();
        move || {
            let mut c = list.first_child();
            while let Some(w) = c {
                if let Ok(row) = w.clone().downcast::<gtk::ListBoxRow>()
                    && row.is_child_visible()
                {
                    return Some(row);
                }
                c = w.next_sibling();
            }
            None
        }
    };
    let (l, fv) = (list.clone(), first_visible.clone());
    search.connect_search_changed(move |_| {
        l.invalidate_filter();
        l.invalidate_headers();
        if let Some(r) = fv() {
            l.select_row(Some(&r));
        }
    });
    // Enter runs the highlighted result; Up/Down move through results while typing.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (l, search2, r2) = (list.clone(), search.clone(), run.clone());
    keys.connect_key_pressed(move |_, key, _, _| {
        let search = &search2;
        if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter) {
            if let Some(row) = l.selected_row().filter(|r| r.is_child_visible()).or_else(&first_visible) {
                r2(&row);
            }
            return glib::Propagation::Stop;
        }
        let step = match key {
            gdk::Key::Down => 1,
            gdk::Key::Up => -1,
            _ => return glib::Propagation::Proceed,
        };
        let mut rows = Vec::new();
        let mut c = l.first_child();
        while let Some(w) = c {
            if let Ok(row) = w.clone().downcast::<gtk::ListBoxRow>()
                && row.is_child_visible()
            {
                rows.push(row);
            }
            c = w.next_sibling();
        }
        if rows.is_empty() {
            return glib::Propagation::Stop;
        }
        let cur = l.selected_row().and_then(|s| rows.iter().position(|r| *r == s)).map(|p| p as i32).unwrap_or(-1);
        let next = &rows[((cur + step).rem_euclid(rows.len() as i32)) as usize];
        l.select_row(Some(next));
        next.grab_focus();
        search.grab_focus();
        search.set_position(-1);
        glib::Propagation::Stop
    });
    dialog.add_controller(keys);

    // The server's actions arrive a moment later.
    let core = ui.borrow().core.clone();
    let (items3, list3) = (items.clone(), list.clone());
    glib::spawn_future_local(async move {
        let Ok(actions) = core.palette_actions(&sel.server).await else { return };
        let from = items3.borrow().len();
        for a in actions {
            let (core, srv, ses, blk, name) = (core.clone(), sel.server.clone(), sel.session.clone(), block.clone(), a.name.clone());
            items3.borrow_mut().push(Item {
                category: if a.category.is_empty() { "Actions".into() } else { a.category },
                glyph: if a.name.contains('.') { "◆" } else { "λ" },
                title: a.title,
                subtitle: a.description,
                keywords: format!("{} {}", a.keywords, a.name),
                run: Rc::new(move || {
                    let (core, srv, ses, blk, name) = (core.clone(), srv.clone(), ses.clone(), blk.clone(), name.clone());
                    glib::spawn_future_local(async move {
                        let _ = core.run_action(&srv, &ses, blk.as_deref(), &name).await;
                    });
                }),
            });
        }
        add_rows(from);
        list3.invalidate_filter();
        list3.invalidate_headers();
    });

    let parent = ui.borrow().window.clone();
    dialog.present(Some(&parent));
    let s = search.clone();
    glib::idle_add_local_once(move || {
        s.grab_focus();
    });
    // When the palette closes, give the keyboard back to the terminal.
    let u = ui.clone();
    dialog.connect_closed(move |_| {
        if let Some((_, _, Some(b))) = current_target(&u)
            && let Some(t) = u.borrow().terms.get(&b)
        {
            t.grab_focus();
        }
    });
}
