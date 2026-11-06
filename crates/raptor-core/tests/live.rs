//! End to end against a real Rex server. Skipped unless RAPTOR_TEST_SERVER is set.
//! Point it at an isolated test server, never one with real work in it:
//!
//! ```sh
//! rex server run --listen unix://$DIR/rex.sock --extra-listen http://127.0.0.1:7984 \
//!   --allow-anonymous --state-dir $DIR/state --restore=false --config /dev/null
//! RAPTOR_TEST_SERVER=127.0.0.1:7984 cargo test -p raptor-core --test live -- --nocapture
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use raptor_client::{ConnectOptions, Connection};
use raptor_core::*;
use serde_json::json;

#[derive(Default)]
struct Notes(Mutex<Vec<Notification>>);
impl Listener for Notes {
    fn state_changed(&self) {}
    fn notify(&self, n: Notification) {
        self.0.lock().push(n);
    }
}

#[derive(Default)]
struct T {
    frames: AtomicUsize,
    statuses: Mutex<Vec<TerminalStatus>>,
    clipboard: Mutex<Vec<String>>,
}
impl TerminalListener for T {
    fn frame_ready(&self) {
        self.frames.fetch_add(1, Ordering::Relaxed);
    }
    fn status_changed(&self, s: TerminalStatus) {
        self.statuses.lock().push(s);
    }
    fn clipboard(&self, text: String) {
        self.clipboard.lock().push(text);
    }
    fn size_changed(&self, _: u16, _: u16) {}
}

impl T {
    /// Statuses after the terminal first went live.
    fn after_live(&self) -> Vec<TerminalStatus> {
        let s = self.statuses.lock();
        let first = s.iter().position(|x| *x == TerminalStatus::Live).unwrap_or(s.len());
        s[first..].to_vec()
    }
}

/// Tests that check a stream resumes byte for byte run one at a time: a busy
/// test server sometimes ends a dropped client instead of keeping it.
static RESUME: Mutex<()> = Mutex::new(());

fn wait<F: FnMut() -> bool>(secs: u64, mut f: F) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    f()
}

fn screen(t: &TerminalSession) -> String {
    t.frame().lines.iter().map(|r| r.runs.iter().map(|x| x.text.as_str()).collect::<String>().trim_end().to_owned() + "\n").collect()
}

/// One Raptor connected to the test server, with a fresh session that is
/// destroyed when the test ends.
struct Live {
    raptor: Raptor,
    notes: Arc<Notes>,
    addr: String,
    id: String,
    sid: String,
    bid: String,
}

impl Live {
    fn new() -> Option<Self> {
        Self::via(None)
    }

    /// Connect through `via` (e.g. a proxy) while the session is made directly.
    fn via(via: Option<&str>) -> Option<Self> {
        let Ok(addr) = std::env::var("RAPTOR_TEST_SERVER") else {
            eprintln!("RAPTOR_TEST_SERVER not set; skipping");
            return None;
        };
        let notes = Arc::new(Notes::default());
        let raptor = Raptor::new(Config::default(), notes.clone());
        let id = raptor.add_server("", via.unwrap_or(&addr)).unwrap();
        assert!(wait(10, || raptor.state().server(&id).unwrap().connectivity == Connectivity::Online), "never online");
        let label = format!("raptor-it-{}-{:?}", std::process::id(), std::thread::current().id());
        let (sid, bid) = raptor.block_on(raptor.create_session(&id, Some(&label), None)).unwrap();
        let live = Self { raptor, notes, addr, id, sid, bid: bid.unwrap() };
        assert!(wait(10, || live.session().is_some_and(|s| s.block_count() == 1)), "session never showed up");
        Some(live)
    }

    fn session(&self) -> Option<SessionState> {
        self.raptor.state().server(&self.id)?.sessions.iter().find(|s| s.id == self.sid).cloned()
    }

    fn block(&self, bid: &str) -> Option<BlockState> {
        self.session()?.windows.iter().flat_map(|w| &w.blocks).find(|b| b.id == bid).cloned()
    }

    fn open(&self, bid: &str) -> (Arc<TerminalSession>, Arc<T>) {
        let tl = Arc::new(T::default());
        let term = self.raptor.open_terminal(&self.id, &self.sid, bid, tl.clone()).unwrap();
        assert!(wait(10, || term.status() == TerminalStatus::Live), "never went live: {:?}", tl.statuses.lock());
        // Wait for the shell's first prompt so typing isn't eaten by its startup.
        assert!(wait(10, || screen(&term).trim().len() > 1), "no prompt");
        (term, tl)
    }

    /// A second client, straight to the server, attached to this session.
    fn side(&self) -> Connection {
        let ep = Endpoint::parse(&self.addr).unwrap();
        let sid = self.sid.clone();
        self.raptor.block_on(async move {
            let (c, _ev) = Connection::connect(&ep, ConnectOptions::default()).await.unwrap();
            c.attach(&sid).await.unwrap();
            c
        })
    }

    fn type_line(&self, term: &TerminalSession, line: &str) {
        term.send_text(line);
        term.send_key(Key::Enter, false, false, false, false);
    }

    /// Write a shell script to a temp file and return its path.
    fn script(&self, name: &str, body: &str) -> String {
        let dir = std::env::temp_dir().join(format!("raptor-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p.display().to_string()
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.raptor.block_on(self.raptor.destroy_session(&self.id, &self.sid));
    }
}

#[test]
fn sessions_and_windows() {
    let Some(l) = Live::new() else { return };
    let r = &l.raptor;
    r.block_on(r.rename_session(&l.id, &l.sid, "renamed-by-test")).unwrap();
    assert!(wait(5, || l.session().is_some_and(|s| s.label == "renamed-by-test")));

    let first = l.session().unwrap().windows[0].id.clone();
    r.block_on(r.new_window(&l.id, &l.sid, None)).unwrap();
    assert!(wait(5, || l.session().is_some_and(|s| s.windows.len() == 2)));
    let s = l.session().unwrap();
    let second = s.windows[1].id.clone();
    assert!(wait(5, || l.session().unwrap().active_window_id.as_deref() == Some(second.as_str())), "new window isn't active");

    r.block_on(r.rename_window(&l.id, &l.sid, &second, "logs")).unwrap();
    assert!(wait(5, || l.session().unwrap().windows[1].label == "logs"));
    r.block_on(r.focus_window(&l.id, &l.sid, &first)).unwrap();
    assert!(wait(5, || l.session().unwrap().active_window_id.as_deref() == Some(first.as_str())));
    r.block_on(r.close_window(&l.id, &l.sid, &second)).unwrap();
    assert!(wait(5, || l.session().unwrap().windows.len() == 1));

    r.block_on(r.destroy_session(&l.id, &l.sid)).unwrap();
    assert!(wait(5, || l.session().is_none()), "destroyed session still listed");
}

#[test]
fn splits_zoom_focus() {
    let Some(l) = Live::new() else { return };
    let r = &l.raptor;
    let right = r.block_on(r.split(&l.id, &l.sid, &l.bid, false, None)).unwrap().unwrap();
    assert!(wait(5, || l.session().unwrap().block_count() == 2));
    let (a, b) = (l.block(&l.bid).unwrap(), l.block(&right).unwrap());
    assert!(a.rect[0] < 0.01 && (b.rect[0] - 0.5).abs() < 0.01, "side by side: {:?} {:?}", a.rect, b.rect);
    let below = r.block_on(r.split(&l.id, &l.sid, &right, true, None)).unwrap().unwrap();
    assert!(wait(5, || l.session().unwrap().block_count() == 3));
    assert!(wait(5, || l.block(&below).is_some_and(|b| b.rect[1] > 0.4)), "{:?}", l.block(&below));

    let window = || l.session().unwrap().windows[0].clone();
    assert!(wait(5, || window().focused_block_id.as_deref() == Some(below.as_str())), "split focuses the new block");
    r.block_on(r.focus_block(&l.id, &l.sid, &l.bid)).unwrap();
    assert!(wait(5, || window().focused_block_id.as_deref() == Some(l.bid.as_str())));
    r.block_on(r.zoom_block(&l.id, &l.sid, &right)).unwrap();
    assert!(wait(5, || window().zoomed_block_id.as_deref() == Some(right.as_str())));
    r.block_on(r.zoom_block(&l.id, &l.sid, &right)).unwrap();
    assert!(wait(5, || window().zoomed_block_id.is_none()));

    r.block_on(r.close_block(&l.id, &l.sid, &below)).unwrap();
    assert!(wait(5, || l.session().unwrap().block_count() == 2 && l.block(&below).is_none()));
}

#[test]
fn typing_keys_and_unicode() {
    let Some(l) = Live::new() else { return };
    let (term, tl) = l.open(&l.bid);

    let t = Instant::now();
    l.type_line(&term, "printf 'raptor-%s\\n' 42");
    assert!(wait(10, || screen(&term).contains("raptor-42\n")), "{}", screen(&term));
    eprintln!("echo round trip {:?}, {} frames", t.elapsed(), tl.frames.load(Ordering::Relaxed));

    // Ctrl+C interrupts, and typing right after it lands after it.
    l.type_line(&term, "sleep 30");
    // A ^C that reaches the shell before sleep starts also flushes the typed-ahead line.
    assert!(wait(10, || l.block(&l.bid).is_some_and(|b| b.process == "sleep")), "{:?}", l.block(&l.bid));
    term.send_key(Key::Char('c'), true, false, false, false);
    l.type_line(&term, "echo after-ctrl-c");
    assert!(wait(10, || screen(&term).contains("after-ctrl-c\n")), "{}", screen(&term));

    // Backspace and arrows go through the server's key handling in order.
    term.send_text("echo abX");
    term.send_key(Key::Backspace, false, false, false, false);
    term.send_text("c");
    term.send_key(Key::Enter, false, false, false, false);
    assert!(wait(10, || screen(&term).contains("\nabc\n")), "{}", screen(&term));
    term.send_key(Key::Up, false, false, false, false);
    term.send_key(Key::Left, false, false, false, false);
    term.send_text("d");
    term.send_key(Key::Enter, false, false, false, false);
    assert!(wait(10, || screen(&term).contains("\nabdc\n")), "{}", screen(&term));

    // Wide and combining characters.
    l.type_line(&term, "echo 'wide:你好🙂 e\u{301}nd'");
    assert!(wait(10, || screen(&term).contains("\nwide:你好🙂 e\u{301}nd\n")), "{}", screen(&term));
    let f = term.frame();
    let run = f.lines.iter().flat_map(|r| &r.runs).find(|r| r.text.starts_with("wide:你") && r.col == 0).expect("wide run");
    let w: Vec<u8> = run.widths.iter().copied().take(9).collect();
    assert_eq!(w, [1, 1, 1, 1, 1, 2, 2, 2, 1], "{:?}", run.text);
    assert!(run.text.contains("e\u{301}"), "combining mark kept with its base: {:?}", run.text);
}

#[test]
fn bracketed_paste() {
    let Some(l) = Live::new() else { return };
    let (term, _) = l.open(&l.bid);
    l.type_line(&term, "printf '\\033[?2004h'; command cat -v");
    assert!(wait(10, || l.block(&l.bid).is_some_and(|b| b.process == "cat")), "{:?}", l.block(&l.bid));
    term.paste("pasted text");
    term.send_key(Key::Enter, false, false, false, false);
    assert!(wait(10, || screen(&term).contains("^[[200~pasted text^[[201~")), "{}", screen(&term));
    term.send_key(Key::Char('c'), true, false, false, false);
}

#[test]
fn resize_while_owning() {
    let Some(l) = Live::new() else { return };
    let (term, _) = l.open(&l.bid);
    term.set_device_grid(100, 30, 9, 18);
    term.take_control();
    assert!(wait(10, || term.size() == (100, 30)), "size {:?}", term.size());
    assert!(wait(5, || l.block(&l.bid).is_some_and(|b| b.owned_by_me)), "{:?}", l.block(&l.bid));
    l.type_line(&term, "stty size");
    assert!(wait(10, || screen(&term).contains("\n30 100\n")), "{}", screen(&term));
    // The keyboard slides in: many sizes in a row settle on the last.
    for rows in (20..30).rev() {
        term.set_device_grid(100, rows, 9, 18);
    }
    assert!(wait(10, || term.size() == (100, 20)), "size {:?}", term.size());
    term.release_control();
    assert!(wait(5, || l.block(&l.bid).is_some_and(|b| !b.owned_by_me)));
}

#[test]
fn agent_status_bell_and_exit() {
    let Some(l) = Live::new() else { return };
    let (term, _) = l.open(&l.bid);
    let osc = l.script(
        "agent.sh",
        "printf '\\033]7501;state=working:app=tester\\033\\\\'\nsleep 0.3\nprintf '\\033]7501;state=blocked:kind=permission:app=tester:msg=aGk=\\033\\\\'\nread x\nprintf '\\033]7501;state=done:app=tester\\033\\\\'\nread x\n",
    );
    l.type_line(&term, &format!("sh {osc}"));
    assert!(
        wait(10, || l.block(&l.bid).and_then(|b| b.agent).is_some_and(|a| a.state == AgentState::Blocked)),
        "{:?}",
        l.block(&l.bid)
    );
    let a = l.block(&l.bid).unwrap().agent.unwrap();
    assert_eq!((a.app.as_str(), a.waiting_for.as_deref(), a.message.as_str()), ("tester", Some("permission"), "hi"));
    let kinds = |k: NotificationKind| l.notes.0.lock().iter().filter(|n| n.kind == k && n.block_id == l.bid).count();
    assert!(wait(5, || kinds(NotificationKind::AgentBlocked) == 1));
    term.send_key(Key::Enter, false, false, false, false);
    assert!(wait(10, || kinds(NotificationKind::AgentDone) == 1), "{:?}", l.notes.0.lock());
    term.send_key(Key::Enter, false, false, false, false);
    assert!(wait(10, || l.block(&l.bid).and_then(|b| b.agent).is_some_and(|a| a.state == AgentState::Done)));

    l.type_line(&term, "printf '\\a'; sleep 0.5; printf '\\a'");
    assert!(wait(10, || kinds(NotificationKind::Bell) >= 1));
    assert!(l.block(&l.bid).unwrap().has_bell);

    l.type_line(&term, "exit 3");
    assert!(wait(10, || kinds(NotificationKind::ProcessExited) == 1), "{:?}", l.notes.0.lock());
    let n = l.notes.0.lock().iter().find(|n| n.kind == NotificationKind::ProcessExited).cloned().unwrap();
    assert!(n.body.contains('3'), "{n:?}");
    assert!(
        wait(10, || matches!(term.status(), TerminalStatus::Exited { code: 3 } | TerminalStatus::Ended { .. })),
        "{:?}",
        term.status()
    );
    // Nothing above should have been announced twice.
    let notes = l.notes.0.lock();
    for k in [NotificationKind::AgentBlocked, NotificationKind::AgentDone, NotificationKind::ProcessExited] {
        assert!(notes.iter().filter(|n| n.kind == k).count() <= 1, "{k:?} twice: {notes:?}");
    }
}

#[test]
fn server_theme_survives_local_theme() {
    let Some(l) = Live::new() else { return };
    let (term, _) = l.open(&l.bid);
    let side = l.side();
    let (sid, bid) = (l.sid.clone(), l.bid.clone());
    l.raptor.block_on(async move {
        side.own_block(&sid, &bid, true).await.unwrap();
        side.terminal(&sid, &bid, "set_theme", json!({"foreground": "#c0c0c0", "background": "#102030", "palette": ["1=#ff0000"]}))
            .await
            .unwrap();
    });
    let bg = |t: &TerminalSession| t.frame().background;
    assert!(wait(10, || bg(&term) == Rgb::new(0x10, 0x20, 0x30)), "{:?}", bg(&term));
    l.type_line(&term, "printf '\\033[31mRED\\033[0m\\n'");
    let red = |t: &TerminalSession| t.frame().lines.iter().flat_map(|r| r.runs.clone()).find(|r| r.text.starts_with("RED")).map(|r| r.fg);
    assert!(wait(10, || red(&term).is_some()));
    assert_eq!(red(&term), Some(Rgb::new(0xff, 0, 0)), "server palette applies");

    let white = Rgb::new(0xff, 0xff, 0xff);
    term.set_theme(Some(ColorTheme { foreground: Rgb::new(0, 0, 0), background: white, cursor: white, ansi: [white; 16] }));
    assert_eq!(bg(&term), white);
    term.set_theme(None);
    assert_eq!(bg(&term), Rgb::new(0x10, 0x20, 0x30), "back to the server's colors");
    assert_eq!(red(&term), Some(Rgb::new(0xff, 0, 0)));
}

#[test]
fn palette_actions() {
    let Some(l) = Live::new() else { return };
    let (term, _) = l.open(&l.bid);
    l.type_line(&term, "echo before-clear");
    assert!(wait(10, || screen(&term).contains("before-clear\n")));
    // The inbox's preview reads the server's copy of the screen.
    let preview = || l.raptor.block_on(l.raptor.preview_text(&l.id, &l.sid, &l.bid, 64 * 1024)).unwrap();
    assert!(wait(5, || preview().contains("\nbefore-clear")), "{:?}", preview());
    let actions = l.raptor.block_on(l.raptor.palette_actions(&l.id)).unwrap();
    let clear = actions.iter().find(|a| a.name == "com.superlogical.terminal.clear").expect("clear action");
    l.raptor.block_on(l.raptor.run_action(&l.id, &l.sid, Some(&l.bid), &clear.name)).unwrap();
    assert!(wait(5, || !screen(&term).contains("before-clear")), "{}", screen(&term));
}

#[test]
fn reply_without_live_connection() {
    let Some(l) = Live::new() else { return };
    let (term, _) = l.open(&l.bid);
    // A request with no usable live connection (dropped, or dead after the app
    // was frozen in the background) goes over a short-lived connection instead.
    l.raptor.set_foreground(false);
    l.raptor.disconnect_for_test(&l.id);
    l.raptor.block_on(l.raptor.send_text(&l.id, &l.sid, &l.bid, "echo bg-reply\r")).expect("reply without a live connection");
    l.raptor.set_foreground(true);
    assert!(wait(15, || screen(&term).contains("bg-reply")), "{}", screen(&term));
}

#[test]
fn background_resumes_streams() {
    let _one = RESUME.lock();
    let Some(l) = Live::new() else { return };
    let (term, tl) = l.open(&l.bid);
    l.raptor.set_foreground(false);
    assert!(wait(5, || l.raptor.state().server(&l.id).unwrap().connectivity == Connectivity::Idle));
    let side = l.side();
    let (sid, bid) = (l.sid.clone(), l.bid.clone());
    l.raptor.block_on(async move { side.write(&sid, &bid, "echo while-away\r").await.unwrap() });
    std::thread::sleep(Duration::from_millis(500));
    l.raptor.set_foreground(true);
    assert!(wait(10, || l.raptor.state().server(&l.id).unwrap().connectivity == Connectivity::Online));
    assert!(wait(10, || screen(&term).contains("\nwhile-away\n")), "{}", screen(&term));
    l.type_line(&term, "echo back");
    assert!(wait(10, || screen(&term).contains("\nback\n")), "{}", screen(&term));
    let after = tl.after_live();
    assert!(!after.contains(&TerminalStatus::Connecting), "stream restarted instead of resuming: {after:?}");
}

#[test]
fn forced_disconnect_keeps_output() {
    let _one = RESUME.lock();
    let Some(l) = Live::new() else { return };
    let (term, tl) = l.open(&l.bid);
    l.raptor.disconnect_for_test(&l.id);
    l.type_line(&term, "echo typed-across-reconnect");
    assert!(wait(15, || screen(&term).contains("\ntyped-across-reconnect\n")), "{}", screen(&term));
    assert!(wait(10, || l.raptor.state().server(&l.id).unwrap().connectivity == Connectivity::Online));
    let after = tl.after_live();
    assert!(!after.contains(&TerminalStatus::Connecting), "stream restarted: {after:?}");
}

/// A TCP proxy whose connections can be cut without a close frame, like a
/// phone losing its network.
struct Proxy {
    addr: String,
    down: Arc<AtomicBool>,
    conns: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    _rt: tokio::runtime::Runtime,
}

impl Proxy {
    fn start(target: &str) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let listener = rt.block_on(tokio::net::TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let down = Arc::new(AtomicBool::new(false));
        let conns = Arc::new(Mutex::new(Vec::new()));
        let (d, c, target) = (down.clone(), conns.clone(), target.to_owned());
        rt.spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                if d.load(Ordering::SeqCst) {
                    continue;
                }
                let target = target.clone();
                c.lock().push(tokio::spawn(async move {
                    if let Ok(mut out) = tokio::net::TcpStream::connect(&target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut out).await;
                    }
                }));
            }
        });
        Self { addr, down, conns, _rt: rt }
    }

    fn cut(&self) {
        self.down.store(true, Ordering::SeqCst);
        for h in self.conns.lock().drain(..) {
            h.abort();
        }
    }

    fn restore(&self) {
        self.down.store(false, Ordering::SeqCst);
    }
}

#[test]
fn network_loss_resumes_byte_exact() {
    let _one = RESUME.lock();
    let Ok(target) = std::env::var("RAPTOR_TEST_SERVER") else { return };
    let proxy = Proxy::start(&target);
    let Some(l) = Live::via(Some(&proxy.addr)) else { return };
    let (term, tl) = l.open(&l.bid);
    l.type_line(&term, "echo before-loss");
    assert!(wait(10, || screen(&term).contains("\nbefore-loss\n")));

    proxy.cut();
    // Output keeps coming while we're away; all of it must arrive, once.
    let side = l.side();
    let (sid, bid) = (l.sid.clone(), l.bid.clone());
    l.raptor.block_on(async move { side.write(&sid, &bid, "printf 'away-%s\\n' 1 2 3 4 5\r").await.unwrap() });
    assert!(wait(10, || l.raptor.state().server(&l.id).unwrap().connectivity != Connectivity::Online));
    std::thread::sleep(Duration::from_secs(1));
    proxy.restore();

    assert!(
        wait(20, || screen(&term).contains("\naway-5\n")),
        "{}\n{:?} {:?}",
        screen(&term),
        tl.statuses.lock(),
        l.raptor.state().server(&l.id).unwrap().connectivity
    );
    let s = screen(&term);
    for i in 1..=5 {
        assert_eq!(s.matches(&format!("\naway-{i}\n")).count(), 1, "{s}");
    }
    assert!(wait(20, || l.raptor.state().server(&l.id).unwrap().connectivity == Connectivity::Online));
    l.type_line(&term, "echo after-loss");
    assert!(wait(10, || screen(&term).contains("\nafter-loss\n")), "{}", screen(&term));
    let after = tl.after_live();
    assert!(after.contains(&TerminalStatus::Reconnecting), "{after:?}");
    assert!(!after.contains(&TerminalStatus::Connecting), "stream restarted instead of resuming: {after:?}");
}


#[test]
fn control_survives_a_new_client() {
    let Some(l) = Live::new() else { return };
    let (term, _) = l.open(&l.bid);
    term.set_device_grid(90, 28, 9, 18);
    term.take_control();
    assert!(wait(10, || term.size() == (90, 28)), "size {:?}", term.size());
    // Saving the server again reconnects as a brand-new client, which owns nothing.
    let addr = l.raptor.state().server(&l.id).unwrap().address.clone();
    l.raptor.update_server(&l.id, "", &addr).unwrap();
    assert!(wait(10, || l.raptor.state().server(&l.id).unwrap().connectivity == Connectivity::Online));
    assert!(wait(10, || l.block(&l.bid).is_some_and(|b| b.owned_by_me)), "{:?}", l.block(&l.bid));
    assert!(wait(10, || term.status() == TerminalStatus::Live));
    // Someone else resizes; ours wins again because we still hold control.
    let side = l.side();
    let (sid, bid) = (l.sid.clone(), l.bid.clone());
    l.raptor.block_on(async move { side.resize(&sid, &bid, 70, 20, None).await.unwrap() });
    std::thread::sleep(Duration::from_millis(300));
    assert!(wait(3, || term.size() == (90, 28)), "{:?}", term.size());
}

#[test]
fn take_then_release_quickly() {
    let Some(l) = Live::new() else { return };
    let (term, _) = l.open(&l.bid);
    term.set_device_grid(90, 28, 9, 18);
    for _ in 0..5 {
        term.take_control();
        term.release_control();
    }
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!term.has_control());
    assert!(wait(5, || l.block(&l.bid).is_some_and(|b| !b.owned_by_me)), "{:?}", l.block(&l.bid));
    term.take_control();
    assert!(wait(10, || l.block(&l.bid).is_some_and(|b| b.owned_by_me)), "{:?}", l.block(&l.bid));
}

#[test]
fn floating_layers_minimize() {
    let Some(l) = Live::new() else { return };
    let side = l.side();
    let w = side.welcome();
    assert_eq!(w.api_revision, w.api_server_revision.min(raptor_client::proto::API_REVISION), "newest shared revision");
    let sid = l.sid.clone();
    let float = l.raptor.block_on(async move {
        let v = side
            .call(
                "session.new_layer",
                json!({"session_id": sid, "bounds": [0.1, 0.1, 0.5, 0.5],
                    "layout": {"block": {"flavor": "com.superlogical.terminal.shell"}}}),
            )
            .await
            .unwrap();
        v["block_ids"][0].as_str().unwrap().to_owned()
    });
    assert!(wait(10, || l.block(&float).is_some_and(|b| b.floating && !b.minimized)), "{:?}", l.block(&float));
    let b = l.block(&float).unwrap();
    assert_eq!(b.rect, [0.1, 0.1, 0.5, 0.5]);
    l.raptor.block_on(l.raptor.set_layer_minimized(&l.id, &l.sid, &b.layer_id, true)).unwrap();
    assert!(wait(10, || l.block(&float).is_some_and(|b| b.minimized)), "{:?}", l.block(&float));
    assert!(!l.block(&l.bid).unwrap().minimized, "the tiled layer stays");
    l.raptor.block_on(l.raptor.set_layer_minimized(&l.id, &l.sid, &b.layer_id, false)).unwrap();
    assert!(wait(10, || l.block(&float).is_some_and(|b| !b.minimized)));
}

#[test]
fn verify_leaves_healthy_connections_alone() {
    let Some(l) = Live::new() else { return };
    let (term, tl) = l.open(&l.bid);
    l.raptor.verify_connections();
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(l.raptor.state().server(&l.id).unwrap().connectivity, Connectivity::Online);
    assert_eq!(tl.after_live(), [TerminalStatus::Live], "a healthy stream was dropped");
    l.type_line(&term, "echo still-here");
    assert!(wait(10, || screen(&term).contains("\nstill-here\n")), "{}", screen(&term));
}

#[test]
fn find_in_scrollback() {
    let Some(l) = Live::new() else { return };
    let (term, _) = l.open(&l.bid);
    let script = l.script("lines", "for i in $(seq 1 200); do case $i in 20|150) echo \"line $i Needle\";; *) echo \"line $i\";; esac; done; echo done-$((6*7))");
    l.type_line(&term, &format!("sh {script}"));
    assert!(wait(10, || screen(&term).contains("done-42\n")), "{}", screen(&term));
    let bottom = term.frame().scrollbar.offset;

    assert!(term.find("needle", true));
    assert_eq!(term.selection_text().as_deref(), Some("Needle"));
    assert!(screen(&term).contains("line 150 Needle"));
    assert!(term.find("needle", true));
    assert!(screen(&term).contains("line 20 Needle"), "{}", screen(&term));
    let there = term.frame().scrollbar.offset;
    assert!(there < bottom);
    assert!(!term.find("needle", true), "no older match");
    assert_eq!(term.frame().scrollbar.offset, there, "a miss leaves the view where it was");

    assert!(term.find("NEEDLE", false));
    assert!(screen(&term).contains("line 150 Needle"));
    term.reset_find();
    assert!(term.selection_text().is_none());
    assert!(!term.find("not on screen anywhere", true));
}
