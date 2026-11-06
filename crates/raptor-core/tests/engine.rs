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
