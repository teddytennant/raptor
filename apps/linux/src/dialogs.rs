//! Dialogs: servers, renaming, confirmations, preferences, shortcuts.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::{Shared, apply_prefs, apply_theme, current_target, fonts, spawn_call, themes};

/// Add a server, or edit one when `id` is given. The address is probed as you type.
pub fn server_dialog(ui: &Shared, id: Option<String>) {
    let u = ui.borrow();
    let existing = id.as_ref().and_then(|id| u.core.state().server(id).cloned());
    let addr = adw::EntryRow::builder().title("Address").activates_default(true).text(existing.as_ref().map(|s| s.address.as_str()).unwrap_or("")).build();
    let name = adw::EntryRow::builder().title("Name (optional)").activates_default(true).text(existing.as_ref().map(|s| s.name.as_str()).unwrap_or("")).build();
    let status = gtk::Label::builder().wrap(true).xalign(0.0).build();
    status.add_css_class("dim-label");
    let icon = gtk::Image::new();
    let status_box = gtk::Box::builder().spacing(8).margin_top(12).margin_start(4).build();
    status_box.append(&icon);
    status_box.append(&status);
    let set_status = {
        let (status, icon) = (status.clone(), icon.clone());
        move |kind: &str, text: &str| {
            status.set_label(text);
            icon.set_visible(kind != "none");
            for c in ["success", "error", "dim-label"] {
                status.remove_css_class(c);
                icon.remove_css_class(c);
            }
            let (name, class) = match kind {
                "ok" => ("object-select-symbolic", "success"),
                "err" => ("dialog-warning-symbolic", "error"),
                _ => ("content-loading-symbolic", "dim-label"),
            };
            icon.set_icon_name(Some(name));
            status.add_css_class(class);
            icon.add_css_class(class);
        }
    };
    set_status("none", "A host name, host:port, a tailnet name, or a socket path: anything you'd pass to rex -S.");

    let group = adw::PreferencesGroup::new();
    group.add(&addr);
    group.add(&name);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.set_margin_top(12);
    body.set_margin_bottom(24);
    body.set_margin_start(12);
    body.set_margin_end(12);
    body.append(&group);
    body.append(&status_box);

    let editing = existing.is_some();
    let save = gtk::Button::with_label(if editing { "Save" } else { "Add" });
    save.add_css_class("suggested-action");
    save.set_sensitive(editing);
    let cancel = gtk::Button::with_label("Cancel");
    let header = adw::HeaderBar::builder().show_end_title_buttons(false).show_start_title_buttons(false).build();
    header.pack_start(&cancel);
    header.pack_end(&save);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&body));
    let d = adw::Dialog::builder().title(if editing { "Edit Server" } else { "Add a Server" }).content_width(440).child(&view).build();
    d.set_default_widget(Some(&save));

    let addr_focus = addr.clone();
    let generation = Rc::new(Cell::new(0u64));
    let core = u.core.clone();
    let (st, sv) = (set_status.clone(), save.clone());
    addr.connect_changed(move |e| {
        let text = e.text().trim().to_owned();
        generation.set(generation.get() + 1);
        let my = generation.get();
        sv.set_sensitive(!text.is_empty());
        if text.is_empty() {
            st("none", "");
            return;
        }
        let (core, st, generation) = (core.clone(), st.clone(), generation.clone());
        glib::spawn_future_local(async move {
            glib::timeout_future(Duration::from_millis(450)).await;
            if generation.get() != my {
                return;
            }
            st("busy", "Checking…");
            let r = core.probe(&text).await;
            if generation.get() != my {
                return;
            }
            match r {
                Ok(v) => st("ok", &format!("Connected to Rex {v}")),
                Err(e) => st("err", &format!("{e}. You can still add it; Raptor will keep trying.")),
            }
        });
    });
    let dd = d.clone();
    cancel.connect_clicked(move |_| {
        dd.close();
    });
    let (core, dd, uu) = (u.core.clone(), d.clone(), ui.clone());
    let activate = move || {
        let (a, n) = (addr.text().trim().to_owned(), name.text().trim().to_owned());
        if a.is_empty() {
            return;
        }
        let r = match &id {
            Some(id) => core.update_server(id, &n, &a),
            None => core.add_server(&n, &a).map(drop),
        };
        match r {
            Ok(()) => {
                dd.close();
                crate::refresh(&uu);
            }
            Err(e) => set_status("err", &e.to_string()),
        }
    };
    save.connect_clicked(move |_| activate());
    d.set_focus(Some(&addr_focus));
    d.present(Some(&u.window));
}

pub fn remove_server(ui: &Shared, id: &str) {
    let u = ui.borrow();
    let name = u.core.state().server(id).map(|s| s.name.clone()).unwrap_or_default();
    let d = adw::AlertDialog::new(
        Some(&format!("Remove {name}?")),
        Some("Raptor forgets this server. Its sessions keep running on the server."),
    );
    d.add_responses(&[("cancel", "Cancel"), ("remove", "Remove")]);
    d.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    d.set_default_response(Some("cancel"));
    let (uu, id) = (ui.clone(), id.to_owned());
    d.connect_response(None, move |_, r| {
        if r == "remove" {
            let mut b = uu.borrow_mut();
            if b.selected.as_ref().is_some_and(|s| s.server == id) {
                b.selected = None;
            }
            b.core.remove_server(&id);
            drop(b);
            crate::refresh(&uu);
        }
    });
    d.present(Some(&u.window));
}

fn ask_text(ui: &Shared, title: &str, current: &str, ok: &str, done: impl Fn(String) + 'static) {
    let entry = gtk::Entry::builder().text(current).activates_default(true).build();
    let d = adw::AlertDialog::new(Some(title), None);
    d.set_extra_child(Some(&entry));
    d.add_responses(&[("cancel", "Cancel"), ("ok", ok)]);
    d.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    d.set_default_response(Some("ok"));
    d.set_close_response("cancel");
    d.connect_response(None, move |_, r| {
        if r == "ok" {
            done(entry.text().trim().to_owned());
        }
    });
    d.present(Some(&ui.borrow().window));
}

pub fn rename_session(ui: &Shared) {
    let Some(sel) = ui.borrow().selected.clone() else { return };
    let core = ui.borrow().core.clone();
    let current = core.state().server(&sel.server).and_then(|s| s.sessions.iter().find(|x| x.id == sel.session)).map(|s| s.label.clone());
    let u = ui.clone();
    ask_text(ui, "Rename Session", &current.unwrap_or_default(), "Rename", move |name| {
        let (core, sel) = (core.clone(), sel.clone());
        spawn_call(&u, "rename the session", async move { core.rename_session(&sel.server, &sel.session, &name).await });
    });
}

pub fn rename_window(ui: &Shared, window: String) {
    let Some(sel) = ui.borrow().selected.clone() else { return };
    let core = ui.borrow().core.clone();
    let current = core
        .state()
        .server(&sel.server)
        .and_then(|s| s.sessions.iter().find(|x| x.id == sel.session))
        .and_then(|s| s.windows.iter().find(|w| w.id == window))
        .map(|w| w.label.clone());
    let u = ui.clone();
    ask_text(ui, "Rename Tab", &current.unwrap_or_default(), "Rename", move |name| {
        let (core, sel, w) = (core.clone(), sel.clone(), window.clone());
        spawn_call(&u, "rename the tab", async move { core.rename_window(&sel.server, &sel.session, &w, &name).await });
    });
}

pub fn end_session(ui: &Shared) {
    let Some((sel, _, _)) = current_target(ui) else { return };
    let u = ui.borrow();
    let state = u.core.state();
    let Some(s) = state.server(&sel.server).and_then(|s| s.sessions.iter().find(|x| x.id == sel.session)) else { return };
    let d = adw::AlertDialog::new(
        Some(&format!("End {}?", s.label)),
        Some(&format!(
            "This closes {} and ends every program running in it, on the server too.",
            match s.block_count() {
                1 => "its terminal".to_owned(),
                n => format!("all {n} of its terminals"),
            }
        )),
    );
    d.add_responses(&[("cancel", "Cancel"), ("end", "End Session")]);
    d.set_response_appearance("end", adw::ResponseAppearance::Destructive);
    d.set_default_response(Some("cancel"));
    let (core, uu) = (u.core.clone(), ui.clone());
    d.connect_response(None, move |_, r| {
        if r == "end" {
            let (core, sel) = (core.clone(), sel.clone());
            spawn_call(&uu, "end the session", async move { core.destroy_session(&sel.server, &sel.session).await });
        }
    });
    d.present(Some(&u.window));
}

pub fn preferences(ui: &Shared) {
    let u = ui.borrow();
    let page = adw::PreferencesPage::new();

    let look = adw::PreferencesGroup::builder().title("Appearance").build();
    let names: Vec<&str> = themes::THEMES.iter().map(|t| t.name).collect();
    let theme = adw::ComboRow::builder().title("Theme").model(&gtk::StringList::new(&names)).build();
    theme.set_selected(themes::THEMES.iter().position(|t| t.id == u.theme_id).unwrap_or(0) as u32);
    let uu = ui.clone();
    theme.connect_selected_notify(move |r| {
        if let Some(t) = themes::THEMES.get(r.selected() as usize) {
            apply_theme(&uu, t.id);
        }
    });
    look.add(&theme);

    let mut families: Vec<String> = fonts::FAMILIES.iter().map(|s| s.to_string()).collect();
    families.extend(fonts::installed(&u.window.pango_context()));
    let refs: Vec<&str> = families.iter().map(String::as_str).collect();
    let font = adw::ComboRow::builder().title("Font").model(&gtk::StringList::new(&refs)).enable_search(true).build();
    font.set_selected(families.iter().position(|f| *f == u.prefs.family).unwrap_or(0) as u32);
    let uu = ui.clone();
    font.connect_selected_notify(move |r| {
        if let Some(f) = families.get(r.selected() as usize) {
            let mut b = uu.borrow_mut();
            b.prefs.family = f.clone();
            b.core.set_setting("font_family", f.clone().into());
            apply_prefs(&b);
        }
    });
    look.add(&font);
    let size = adw::SpinRow::with_range(7.0, 40.0, 1.0);
    size.set_title("Text Size");
    size.set_value(u.prefs.size);
    let uu = ui.clone();
    size.connect_value_notify(move |r| {
        let mut b = uu.borrow_mut();
        if b.prefs.size != r.value() {
            b.prefs.size = r.value();
            b.core.set_setting("font_size", r.value().into());
            apply_prefs(&b);
        }
    });
    look.add(&size);
    page.add(&look);

    let term = adw::PreferencesGroup::builder().title("Terminal").build();
    let blink = adw::SwitchRow::builder().title("Blinking Cursor").subtitle("Stops blinking after a few seconds without typing").active(u.prefs.blink).build();
    let uu = ui.clone();
    blink.connect_active_notify(move |r| {
        let mut b = uu.borrow_mut();
        b.prefs.blink = r.is_active();
        b.core.set_setting("cursor_blink", r.is_active().into());
        apply_prefs(&b);
    });
    term.add(&blink);
    let copy = adw::SwitchRow::builder()
        .title("Copy on Select")
        .subtitle("Selected text also goes to the clipboard, not only the middle-click selection")
        .active(u.prefs.copy_on_select)
        .build();
    let uu = ui.clone();
    copy.connect_active_notify(move |r| {
        let mut b = uu.borrow_mut();
        b.prefs.copy_on_select = r.is_active();
        b.core.set_setting("copy_on_select", r.is_active().into());
        apply_prefs(&b);
    });
    term.add(&copy);
    page.add(&term);

    let notes = adw::PreferencesGroup::builder().title("Notifications").build();
    let agents = adw::SwitchRow::builder()
        .title("Agent Notifications")
        .subtitle("When a program needs you, finishes, or fails while you're looking elsewhere")
        .active(crate::setting_bool(&u.core, "notifications", true))
        .build();
    let core = u.core.clone();
    agents.connect_active_notify(move |r| core.set_setting("notifications", r.is_active().into()));
    notes.add(&agents);
    page.add(&notes);

    let d = adw::PreferencesDialog::new();
    d.add(&page);
    d.present(Some(&u.window));
}

const SHORTCUTS: &[(&str, &[(&str, &str)])] = &[
    (
        "Sessions and Tabs",
        &[
            ("New Session", "<Control><Shift>n"),
            ("New Tab", "<Control><Shift>t"),
            ("Next Tab", "<Control>Page_Down"),
            ("Previous Tab", "<Control>Page_Up"),
            ("Go to Tab 1–9", "<Alt>1"),
            ("Commands", "<Control><Shift>p"),
            ("Switch Session", "<Control><Shift>s"),
        ],
    ),
    (
        "Panes",
        &[
            ("Split Right", "<Control><Shift>o"),
            ("Split Down", "<Control><Shift>e"),
            ("Zoom Pane", "<Control><Shift>Return"),
            ("Close Pane", "<Control><Shift>w"),
            ("Next Pane", "<Control>Tab"),
            ("Previous Pane", "<Control><Shift>Tab"),
            ("Pane to the Left/Right/Above/Below", "<Control><Alt>Left"),
        ],
    ),
    (
        "Terminal",
        &[
            ("Copy", "<Control><Shift>c"),
            ("Paste", "<Control><Shift>v"),
            ("Paste Selection", "Middle click"),
            ("Find in Scrollback", "<Control><Shift>f"),
            ("Scroll a Page", "<Shift>Page_Up"),
            ("Scroll to Top/Bottom", "<Shift>Home"),
            ("Previous/Next Prompt", "<Control><Shift>Up"),
            ("Send Mouse to Raptor, Not the Program", "Shift + drag"),
        ],
    ),
    (
        "View",
        &[
            ("Bigger Text", "<Control>plus"),
            ("Smaller Text", "<Control>minus"),
            ("Reset Text Size", "<Control>0"),
            ("Full Screen", "F11"),
            ("Preferences", "<Control>comma"),
            ("Keyboard Shortcuts", "<Control>question"),
        ],
    ),
];

pub fn shortcuts(ui: &Shared) {
    let page = adw::PreferencesPage::new();
    for (title, items) in SHORTCUTS {
        let g = adw::PreferencesGroup::builder().title(*title).build();
        for (what, accel) in *items {
            let row = adw::ActionRow::builder().title(*what).build();
            if accel.starts_with('<') || accel.starts_with('F') {
                let l = gtk::ShortcutLabel::new(accel);
                l.set_valign(gtk::Align::Center);
                row.add_suffix(&l);
            } else {
                let l = gtk::Label::new(Some(accel));
                l.add_css_class("dim-label");
                row.add_suffix(&l);
            }
            g.add(&row);
        }
        page.add(&g);
    }
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&page));
    let d = adw::Dialog::builder().title("Keyboard Shortcuts").content_width(520).content_height(640).child(&view).build();
    d.present(Some(&ui.borrow().window));
}

pub fn about(ui: &Shared) {
    let d = adw::AboutDialog::builder()
        .application_name("Raptor")
        .application_icon(crate::APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .comments("A client for Rex terminal servers.\nRex is a product of Superlogical; Raptor is an independent project.")
        .website("https://github.com/teddytennant/raptor")
        .issue_url("https://github.com/teddytennant/raptor/issues")
        .license_type(gtk::License::MitX11)
        .build();
    d.present(Some(&ui.borrow().window));
}
