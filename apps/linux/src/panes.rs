//! Lays out one window's panes where the server says they go, with a
//! draggable divider on every split.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use raptor_core::SplitState;

/// Gap between panes, in logical pixels. The divider line is drawn in it.
const GAP: f64 = 1.0;
/// How wide the invisible grab area around a divider is.
const GRAB: f64 = 7.0;

type OnResize = Rc<dyn Fn(&SplitState, f64)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct PaneGrid {
        pub panes: RefCell<Vec<(gtk::Widget, [f64; 4])>>,
        pub dividers: RefCell<Vec<(gtk::Widget, SplitState)>>,
        pub on_resize: RefCell<Option<OnResize>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PaneGrid {
        const NAME: &'static str = "RaptorPaneGrid";
        type Type = super::PaneGrid;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("panegrid");
        }
    }

    impl ObjectImpl for PaneGrid {
        fn dispose(&self) {
            while let Some(c) = self.obj().first_child() {
                c.unparent();
            }
        }
    }

    impl WidgetImpl for PaneGrid {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            (0, 0, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let (w, h) = (width as f64, height as f64);
            for (child, r) in self.panes.borrow().iter() {
                let (x0, y0) = ((r[0] * w).round(), (r[1] * h).round());
                let (x1, y1) = (((r[0] + r[2]) * w).round(), ((r[1] + r[3]) * h).round());
                // Leave a gap only on sides that border another pane.
                let x1 = if x1 < w - 0.5 { x1 - GAP } else { x1 };
                let y1 = if y1 < h - 0.5 { y1 - GAP } else { y1 };
                let rect = gtk::Allocation::new(x0 as i32, y0 as i32, (x1 - x0).max(1.0) as i32, (y1 - y0).max(1.0) as i32);
                // GTK wants every child measured before it is allocated.
                let _ = child.measure(gtk::Orientation::Horizontal, -1);
                child.size_allocate(&rect, -1);
            }
            for (child, s) in self.dividers.borrow().iter() {
                let r = s.rect;
                let rect = if s.vertical {
                    let y = ((r[1] + r[3] * s.ratio) * h).round() - GAP / 2.0;
                    gtk::Allocation::new((r[0] * w) as i32, (y - GRAB / 2.0) as i32, (r[2] * w) as i32, GRAB as i32)
                } else {
                    let x = ((r[0] + r[2] * s.ratio) * w).round() - GAP / 2.0;
                    gtk::Allocation::new((x - GRAB / 2.0) as i32, (r[1] * h) as i32, GRAB as i32, (r[3] * h) as i32)
                };
                let _ = child.measure(gtk::Orientation::Horizontal, -1);
                child.size_allocate(&rect, -1);
            }
        }
    }
}

glib::wrapper! {
    pub struct PaneGrid(ObjectSubclass<imp::PaneGrid>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for PaneGrid {
    fn default() -> Self {
        let g: Self = glib::Object::new();
        g.set_hexpand(true);
        g.set_vexpand(true);
        g.set_overflow(gtk::Overflow::Hidden);
        g.wire_drag();
        g
    }
}

impl PaneGrid {
    /// Called while a divider is dragged (throttled) and when it's let go.
    pub fn connect_resize(&self, f: impl Fn(&SplitState, f64) + 'static) {
        *self.imp().on_resize.borrow_mut() = Some(Rc::new(f));
    }

    pub fn contains(&self, w: &impl IsA<gtk::Widget>) -> bool {
        w.parent().as_ref() == Some(self.upcast_ref())
    }

    /// Show exactly these panes at these rects (fractions of the grid).
    pub fn set_panes(&self, panes: Vec<(gtk::Widget, [f64; 4])>) {
        let imp = self.imp();
        for (old, _) in imp.panes.borrow().iter() {
            if !panes.iter().any(|(w, _)| w == old) && old.parent().as_ref() == Some(self.upcast_ref()) {
                old.unparent();
            }
        }
        for (w, _) in &panes {
            if w.parent().as_ref() != Some(self.upcast_ref()) {
                if let Some(p) = w.parent() {
                    w.unparent();
                    p.queue_resize();
                }
                // Panes stay below the dividers.
                match imp.dividers.borrow().first() {
                    Some((d, _)) => w.insert_before(self, Some(d)),
                    None => w.set_parent(self),
                }
            }
        }
        *imp.panes.borrow_mut() = panes;
        self.queue_allocate();
    }

    pub fn set_splits(&self, splits: Vec<SplitState>) {
        let imp = self.imp();
        let same = imp.dividers.borrow().len() == splits.len();
        if same {
            for ((_, s), n) in imp.dividers.borrow_mut().iter_mut().zip(splits) {
                *s = n;
            }
            self.queue_allocate();
            return;
        }
        for (d, _) in imp.dividers.take() {
            d.unparent();
        }
        let mut v = Vec::new();
        for s in splits {
            let d = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            d.add_css_class("raptor-divider");
            d.set_cursor_from_name(Some(if s.vertical { "row-resize" } else { "col-resize" }));
            d.set_parent(self);
            v.push((d.upcast(), s));
        }
        *imp.dividers.borrow_mut() = v;
        self.queue_allocate();
    }

    /// Divider drags are tracked on the grid, whose coordinates stay put
    /// while the divider itself moves under the pointer.
    fn wire_drag(&self) {
        let drag = gtk::GestureDrag::new();
        // (divider index, its ratio when grabbed, when the server was last asked)
        let grab: Rc<RefCell<Option<(usize, f64, Instant)>>> = Rc::default();
        let (g, gr) = (self.downgrade(), grab.clone());
        drag.connect_drag_begin(move |gesture, x, y| {
            let Some(g) = g.upgrade() else { return };
            let hit = g.imp().dividers.borrow().iter().position(|(d, _)| {
                d.compute_bounds(&g).is_some_and(|b| b.contains_point(&gtk::graphene::Point::new(x as f32, y as f32)))
            });
            let Some(i) = hit else {
                gesture.set_state(gtk::EventSequenceState::Denied);
                return;
            };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let divs = g.imp().dividers.borrow();
            divs[i].0.add_css_class("dragging");
            *gr.borrow_mut() = Some((i, divs[i].1.ratio, Instant::now() - Duration::from_secs(1)));
        });
        let report = move |g: &PaneGrid, offset: (f64, f64), last: bool| {
            let Some((index, base, _)) = *grab.borrow() else { return };
            let (w, h) = (g.width() as f64, g.height() as f64);
            let (s, cb) = {
                let mut divs = g.imp().dividers.borrow_mut();
                let Some((d, s)) = divs.get_mut(index) else { return };
                let r = s.rect;
                let ratio = if s.vertical { base + offset.1 / (r[3] * h).max(1.0) } else { base + offset.0 / (r[2] * w).max(1.0) };
                s.ratio = ratio.clamp(0.05, 0.95);
                if last {
                    d.remove_css_class("dragging");
                }
                (s.clone(), g.imp().on_resize.borrow().clone())
            };
            g.queue_allocate();
            // Ask the server as we go, but not more than ~20 times a second.
            let mut gb = grab.borrow_mut();
            let due = gb.as_ref().is_some_and(|x| x.2.elapsed() > Duration::from_millis(50));
            if let Some(x) = gb.as_mut().filter(|_| last || due) {
                x.2 = Instant::now();
                if let Some(cb) = cb {
                    cb(&s, s.ratio);
                }
            }
            if last {
                *gb = None;
            }
        };
        let report = Rc::new(report);
        let (g, r) = (self.downgrade(), report.clone());
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some(g) = g.upgrade() {
                r(&g, (dx, dy), false);
            }
        });
        let g = self.downgrade();
        drag.connect_drag_end(move |_, dx, dy| {
            if let Some(g) = g.upgrade() {
                report(&g, (dx, dy), true);
            }
        });
        self.add_controller(drag);
    }
}
