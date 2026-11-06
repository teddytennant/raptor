//! A tiny stand-in for a Rex server, for tests that need to misbehave on cue
//! or record exactly what the client sent.

#![allow(dead_code, clippy::result_large_err)] // the handshake callback signature is tungstenite's

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

pub const SESSION: &str = "session:s";
pub const BLOCK: &str = "block:b";

#[derive(Default)]
pub struct Options {
    /// Hang up (no close frame) on the first request after the welcome.
    pub drop_on_first_request: bool,
    /// Answer these methods only after a delay.
    pub delays: Vec<(&'static str, Duration)>,
    /// Refuse every data connection with this REJECT reason.
    pub data_reject: Option<u8>,
}

pub struct Fake {
    pub addr: String,
    /// Every request in the order it arrived: (method, payload).
    pub log: Arc<Mutex<Vec<(String, Value)>>>,
    pub control_connects: Arc<AtomicUsize>,
    pub data_connects: Arc<AtomicUsize>,
    /// Hang up on this many control connections before the handshake, like a network still down.
    pub refuse_control: Arc<AtomicUsize>,
    /// Stop answering anything, like a socket that died while the laptop slept.
    pub mute: Arc<std::sync::atomic::AtomicBool>,
    /// Senders for every live control connection, for pushing events.
    conns: Arc<Mutex<Vec<tokio::sync::mpsc::UnboundedSender<String>>>>,
    _rt: tokio::runtime::Runtime,
}

impl Fake {
    pub fn start(opts: Options) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = Arc::new(Mutex::new(Vec::new()));
        let control_connects = Arc::new(AtomicUsize::new(0));
        let data_connects = Arc::new(AtomicUsize::new(0));
        let opts = Arc::new(opts);
        let conns = Arc::new(Mutex::new(Vec::new()));
        let refuse_control = Arc::new(AtomicUsize::new(0));
        let mute = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (l, c, d, cs, rc, mu) =
            (log.clone(), control_connects.clone(), data_connects.clone(), conns.clone(), refuse_control.clone(), mute.clone());
        rt.spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                if rc.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_ok() {
                    continue;
                }
                let (l, c, d, cs, mu, opts) = (l.clone(), c.clone(), d.clone(), cs.clone(), mu.clone(), opts.clone());
                tokio::spawn(async move {
                    let path = Arc::new(Mutex::new(String::new()));
                    let p = path.clone();
                    let cb = move |req: &Request, mut resp: Response| {
                        *p.lock() = req.uri().path().to_owned();
                        let proto = req.headers().get("Sec-WebSocket-Protocol").cloned();
                        if let Some(v) = proto {
                            resp.headers_mut().insert("Sec-WebSocket-Protocol", v);
                        }
                        Ok(resp)
                    };
                    let Ok(ws) = tokio_tungstenite::accept_hdr_async(tcp, cb).await else { return };
                    let path = path.lock().clone();
                    if path == "/api/control" {
                        c.fetch_add(1, Ordering::SeqCst);
                        control(ws, l, cs, mu, opts).await;
                    } else {
                        d.fetch_add(1, Ordering::SeqCst);
                        data(ws, opts.data_reject, mu).await;
                    }
                });
            }
        });
        Self { addr, log, control_connects, data_connects, refuse_control, mute, conns, _rt: rt }
    }

    /// Send an event to every connection.
    pub fn emit(&self, event: Value) {
        let msg = json!({"type": "event", "seq": 1, "payload": event}).to_string();
        self.conns.lock().retain(|tx| tx.send(msg.clone()).is_ok());
    }

    /// Send a session event from the fake session.
    pub fn emit_session(&self, kind: &str, payload: Value) {
        self.emit(json!({"type": "session_event", "payload": {"session_id": SESSION, "event": {"type": kind, "payload": payload}}}));
    }

    /// Send an event from the fake terminal block.
    pub fn emit_block(&self, name: &str, payload: Value) {
        self.emit_session(
            "block_event",
            json!({"block_id": BLOCK, "creator_name": "com.superlogical.terminal", "name": name, "payload": payload}),
        );
    }

    pub fn methods(&self) -> Vec<String> {
        self.log.lock().iter().map(|(m, _)| m.clone()).collect()
    }
}

fn view() -> Value {
    json!({"revision": 1, "session_id": SESSION, "label": "one", "active_window_id": "window:w",
        "windows": [{"window_id": "window:w", "label": "1", "focused_block_id": BLOCK, "layers": [
            {"layer_id": "layer:t", "kind": "tiled", "bounds": [0,0,1,1], "layout": {"rect": [0,0,1,1], "block_id": BLOCK},
             "blocks": [{"block_id": BLOCK, "label": "shell", "creator_name": "com.superlogical.terminal"}]}]}]})
}

async fn control(
    ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    log: Arc<Mutex<Vec<(String, Value)>>>,
    conns: Arc<Mutex<Vec<tokio::sync::mpsc::UnboundedSender<String>>>>,
    mute: Arc<std::sync::atomic::AtomicBool>,
    opts: Arc<Options>,
) {
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    conns.lock().push(tx.clone());
    // Like Rex, session methods fail until the connection attaches to the session.
    let attached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if sink.send(Message::text(m)).await.is_err() {
                return;
            }
        }
    });
    let mut welcomed = false;
    while let Some(Ok(msg)) = stream.next().await {
        if let Message::Close(_) = msg {
            log.lock().push(("close frame".into(), Value::Null));
        }
        let Message::Text(t) = msg else { continue };
        let v: Value = serde_json::from_str(&t).unwrap();
        if !welcomed {
            welcomed = true;
            log.lock().push(("hello".into(), v.clone()));
            let w = json!({"type": "welcome", "protocol": "rex.control.v1", "limits": {"max_message_bytes": 1048576},
                "client": {"id": "client:me", "reconnect_token": "tok"}, "api_revision": 9, "api_server_revision": 9});
            let _ = tx.send(w.to_string());
            continue;
        }
        if mute.load(Ordering::SeqCst) {
            continue;
        }
        if opts.drop_on_first_request {
            writer.abort();
            return;
        }
        let id = v["id"].as_str().unwrap_or_default().to_owned();
        let method = v["method"].as_str().unwrap_or_default().to_owned();
        log.lock().push((method.clone(), v.get("payload").cloned().unwrap_or(Value::Null)));
        let p = v.get("payload").cloned().unwrap_or(Value::Null);
        let attaches_here = p["attach_session_ids"].as_array().is_some_and(|a| a.iter().any(|s| s == SESSION));
        let payload = match method.as_str() {
            "session.bootstrap" => json!({"client_id": "client:me", "server_status": {},
                "attach_all": {"sessions": [{"session_id": SESSION, "label": "one"}]}, "selected_session": null,
                "attached_sessions": if attaches_here { json!([{"session": {"session_id": SESSION, "label": "one"}, "view": view()}]) } else { json!([]) }}),
            "session.attach" => json!({"client_id": "client:me", "view": view()}),
            "session.view" => view(),
            "data.connect" => json!({"ticket": "AAAAAAAAAAAAAAAAAAAAAA"}),
            "client.inspect" => json!({"client": {"client_id": p["client_id"], "info": {"name": "Inspected"}, "principal": {"display": "someone"}}}),
            "com.superlogical.terminal.clipboard_read" => json!({"id": 5, "data": "copied in full", "total": 14, "eof": true}),
            _ => json!({}),
        };
        let needs_attach = method == "data.connect" || method.starts_with("com.superlogical.terminal.") || method == "client.key";
        let delay = opts.delays.iter().find(|(m, _)| *m == method).map(|(_, d)| *d);
        let (tx, attached) = (tx.clone(), attached.clone());
        if attaches_here {
            attached.store(true, Ordering::SeqCst);
        }
        let reply = async move {
            if let Some(d) = delay {
                tokio::time::sleep(d).await;
            }
            let msg = if needs_attach && !attached.load(Ordering::SeqCst) {
                json!({"type": "response", "id": id, "seq": 0, "error": {"code": "conflict", "message": "not attached", "status": 409}})
            } else {
                if method == "session.attach" {
                    attached.store(true, Ordering::SeqCst);
                }
                json!({"type": "response", "id": id, "seq": 0, "payload": payload})
            };
            let _ = tx.send(msg.to_string());
        };
        if delay.is_some() {
            tokio::spawn(reply);
        } else {
            reply.await;
        }
    }
}

async fn data(mut ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, reject: Option<u8>, mute: Arc<std::sync::atomic::AtomicBool>) {
    let Some(Ok(Message::Binary(_hello))) = ws.next().await else { return };
    if let Some(r) = reject {
        let _ = ws.send(Message::binary(vec![0x82, r])).await;
        let _ = ws.close(None).await;
        return;
    }
    let mut w = vec![0x81];
    w.extend([1u8; 16]);
    w.extend([0, 0, 0]);
    w.extend(60_000u32.to_le_bytes());
    w.extend(7u64.to_le_bytes());
    let _ = ws.send(Message::binary(w)).await;
    let mut t = raptor_term::Terminal::new(80, 24);
    t.write_vt(b"fake$ ");
    let _ = ws.send(Message::binary(t.snapshot().unwrap())).await;
    loop {
        // While muted, don't even read: pings go unanswered.
        if mute.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
            continue;
        }
        match tokio::time::timeout(Duration::from_millis(20), ws.next()).await {
            Ok(Some(Ok(_))) | Err(_) => {}
            Ok(_) => return,
        }
    }
}
