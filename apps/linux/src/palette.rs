//! The command palette: Raptor's commands plus the server's own actions
//! (its config's `rex.action`s and block actions like Clear Terminal).

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};

use crate::{Shared, current_target, label, select, themes};

struct Item {
    category: String,
    glyph: &'static str,
    title: String,
    subtitle: String,
    keywords: String,
    accel: Option<String>,
    run: Rc<dyn Fn()>,
}

fn item(category: &str, glyph: &'static str, title: &str, run: impl Fn() + 'static) -> Item {
    Item { category: category.into(), glyph, title: title.into(), subtitle: String::new(), keywords: String::new(), accel: None, run: Rc::new(run) }
}

fn local_items(ui: &Shared) -> Vec<Item> {
    let mut v = Vec::new();
    let Some((sel, _, _)) = current_target(ui) else { return v };
    let window = ui.borrow().window.clone();
    let app = window.application().unwrap();
    let mut act = |category: &str, glyph: &'static str, title: &str, name: &'static str| {
        let w = window.clone();
        let mut it = item(category, glyph, title, move || {
            let _ = WidgetExt::activate_action(&w, &format!("app.{name}"), None);
        });
        it.accel = app.accels_for_action(&format!("app.{name}")).first().map(|a| a.to_string());
        v.push(it);
    };
    act("Panes", "▥", "Split Right", "split-right");
    act("Panes", "▤", "Split Down", "split-down");
    act("Panes", "⤢", "Zoom Pane", "zoom-pane");
    act("Panes", "→", "Next Pane", "next-pane");
    act("Panes", "←", "Previous Pane", "prev-pane");
    act("Panes", "×", "Close Pane", "close-pane");
    act("Tabs", "+", "New Tab", "new-window");
    act("Tabs", "✎", "Rename Tab…", "rename-window");
    act("Tabs", "×", "Close Tab", "close-window");
    act("Sessions", "+", "New Session", "new-session");
    act("Sessions", "✎", "Rename Session…", "rename-session");
    act("Sessions", "⏻", "End Session…", "end-session");
    act("View", "A", "Bigger Text", "font-bigger");
    act("View", "a", "Smaller Text", "font-smaller");
    act("View", "=", "Reset Text Size", "font-reset");
    act("View", "⛶", "Full Screen", "fullscreen");
    act("Raptor", "⚙", "Preferences", "preferences");
    act("Raptor", "⌨", "Keyboard Shortcuts", "shortcuts");
    act("Raptor", "+", "Add Server…", "add-server");
    let state = ui.borrow().core.state();
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
        let w = window.clone();
        let mut it = item("Themes", if t.id == current { "●" } else { "○" }, t.name, move || {
            let _ = WidgetExt::activate_action(&w, "app.theme", Some(&t.id.to_variant()));
        });
        it.keywords = "theme color".into();
        v.push(it);
    }
    v
}

fn palette_row(it: &Item) -> gtk::ListBoxRow {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let g = gtk::Label::new(Some(it.glyph));
    g.add_css_class("glyph");
    row.append(&g);
    let col = gtk::Box::new(gtk::Orientation::Vertical, 1);
    col.set_hexpand(true);
    col.append(&label(&it.title, &[]));
    if !it.subtitle.is_empty() {
        col.append(&label(&it.subtitle, &["dim-label", "caption"]));
    }
    row.append(&col);
    if let Some(a) = &it.accel {
        let s = gtk::ShortcutLabel::new(a);
        s.add_css_class("dim-label");
        row.append(&s);
    }
    gtk::ListBoxRow::builder().child(&row).build()
}

pub fn open(ui: &Shared) {
    let Some((sel, _, block)) = current_target(ui) else { return };
    let items: Rc<RefCell<Vec<Item>>> = Rc::new(RefCell::new(local_items(ui)));

    let search = gtk::SearchEntry::builder().placeholder_text("Run a command").hexpand(true).build();
    let list = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Browse).build();
    list.add_css_class("raptor-palette");
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
    list.set_filter_func(matches);
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
    let visible_rows = {
        let list = list.clone();
        move || {
            let mut rows = Vec::new();
            let mut c = list.first_child();
            while let Some(w) = c {
                if let Ok(row) = w.clone().downcast::<gtk::ListBoxRow>()
                    && row.is_child_visible()
                {
                    rows.push(row);
                }
                c = w.next_sibling();
            }
            rows
        }
    };
    let (l, vr) = (list.clone(), visible_rows.clone());
    search.connect_search_changed(move |_| {
        l.invalidate_filter();
        l.invalidate_headers();
        if let Some(r) = vr().first() {
            l.select_row(Some(r));
        }
    });
    if let Some(r) = visible_rows().first() {
        list.select_row(Some(r));
    }
    // Enter runs the highlighted result; Up/Down move through results while typing.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (l, search2, r2) = (list.clone(), search.clone(), run.clone());
    keys.connect_key_pressed(move |_, key, _, _| {
        let rows = visible_rows();
        if matches!(key, gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter) {
            if let Some(row) = l.selected_row().filter(|r| r.is_child_visible()).or_else(|| rows.first().cloned()) {
                r2(&row);
            }
            return glib::Propagation::Stop;
        }
        let step = match key {
            gdk::Key::Down => 1,
            gdk::Key::Up => -1,
            _ => return glib::Propagation::Proceed,
        };
        if rows.is_empty() {
            return glib::Propagation::Stop;
        }
        let cur = l.selected_row().and_then(|s| rows.iter().position(|r| *r == s)).map(|p| p as i32).unwrap_or(-1);
        let next = &rows[((cur + step).rem_euclid(rows.len() as i32)) as usize];
        l.select_row(Some(next));
        next.grab_focus();
        search2.grab_focus();
        search2.set_position(-1);
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
                accel: None,
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
            && let Some(t) = u.borrow().terms.get(&b).cloned()
        {
            t.grab_focus();
        }
    });
}
