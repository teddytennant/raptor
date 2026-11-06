//! A tiny stand-in for a Rex server, for tests that need to misbehave on cue
//! or record exactly what the client sent.

#![allow(dead_code)]

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
}

pub struct Fake {
    pub addr: String,
    /// Every request in the order it arrived: (method, payload).
    pub log: Arc<Mutex<Vec<(String, Value)>>>,
    pub control_connects: Arc<AtomicUsize>,
    _rt: tokio::runtime::Runtime,
}

impl Fake {
    pub fn start(opts: Options) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let log = Arc::new(Mutex::new(Vec::new()));
        let control_connects = Arc::new(AtomicUsize::new(0));
        let opts = Arc::new(opts);
        let (l, c) = (log.clone(), control_connects.clone());
        rt.spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let (l, c, opts) = (l.clone(), c.clone(), opts.clone());
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
                        control(ws, l, opts).await;
                    } else {
                        data(ws).await;
                    }
                });
            }
        });
        Self { addr, log, control_connects, _rt: rt }
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

async fn control(ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>, log: Arc<Mutex<Vec<(String, Value)>>>, opts: Arc<Options>) {
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if sink.send(Message::text(m)).await.is_err() {
                return;
            }
        }
    });
    let mut welcomed = false;
    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(t) = msg else { continue };
        let v: Value = serde_json::from_str(&t).unwrap();
        if !welcomed {
            welcomed = true;
            let w = json!({"type": "welcome", "protocol": "rex.control.v1", "limits": {"max_message_bytes": 1048576},
                "client": {"id": "client:me", "reconnect_token": "tok"}, "api_revision": 9, "api_server_revision": 9});
            let _ = tx.send(w.to_string());
            continue;
        }
        if opts.drop_on_first_request {
            writer.abort();
            return;
        }
        let id = v["id"].as_str().unwrap_or_default().to_owned();
        let method = v["method"].as_str().unwrap_or_default().to_owned();
        log.lock().push((method.clone(), v.get("payload").cloned().unwrap_or(Value::Null)));
        let payload = match method.as_str() {
            "session.bootstrap" => json!({"client_id": "client:me", "server_status": {},
                "attach_all": {"sessions": [{"session_id": SESSION, "label": "one"}]}, "selected_session": null, "attached_sessions": []}),
            "session.attach" => json!({"client_id": "client:me", "view": view()}),
            "session.view" => view(),
            "data.connect" => json!({"ticket": "AAAAAAAAAAAAAAAAAAAAAA"}),
            _ => json!({}),
        };
        let delay = opts.delays.iter().find(|(m, _)| *m == method).map(|(_, d)| *d);
        let tx = tx.clone();
        let reply = async move {
            if let Some(d) = delay {
                tokio::time::sleep(d).await;
            }
            let _ = tx.send(json!({"type": "response", "id": id, "seq": 0, "payload": payload}).to_string());
        };
        if delay.is_some() {
            tokio::spawn(reply);
        } else {
            reply.await;
        }
    }
}

async fn data(mut ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) {
    let Some(Ok(Message::Binary(_hello))) = ws.next().await else { return };
    let mut w = vec![0x81];
    w.extend([1u8; 16]);
    w.extend([0, 0, 0]);
    w.extend(60_000u32.to_le_bytes());
    w.extend(7u64.to_le_bytes());
    let _ = ws.send(Message::binary(w)).await;
    let mut t = raptor_term::Terminal::new(80, 24);
    t.write_vt(b"fake$ ");
    let _ = ws.send(Message::binary(t.snapshot().unwrap())).await;
    while let Some(Ok(_)) = ws.next().await {}
}
