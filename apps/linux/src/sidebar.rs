//! The session list in the header's switcher: what needs you, then each
//! server and its sessions.

use adw::prelude::*;
use gtk::{gio, glib};
use raptor_core::{AgentState, AppState, Connectivity};

use crate::{Shared, agent_class, dialogs, dot, label, new_session};

/// Everything the sidebar shows, so it is only rebuilt when that changes.
fn key(ui: &Shared, state: &AppState) -> String {
    use std::fmt::Write;
    let sel = ui.borrow().selected.clone();
    let mut k = format!("{:?}|", sel.map(|s| (s.server, s.session)));
    for a in state.attention() {
        let ag = a.block.agent.as_ref().unwrap();
        let _ = write!(k, "{}{}{:?}{}{}{};", a.server_id, a.block.id, ag.state, ag.app, ag.message, a.session_label);
    }
    for s in &state.servers {
        let _ = write!(k, "[{}{}{:?}{:?}{}]", s.id, s.name, s.connectivity, s.latency_ms.map(|l| l / 25), s.address);
        for x in &s.sessions {
            let f = focused(x);
            let _ = write!(
                k,
                "({}{}{}{:?}{:?})",
                x.id,
                x.label,
                x.windows.len(),
                x.agent_state(),
                f.map(|b| (b.display_name(), b.pwd.clone()))
            );
        }
    }
    k
}

fn focused(s: &raptor_core::SessionState) -> Option<&raptor_core::BlockState> {
    let active = s.windows.iter().find(|w| Some(&w.id) == s.active_window_id.as_ref()).or(s.windows.first());
    active.and_then(|w| w.blocks.iter().find(|b| Some(&b.id) == w.focused_block_id.as_ref()).or(w.blocks.first()))
}

/// Text a search in the switcher matches, kept on each session row.
const SEARCH: &str = "raptor-search";

fn set_search(row: &gtk::ListBoxRow, text: String) {
    // SAFETY: SEARCH is only ever stored and read as a String.
    unsafe { row.set_data(SEARCH, text) };
}

fn search_text(row: &gtk::ListBoxRow) -> Option<String> {
    // SAFETY: as above.
    unsafe { row.data::<String>(SEARCH).map(|p| p.as_ref().clone()) }
}

/// Typing in the switcher filters the list; Return opens the first match, and
/// the list starts unfiltered each time it opens.
pub fn install_filter(list: &gtk::ListBox, entry: &gtk::SearchEntry, pop: &gtk::Popover) {
    let e = entry.clone();
    list.set_filter_func(move |row| {
        let q = e.text().trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        search_text(row).is_some_and(|t| t.contains(&q))
    });
    let l = list.clone();
    entry.connect_search_changed(move |_| l.invalidate_filter());
    let l = list.clone();
    entry.connect_activate(move |_| {
        let mut c = l.first_child();
        while let Some(w) = c {
            if let Some(r) = w.downcast_ref::<gtk::ListBoxRow>()
                && r.is_child_visible()
                && r.is_activatable()
                && search_text(r).is_some()
            {
                r.activate();
                return;
            }
            c = w.next_sibling();
        }
    });
    let l = list.clone();
    entry.connect_stop_search(move |e| {
        e.set_text("");
        if let Some(p) = l.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>() {
            p.popdown();
        }
    });
    let (e, l) = (entry.clone(), list.clone());
    pop.connect_show(move |_| {
        e.set_text("");
        l.invalidate_filter();
        e.grab_focus();
    });
}

pub fn rebuild(ui: &Shared, state: &AppState) {
    let k = key(ui, state);
    if ui.borrow().sidebar_key == k {
        return;
    }
    ui.borrow_mut().sidebar_key = k;
    let u = ui.borrow();
    let list = u.sidebar.clone();
    let selected = u.selected.clone();
    drop(u);
    // Keep the scroll position across rebuilds.
    let adj = list.parent().and_then(|v| v.parent()).and_downcast::<gtk::ScrolledWindow>().map(|s| s.vadjustment());
    let scroll = adj.as_ref().map(|a| a.value());
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    let section = |t: &str| {
        let r = gtk::ListBoxRow::builder().selectable(false).activatable(false).build();
        r.set_child(Some(&label(t, &["raptor-section"])));
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
            set_search(&row, format!("{} {} {}", a.session_label, a.server_name, a.block.display_name()).to_lowercase());
            row.add_css_class("raptor-needs");
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            b.set_margin_top(8);
            b.set_margin_bottom(8);
            b.set_margin_start(8);
            b.set_margin_end(8);
            b.append(&dot(agent_class(Some(ag.state))));
            let v = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let who = if ag.app.is_empty() { a.block.display_name() } else { ag.app.clone() };
            let what = match ag.state {
                AgentState::Error => format!("{who} hit an error"),
                _ => format!("{who} needs you"),
            };
            v.append(&label(&what, &["heading"]));
            if !ag.message.is_empty() {
                let m = label(&ag.message, &[]);
                m.set_wrap(true);
                m.set_lines(2);
                m.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                v.append(&m);
            }
            v.append(&label(&format!("{} · {}", a.session_label, a.server_name), &["caption", "dim-label"]));
            b.append(&v);
            row.set_child(Some(&b));
            row.set_tooltip_text(Some("Open this terminal"));
            list.append(&row);
        }
    }

    for srv in &state.servers {
        let head = gtk::ListBoxRow::builder().selectable(false).activatable(false).build();
        let hb = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        hb.set_margin_top(14);
        hb.set_margin_start(6);
        let (cls, status) = match &srv.connectivity {
            Connectivity::Online => ("dot-online", srv.latency_ms.map(|l| format!("Online · {l} ms")).unwrap_or_else(|| "Online".into())),
            Connectivity::Connecting => ("dot-connecting", "Connecting…".into()),
            Connectivity::Offline { error, .. } => ("dot-offline", error.clone()),
            Connectivity::Idle => ("dot-idle", "Paused".into()),
        };
        let v = gtk::Box::new(gtk::Orientation::Vertical, 0);
        v.set_hexpand(true);
        v.append(&label(&srv.name, &["raptor-server"]));
        let sb = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        sb.append(&dot(cls));
        let st = label(&status, &["caption", "dim-label"]);
        st.set_tooltip_text(Some(&format!("{}\n{status}", srv.address)));
        sb.append(&st);
        v.append(&sb);
        hb.append(&v);
        if matches!(srv.connectivity, Connectivity::Online) {
            let nb = gtk::Button::from_icon_name("list-add-symbolic");
            nb.add_css_class("flat");
            nb.set_valign(gtk::Align::Center);
            nb.set_tooltip_text(Some("New Session"));
            let (u, sid) = (ui.clone(), srv.id.clone());
            nb.connect_clicked(move |_| new_session(&u, sid.clone()));
            hb.append(&nb);
        } else if matches!(srv.connectivity, Connectivity::Offline { .. }) {
            let rb = gtk::Button::from_icon_name("view-refresh-symbolic");
            rb.add_css_class("flat");
            rb.set_valign(gtk::Align::Center);
            rb.set_tooltip_text(Some("Try Again Now"));
            let (core, sid) = (ui.borrow().core.clone(), srv.id.clone());
            rb.connect_clicked(move |_| core.reconnect(&sid));
            hb.append(&rb);
        }
        hb.append(&server_menu(ui, &srv.id));
        head.set_child(Some(&hb));
        list.append(&head);

        if srv.sessions.is_empty() && matches!(srv.connectivity, Connectivity::Online) {
            let r = gtk::ListBoxRow::builder().selectable(false).activatable(false).build();
            let l = label("No sessions", &["dim-label", "caption"]);
            l.set_margin_start(10);
            l.set_margin_top(4);
            l.set_margin_bottom(4);
            r.set_child(Some(&l));
            list.append(&r);
        }
        for s in &srv.sessions {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("session|{}|{}", srv.id, s.id));
            set_search(&row, format!("{} {}", s.label, srv.name).to_lowercase());
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            b.set_margin_top(6);
            b.set_margin_bottom(6);
            b.set_margin_start(8);
            let v = gtk::Box::new(gtk::Orientation::Vertical, 1);
            v.set_hexpand(true);
            v.append(&label(&s.label, &[]));
            let detail = match focused(s) {
                Some(b) if !b.pwd.is_empty() => format!("{} · {}", b.display_name(), raptor_core::short_path(&b.pwd)),
                Some(b) => b.display_name(),
                None => String::new(),
            };
            v.append(&label(&detail, &["caption", "dim-label", "raptor-mono"]));
            b.append(&v);
            if let Some(st) = s.agent_state().filter(|s| *s != AgentState::Idle) {
                let d = dot(agent_class(Some(st)));
                d.set_tooltip_text(Some(match st {
                    AgentState::Working => "Working",
                    AgentState::Blocked => "Needs you",
                    AgentState::Done => "Finished",
                    AgentState::Error => "Error",
                    AgentState::Idle => "",
                }));
                b.append(&d);
            }
            if s.windows.len() > 1 {
                let count = gtk::Label::new(Some(&s.windows.len().to_string()));
                count.add_css_class("dim-label");
                count.add_css_class("raptor-count");
                count.set_tooltip_text(Some(&format!("{} tabs", s.windows.len())));
                b.append(&count);
            }
            b.set_margin_end(6);
            row.set_child(Some(&b));
            session_context_menu(ui, &row, &srv.id, &s.id);
            list.append(&row);
            if selected.as_ref().is_some_and(|x| x.server == srv.id && x.session == s.id) {
                list.select_row(Some(&row));
            }
        }
    }
    if let (Some(adj), Some(v)) = (adj, scroll) {
        glib::idle_add_local_once(move || adj.set_value(v));
    }
}

fn server_menu(ui: &Shared, id: &str) -> gtk::MenuButton {
    let m = gio::Menu::new();
    m.append(Some("Edit…"), Some("srv.edit"));
    m.append(Some("Reconnect"), Some("srv.reconnect"));
    let danger = gio::Menu::new();
    danger.append(Some("Remove…"), Some("srv.remove"));
    m.append_section(None, &danger);
    let b = gtk::MenuButton::builder().icon_name("view-more-symbolic").menu_model(&m).valign(gtk::Align::Center).tooltip_text("Server Options").build();
    b.add_css_class("flat");
    let group = gio::SimpleActionGroup::new();
    let add = |name: &str, f: fn(&Shared, &str)| {
        let a = gio::SimpleAction::new(name, None);
        let (u, id) = (ui.clone(), id.to_owned());
        a.connect_activate(move |_, _| f(&u, &id));
        group.add_action(&a);
    };
    add("edit", |u, id| dialogs::server_dialog(u, Some(id.to_owned())));
    add("reconnect", |u, id| u.borrow().core.reconnect(id));
    add("remove", dialogs::remove_server);
    b.insert_action_group("srv", Some(&group));
    b
}

fn session_context_menu(ui: &Shared, row: &gtk::ListBoxRow, server: &str, session: &str) {
    let m = gio::Menu::new();
    m.append(Some("Rename…"), Some("ses.rename"));
    m.append(Some("End Session…"), Some("ses.end"));
    let group = gio::SimpleActionGroup::new();
    let add = |name: &str, f: fn(&Shared)| {
        let a = gio::SimpleAction::new(name, None);
        let (u, srv, ses) = (ui.clone(), server.to_owned(), session.to_owned());
        a.connect_activate(move |_, _| {
            crate::select(&u, srv.clone(), ses.clone(), None);
            f(&u);
        });
        group.add_action(&a);
    };
    add("rename", dialogs::rename_session);
    add("end", dialogs::end_session);
    row.insert_action_group("ses", Some(&group));
    let click = gtk::GestureClick::builder().button(3).build();
    let r = row.clone();
    click.connect_pressed(move |g, _, x, y| {
        g.set_state(gtk::EventSequenceState::Claimed);
        let pop = gtk::PopoverMenu::from_model(Some(&m));
        pop.set_parent(&r);
        pop.set_has_arrow(false);
        pop.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        pop.popup();
    });
    row.add_controller(click);
}
