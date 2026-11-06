//! Engine behaviour against a fake server that misbehaves on cue.

mod fake;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use fake::{Fake, Options};
use raptor_core::*;

struct Quiet;
impl Listener for Quiet {
    fn state_changed(&self) {}
    fn notify(&self, _: Notification) {}
}

fn wait<F: FnMut() -> bool>(secs: f64, mut f: F) -> bool {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    f()
}

#[test]
fn quick_losses_back_off() {
    let fake = Fake::start(Options { drop_on_first_request: true, ..Default::default() });
    let raptor = Raptor::new(Config::default(), Arc::new(Quiet));
    let id = raptor.add_server("", &fake.addr).unwrap();
    std::thread::sleep(Duration::from_secs(3));
    let n = fake.control_connects.load(Ordering::SeqCst);
    // 0.5 s, 1 s, 2 s apart: four tries at most in three seconds.
    assert!((2..=4).contains(&n), "{n} connections in 3 s");
    match &raptor.state().server(&id).unwrap().connectivity {
        Connectivity::Offline { retry_in_ms, .. } => assert!(*retry_in_ms > 0),
        c => panic!("{c:?}"),
    }
}

#[derive(Default)]
struct NoTerm;
impl TerminalListener for NoTerm {
    fn frame_ready(&self) {}
    fn status_changed(&self, _: TerminalStatus) {}
    fn clipboard(&self, _: String) {}
    fn size_changed(&self, _: u16, _: u16) {}
}

fn online(fake: &Fake) -> (Raptor, String) {
    let raptor = Raptor::new(Config::default(), Arc::new(Quiet));
    let id = raptor.add_server("", &fake.addr).unwrap();
    assert!(wait(5.0, || raptor.state().server(&id).unwrap().connectivity == Connectivity::Online));
    (raptor, id)
}

#[test]
fn closed_terminal_is_freed() {
    let fake = Fake::start(Options::default());
    let (raptor, id) = online(&fake);
    let term = raptor.open_terminal(&id, fake::SESSION, fake::BLOCK, Arc::new(NoTerm)).unwrap();
    assert!(wait(5.0, || term.status() == TerminalStatus::Live), "{:?}", term.status());
    term.send_text("x");
    let weak = Arc::downgrade(&term);
    term.close();
    drop(term);
    assert!(wait(2.0, || weak.upgrade().is_none()), "terminal still alive after close");
}

#[test]
fn dropped_terminal_is_freed() {
    let fake = Fake::start(Options::default());
    let (raptor, id) = online(&fake);
    let term = raptor.open_terminal(&id, fake::SESSION, fake::BLOCK, Arc::new(NoTerm)).unwrap();
    assert!(wait(5.0, || term.status() == TerminalStatus::Live));
    let weak = Arc::downgrade(&term);
    drop(term);
    assert!(wait(2.0, || weak.upgrade().is_none()), "a terminal the app let go of stays alive");
}

#[test]
fn close_while_offline_stops_at_once() {
    // Nothing listens here, so the terminal waits for a connection that never comes.
    let raptor = Raptor::new(Config::default(), Arc::new(Quiet));
    let id = raptor.add_server("", "127.0.0.1:9").unwrap();
    let term = raptor.open_terminal(&id, fake::SESSION, fake::BLOCK, Arc::new(NoTerm)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let weak = Arc::downgrade(&term);
    term.close();
    drop(term);
    assert!(wait(1.0, || weak.upgrade().is_none()), "still waiting for a connection after close");
}

#[test]
fn mouse_and_keys_stay_in_order() {
    // Every reply takes a while, like a slow link; input must still arrive in order.
    let slow = Duration::from_millis(80);
    let fake = Fake::start(Options {
        delays: vec![("block.event", slow), ("client.key", slow), ("com.superlogical.terminal.write", slow)],
        ..Default::default()
    });
    let (raptor, id) = online(&fake);
    let term = raptor.open_terminal(&id, fake::SESSION, fake::BLOCK, Arc::new(NoTerm)).unwrap();
    assert!(wait(5.0, || term.status() == TerminalStatus::Live));
    fake.log.lock().clear();
    term.click(3, 4);
    term.send_text("a");
    term.wheel(1, 1, 2.0);
    term.send_key(Key::Up, false, false, false, false);
    term.paste("p");
    let input = |f: &Fake| {
        f.log
            .lock()
            .iter()
            .filter_map(|(m, p)| {
                let what = match m.as_str() {
                    "block.event" => p.pointer("/event/payload/action").or(p.pointer("/event/type"))?.as_str()?.to_owned(),
                    "client.key" => format!("key {}", p.pointer("/key/action")?.as_str()?),
                    "com.superlogical.terminal.write" => format!("write {}", p.pointer("/args/data")?.as_str()?),
                    _ => return None,
                };
                Some(what)
            })
            .collect::<Vec<_>>()
    };
    assert!(wait(5.0, || input(&fake).len() == 7), "{:?}", input(&fake));
    assert_eq!(input(&fake), ["press", "release", "write a", "wheel", "key press", "key release", "paste"]);
}

#[test]
fn held_arrow_keys_pipeline() {
    // 50 ms per reply: one round trip per key would take a second for ten keys.
    let fake = Fake::start(Options { delays: vec![("client.key", Duration::from_millis(50))], ..Default::default() });
    let (raptor, id) = online(&fake);
    let term = raptor.open_terminal(&id, fake::SESSION, fake::BLOCK, Arc::new(NoTerm)).unwrap();
    assert!(wait(5.0, || term.status() == TerminalStatus::Live));
    fake.log.lock().clear();
    let t = Instant::now();
    for _ in 0..10 {
        term.send_key(Key::Up, false, false, false, false);
    }
    assert!(wait(5.0, || fake.methods().iter().filter(|m| *m == "client.key").count() == 20));
    let took = t.elapsed();
    eprintln!("10 keys reached the server in {took:?}");
    assert!(took < Duration::from_millis(300), "{took:?}");
}

#[test]
fn unavailable_stream_ends() {
    let fake = Fake::start(Options { data_reject: Some(0x03), ..Default::default() });
    let (raptor, id) = online(&fake);
    let term = raptor.open_terminal(&id, fake::SESSION, fake::BLOCK, Arc::new(NoTerm)).unwrap();
    assert!(wait(3.0, || matches!(term.status(), TerminalStatus::Ended { .. })), "{:?}", term.status());
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(fake.data_connects.load(Ordering::SeqCst), 1);
}

#[test]
fn refused_streams_back_off() {
    // "Already connected": a fresh ticket may work later, but not in a tight loop.
    let fake = Fake::start(Options { data_reject: Some(0x02), ..Default::default() });
    let (raptor, id) = online(&fake);
    let _term = raptor.open_terminal(&id, fake::SESSION, fake::BLOCK, Arc::new(NoTerm)).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let n = fake.data_connects.load(Ordering::SeqCst);
    assert!((2..=6).contains(&n), "{n} data connections in 2 s");
}

fn session(raptor: &Raptor, id: &str) -> Option<SessionState> {
    raptor.state().server(id)?.sessions.iter().find(|s| s.id == fake::SESSION).cloned()
}

#[test]
fn short_lived_client_leaves_no_ghost() {
    // The CLI attaches and leaves in a blink; its inspect reply lands after it left.
    let fake = Fake::start(Options { delays: vec![("client.inspect", Duration::from_millis(150))], ..Default::default() });
    let (raptor, id) = online(&fake);
    assert!(wait(2.0, || fake.methods().contains(&"session.list_clients".to_owned())));
    std::thread::sleep(Duration::from_millis(100));
    let principal = serde_json::json!({"kind": "peer", "display": "uid 1000"});
    fake.emit_session("client_connected", serde_json::json!({"client_id": "client:cli", "principal": principal, "connected_at": "now"}));
    fake.emit_session("client_disconnected", serde_json::json!({"client_id": "client:cli", "principal": principal}));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(session(&raptor, &id).unwrap().clients, Vec::<String>::new());
}

#[test]
fn client_label_changes_show() {
    let fake = Fake::start(Options::default());
    let (raptor, id) = online(&fake);
    assert!(wait(2.0, || fake.methods().contains(&"session.list_clients".to_owned())));
    std::thread::sleep(Duration::from_millis(100));
    let principal = serde_json::json!({"kind": "peer", "display": "uid 1000"});
    fake.emit_session("client_connected", serde_json::json!({"client_id": "client:mac", "principal": principal, "connected_at": "now"}));
    assert!(wait(2.0, || session(&raptor, &id).unwrap().clients == ["Inspected · someone"]), "{:?}", session(&raptor, &id));
    fake.emit(serde_json::json!({"type": "client_changed", "payload": {"client": {"client_id": "client:mac",
        "info": {"name": "Rex.app", "label": "studio"}, "principal": principal, "connected_at": "now"}}}));
    assert!(wait(2.0, || session(&raptor, &id).unwrap().clients == ["studio · uid 1000"]), "{:?}", session(&raptor, &id));
}

#[derive(Default)]
struct Clip(parking_lot::Mutex<Vec<String>>);
impl TerminalListener for Clip {
    fn frame_ready(&self) {}
    fn status_changed(&self, _: TerminalStatus) {}
    fn clipboard(&self, text: String) {
        self.0.lock().push(text);
    }
    fn size_changed(&self, _: u16, _: u16) {}
}

#[test]
fn large_clipboard_is_read() {
    let fake = Fake::start(Options::default());
    let (raptor, id) = online(&fake);
    let clip = Arc::new(Clip::default());
    let term = raptor.open_terminal(&id, fake::SESSION, fake::BLOCK, clip.clone()).unwrap();
    assert!(wait(5.0, || term.status() == TerminalStatus::Live));
    fake.emit_block("clipboard_written", serde_json::json!({"text": "small"}));
    // Too big for an event: only its id and size come along.
    fake.emit_block("clipboard_written", serde_json::json!({"id": 5, "bytes": 2_000_000}));
    assert!(wait(2.0, || clip.0.lock().len() == 2), "{:?}", clip.0.lock());
    assert_eq!(*clip.0.lock(), ["small", "copied in full"]);
}

#[test]
fn online_means_attached() {
    // Until session.attach answers, the server refuses input for the session.
    let fake = Fake::start(Options { delays: vec![("session.attach", Duration::from_millis(300))], ..Default::default() });
    let raptor = Raptor::new(Config::default(), Arc::new(Quiet));
    let id = raptor.add_server("", &fake.addr).unwrap();
    assert!(wait(5.0, || raptor.state().server(&id).unwrap().connectivity == Connectivity::Online));
    raptor.block_on(raptor.send_text(&id, fake::SESSION, fake::BLOCK, "y")).expect("input right after going online");
}

#[test]
fn reconnect_attaches_in_bootstrap() {
    let fake = Fake::start(Options::default());
    let (raptor, id) = online(&fake);
    assert!(wait(2.0, || session(&raptor, &id).is_some_and(|s| !s.windows.is_empty())));
    fake.log.lock().clear();
    raptor.disconnect_for_test(&id);
    assert!(wait(5.0, || fake.methods().contains(&"session.bootstrap".to_owned())));
    assert!(wait(5.0, || raptor.state().server(&id).unwrap().connectivity == Connectivity::Online));
    let log = fake.log.lock();
    let boot = &log.iter().find(|(m, _)| m == "session.bootstrap").unwrap().1;
    assert_eq!(boot["attach_session_ids"], serde_json::json!([fake::SESSION]), "known sessions attach in the first round trip");
    assert!(!log.iter().any(|(m, _)| m == "session.attach"), "{:?}", log.iter().map(|x| &x.0).collect::<Vec<_>>());
}

#[test]
fn view_changes_coalesce() {
    let fake = Fake::start(Options { delays: vec![("session.view", Duration::from_millis(100))], ..Default::default() });
    let (raptor, id) = online(&fake);
    assert!(wait(2.0, || session(&raptor, &id).is_some_and(|s| !s.windows.is_empty())));
    fake.log.lock().clear();
    for rev in 2..30 {
        fake.emit_session("session_view_changed", serde_json::json!({"revision": rev}));
    }
    std::thread::sleep(Duration::from_millis(800));
    let n = fake.methods().iter().filter(|m| *m == "session.view").count();
    assert!((1..=2).contains(&n), "{n} view reads for one burst of changes");
}

#[test]
fn process_labels() {
    let fake = Fake::start(Options::default());
    let (raptor, id) = online(&fake);
    let name = || session(&raptor, &id).and_then(|s| s.windows.first()?.blocks.first().map(|b| b.process.clone()));
    fake.emit_block("process_changed", serde_json::json!({"foreground": {"pid": 1, "user_id": 1, "name": "python3.12",
        "argv0": "/usr/bin/python3", "script": "/home/me/manage.py"}}));
    assert!(wait(2.0, || name().as_deref() == Some("manage.py")), "{:?}", name());
    fake.emit_block("process_changed", serde_json::json!({"foreground": {"pid": 2, "user_id": 1, "name": "zsh", "argv0": "-zsh"}}));
    assert!(wait(2.0, || name().as_deref() == Some("zsh")), "{:?}", name());
    fake.emit_block("process_changed", serde_json::json!({"foreground": {"pid": 3, "user_id": 1, "name": "node", "argv0": "claude"}}));
    assert!(wait(2.0, || name().as_deref() == Some("claude")), "{:?}", name());
}

#[test]
fn huge_paste_fits_the_message_limit() {
    let fake = Fake::start(Options::default());
    let (raptor, id) = online(&fake);
    let term = raptor.open_terminal(&id, fake::SESSION, fake::BLOCK, Arc::new(NoTerm)).unwrap();
    assert!(wait(5.0, || term.status() == TerminalStatus::Live));
    fake.log.lock().clear();
    // 3 MB of log with control characters, which JSON escapes six times over.
    let text: String = (0..60_000).map(|i| format!("line {i} é\u{1}\n")).collect();
    term.paste(&text);
    term.send_text(&text);
    let got = |kind: &str| -> String {
        fake.log
            .lock()
            .iter()
            .filter_map(|(m, p)| match (m.as_str(), kind) {
                ("block.event", "paste") => p.pointer("/event/payload/text")?.as_str().map(str::to_owned),
                ("com.superlogical.terminal.write", "write") => p.pointer("/args/data")?.as_str().map(str::to_owned),
                _ => None,
            })
            .collect()
    };
    assert!(wait(10.0, || got("write").len() == text.len()), "{} of {}", got("write").len(), text.len());
    assert_eq!(got("paste"), text);
    assert_eq!(got("write"), text);
}
