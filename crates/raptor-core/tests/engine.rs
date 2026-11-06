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
