//! One supervised connection to one Rex server, and the live model it keeps.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{Value, json};
use raptor_client::{ConnEvent, ConnectOptions, Connection, Endpoint};
use raptor_proto::{AgentState, Event, Process, ProgramStatus, SessionEvent, SessionSummary, SessionView, TerminalEvent};
use tokio::sync::{Notify, watch};

use crate::state::*;
use crate::store::Profile;

/// A connection that lasted this long was healthy; losing it isn't the server's fault.
const STABLE_AFTER: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(crate) struct BlockMeta {
    pub title: String,
    pub pwd: String,
    pub process: String,
    pub statuses: BTreeMap<String, ProgramStatus>,
    pub exited: Option<i32>,
    pub progress: Option<u8>,
    pub bell: bool,
    pub fetched: bool,
}

impl BlockMeta {
    fn agent(&self) -> Option<AgentStatus> {
        let (_, top) = self.statuses.iter().max_by_key(|(_, r)| (AgentState::parse(&r.state), r.updated_at.clone()))?;
        let state = AgentState::parse(&top.state);
        let app = top.effective_app.clone().or_else(|| top.app.clone()).unwrap_or_default();
        Some(AgentStatus {
            state,
            waiting_for: top.kind.clone(),
            app,
            title: top.title.clone().unwrap_or_default(),
            message: top.msg.clone().unwrap_or_default(),
            progress: top.progress,
            updated_at: top.updated_at.clone().unwrap_or_default(),
        })
    }
}

#[derive(Default)]
pub(crate) struct SessionModel {
    pub summary: SessionSummary,
    pub view: Option<SessionView>,
    pub clients: HashMap<String, String>,
}

#[derive(Default)]
pub(crate) struct ServerModel {
    pub connectivity: Connectivity,
    pub latency_ms: Option<u32>,
    pub version: Option<String>,
    pub host_name: Option<String>,
    pub order: Vec<String>,
    pub sessions: HashMap<String, SessionModel>,
    pub meta: HashMap<String, BlockMeta>,
    pub client_id: Option<String>,
    pub account: Option<String>,
}

pub(crate) struct ServerEntry {
    pub profile: Mutex<Profile>,
    pub conn: Mutex<Option<Connection>>,
    /// Fires whenever a new connection is ready (terminals wait on it).
    pub conn_tx: watch::Sender<u64>,
    pub model: Mutex<ServerModel>,
    pub wake: Notify,
    pub stop: watch::Sender<bool>,
    /// Open terminals, for routing block events (clipboard) to them.
    pub terminals: Mutex<HashMap<String, std::sync::Weak<crate::terminal::TerminalSession>>>,
}

pub(crate) trait Sink: Send + Sync {
    fn changed(&self);
    fn notify(&self, n: Notification);
    fn client_options(&self) -> ConnectOptions;
    fn foreground(&self) -> watch::Receiver<bool>;
}

impl ServerEntry {
    pub fn new(profile: Profile) -> Arc<Self> {
        Arc::new(Self {
            profile: Mutex::new(profile),
            conn: Mutex::new(None),
            conn_tx: watch::channel(0).0,
            model: Mutex::new(ServerModel::default()),
            wake: Notify::new(),
            stop: watch::channel(false).0,
            terminals: Mutex::new(HashMap::new()),
        })
    }

    pub fn id(&self) -> String {
        self.profile.lock().id.clone()
    }

    /// Drop the live connection as a lost network would; the supervisor resumes it.
    pub(crate) fn drop_connection(&self) {
        if let Some(c) = self.conn.lock().take() {
            c.abort();
        }
    }

    pub fn connection(&self) -> Option<Connection> {
        self.conn.lock().clone().filter(|c| !c.is_closed())
    }

    /// Wait (up to `timeout`) for a live connection.
    pub async fn wait_connection(&self, timeout: Duration) -> Option<Connection> {
        if let Some(c) = self.connection() {
            return Some(c);
        }
        self.wake.notify_one();
        let mut rx = self.conn_tx.subscribe();
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                r = rx.changed() => {
                    if r.is_err() {
                        return None;
                    }
                    if let Some(c) = self.connection() {
                        return Some(c);
                    }
                }
                _ = &mut deadline => return None,
            }
        }
    }

    pub fn snapshot(&self) -> ServerState {
        let p = self.profile.lock().clone();
        let m = self.model.lock();
        let me = m.client_id.clone();
        let sessions = m
            .order
            .iter()
            .filter_map(|id| m.sessions.get(id))
            .map(|s| {
                let windows = s
                    .view
                    .as_ref()
                    .map(|v| {
                        v.windows
                            .iter()
                            .map(|w| {
                                let placed = w.placed_blocks();
                                let blocks = w
                                    .blocks()
                                    .filter(|b| b.creator_name.is_empty() || b.creator_name == raptor_proto::TERMINAL_CREATOR)
                                    .map(|b| {
                                        let id = b.block_id.clone().unwrap_or_default();
                                        let meta = m.meta.get(&id);
                                        let pl = placed.iter().find(|p| p.block_id == id);
                                        BlockState {
                                            label: b.label.clone(),
                                            title: meta.map(|m| m.title.clone()).unwrap_or_default(),
                                            pwd: meta.map(|m| m.pwd.clone()).unwrap_or_default(),
                                            process: meta.map(|m| m.process.clone()).unwrap_or_default(),
                                            rect: pl.map(|p| p.rect).unwrap_or([0.0, 0.0, 1.0, 1.0]),
                                            floating: pl.is_some_and(|p| p.floating),
                                            layer_id: pl.map(|p| p.layer_id.clone()).unwrap_or_default(),
                                            minimized: pl.is_some_and(|p| p.minimized),
                                            owned_by_me: me.is_some() && b.owner == me,
                                            // An owner that's no longer connected (e.g. the CLI that created
                                            // the session) shouldn't stop us taking the size. Only judge once
                                            // the client list has loaded, which always includes us.
                                            owner: b.owner.clone().filter(|o| {
                                                let loaded = me.as_ref().is_some_and(|m| s.clients.contains_key(m));
                                                !loaded || Some(o) == me.as_ref() || s.clients.contains_key(o)
                                            }),
                                            exited: meta.and_then(|m| m.exited),
                                            agent: meta.and_then(BlockMeta::agent),
                                            progress: meta.and_then(|m| m.progress),
                                            has_bell: meta.is_some_and(|m| m.bell),
                                            id,
                                        }
                                    })
                                    .collect();
                                WindowState {
                                    id: w.id().to_owned(),
                                    label: w.label.clone(),
                                    focused_block_id: w.focused_block_id.clone(),
                                    zoomed_block_id: w.zoomed_block_id.clone(),
                                    blocks,
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut clients: Vec<String> =
                    s.clients.iter().filter(|(id, _)| Some(*id) != me.as_ref()).map(|(_, d)| d.clone()).collect();
                clients.sort();
                SessionState {
                    id: s.summary.session_id.clone(),
                    label: s.summary.label.clone(),
                    active_window_id: s.view.as_ref().and_then(|v| v.active_window_id.clone()),
                    windows,
                    clients,
                }
            })
            .collect();
        // A bare IP isn't a great name; "account@ip" (or the tailnet name) reads better.
        let host = Endpoint::parse(&p.address)
            .ok()
            .map(|e| match e {
                Endpoint::Unix(_) => "This computer".to_owned(),
                Endpoint::Url(u) => u.host_str().unwrap_or("server").to_owned(),
            })
            .unwrap_or_else(|| p.address.clone());
        let raw = if p.name.trim().is_empty() { host } else { p.name.clone() };
        let name = if raw.parse::<std::net::IpAddr>().is_ok() {
            match (&m.host_name, &m.account) {
                (Some(h), _) => h.clone(),
                (None, Some(a)) => format!("{a}@{raw}"),
                _ => raw,
            }
        } else {
            raw
        };
        ServerState {
            id: p.id.clone(),
            name,
            address: p.address.clone(),
            connectivity: m.connectivity.clone(),
            latency_ms: m.latency_ms,
            version: m.version.clone(),
            host_name: m.host_name.clone(),
            sessions,
            is_local: Endpoint::parse(&p.address).map(|e| e.is_local()).unwrap_or(false),
        }
    }

    fn set_connectivity(&self, c: Connectivity, sink: &dyn Sink) {
        let mut m = self.model.lock();
        if m.connectivity != c {
            m.connectivity = c;
            drop(m);
            sink.changed();
        }
    }

    /// The supervisor: connect, mirror state, reconnect forever until stopped.
    pub async fn run(self: Arc<Self>, sink: Arc<dyn Sink>) {
        let mut stop = self.stop.subscribe();
        let mut fg = sink.foreground();
        let mut token: Option<String> = None;
        let mut backoff = Duration::from_millis(500);
        loop {
            if *stop.borrow() {
                return;
            }
            // Don't hold sockets open while a phone app is in the background. Drop
            // them without a close frame: the server then keeps this client (and
            // its terminal streams) for a minute, so coming back is a quick resume.
            while !*fg.borrow() {
                self.drop_connection();
                self.set_connectivity(Connectivity::Idle, &*sink);
                tokio::select! {
                    _ = fg.changed() => {}
                    _ = stop.changed() => return,
                }
            }
            let address = self.profile.lock().address.clone();
            let endpoint = match Endpoint::parse(&address) {
                Ok(e) => e,
                Err(e) => {
                    self.set_connectivity(Connectivity::Offline { error: e.to_string(), retry_in_ms: 0 }, &*sink);
                    tokio::select! { _ = self.wake.notified() => continue, _ = stop.changed() => return }
                }
            };
            self.set_connectivity(Connectivity::Connecting, &*sink);
            let mut opts = sink.client_options();
            opts.reconnect_token = token.take();
            let started = Instant::now();
            let error = match Connection::connect(&endpoint, opts).await {
                Ok((conn, events)) => {
                    token = conn.reconnect_token().map(str::to_owned);
                    let rtt = started.elapsed().as_millis().min(60_000) as u32;
                    match self.clone().serve(conn, events, &sink, rtt, &mut stop, &mut fg, &mut token).await {
                        Served::Stopped => return,
                        Served::Background => continue,
                        Served::Lost(reason) => {
                            tracing::info!("connection to {address} lost: {reason}");
                            // After a connection that held up, reconnect right away (the
                            // server keeps us for 60s). One that failed soon after it opened
                            // backs off like a failed connect, or a server that accepts and
                            // then drops us would be hammered in a tight loop.
                            if started.elapsed() > STABLE_AFTER {
                                backoff = Duration::from_millis(500);
                                self.set_connectivity(Connectivity::Offline { error: reason, retry_in_ms: 0 }, &*sink);
                                continue;
                            }
                            reason
                        }
                    }
                }
                Err(e) => friendly(&e),
            };
            let retry = backoff;
            backoff = (backoff * 2).min(Duration::from_secs(30));
            self.set_connectivity(Connectivity::Offline { error, retry_in_ms: retry.as_millis() as u64 }, &*sink);
            tokio::select! {
                _ = tokio::time::sleep(retry) => {}
                _ = self.wake.notified() => { backoff = Duration::from_millis(500); }
                _ = fg.changed() => {}
                _ = stop.changed() => return,
            }
        }
    }

    async fn serve(
        self: Arc<Self>,
        conn: Connection,
        mut events: tokio::sync::mpsc::UnboundedReceiver<ConnEvent>,
        sink: &Arc<dyn Sink>,
        rtt: u32,
        stop: &mut watch::Receiver<bool>,
        fg: &mut watch::Receiver<bool>,
        token: &mut Option<String>,
    ) -> Served {
        let boot = match conn.bootstrap(Some(&[]), None).await {
            Ok(b) => b,
            Err(e) => return Served::Lost(friendly(&e)),
        };
        // Time a tiny call for a true round-trip figure (connect time includes TLS etc.).
        let t = Instant::now();
        let version = conn.call("server.version", json!({})).await.ok();
        let rtt = (t.elapsed().as_millis() as u32).min(rtt.max(1));
        let account =
            conn.call("server.whoami", json!({})).await.ok().and_then(|v| v.get("account").and_then(Value::as_str).map(str::to_owned));
        {
            let mut m = self.model.lock();
            m.client_id = Some(boot.client_id.clone()).filter(|s| !s.is_empty()).or_else(|| conn.client_id().map(str::to_owned));
            m.latency_ms = Some(rtt);
            m.version = version.as_ref().and_then(|v| v.get("rex_version")).and_then(Value::as_str).map(short_version);
            m.host_name = boot
                .server_status
                .pointer("/tailscale/dns_name")
                .and_then(Value::as_str)
                .map(|s| s.split('.').next().unwrap_or(s).to_owned());
            m.account = account;
            m.order = boot.sessions().iter().map(|s| s.session_id.clone()).collect();
            let keep: std::collections::HashSet<_> = m.order.iter().cloned().collect();
            m.sessions.retain(|k, _| keep.contains(k));
            for s in boot.sessions() {
                m.sessions.entry(s.session_id.clone()).or_default().summary = s.clone();
            }
        }
        *self.conn.lock() = Some(conn.clone());
        self.conn_tx.send_modify(|v| *v += 1);
        for s in boot.sessions() {
            self.clone().attach(conn.clone(), s.session_id.clone(), sink.clone());
        }
        self.set_connectivity(Connectivity::Online, &**sink);

        loop {
            tokio::select! {
                ev = events.recv() => match ev {
                    Some(ConnEvent::Event { event, .. }) => self.clone().apply(&conn, event, sink),
                    Some(ConnEvent::Closed { reason }) => {
                        *self.conn.lock() = None;
                        *token = conn.reconnect_token().map(str::to_owned);
                        return Served::Lost(reason);
                    }
                    None => {
                        *self.conn.lock() = None;
                        *token = conn.reconnect_token().map(str::to_owned);
                        return Served::Lost("connection lost".into());
                    }
                },
                _ = stop.changed() => {
                    conn.close();
                    return Served::Stopped;
                }
                _ = fg.changed() => if !*fg.borrow() {
                    *token = conn.reconnect_token().map(str::to_owned);
                    *self.conn.lock() = None;
                    conn.abort();
                    return Served::Background;
                },
                _ = self.wake.notified() => {
                    // A manual "reconnect" while online: re-read everything.
                    for id in self.model.lock().order.clone() {
                        self.clone().refresh_view(conn.clone(), id, sink.clone());
                    }
                }
            }
        }
    }

    fn attach(self: Arc<Self>, conn: Connection, session_id: String, sink: Arc<dyn Sink>) {
        tokio::spawn(async move {
            match conn.attach(&session_id).await {
                Ok((_, view)) => {
                    let blocks: Vec<String> = view.all_blocks().filter_map(|(_, b)| b.block_id.clone()).collect();
                    {
                        let mut m = self.model.lock();
                        let s = m.sessions.entry(session_id.clone()).or_default();
                        s.summary.session_id = session_id.clone();
                        if s.summary.label.is_empty() {
                            s.summary.label = view.label.clone();
                        }
                        s.view = Some(view);
                    }
                    sink.changed();
                    self.fetch_meta(&conn, &session_id, blocks, &sink).await;
                    if let Ok(v) = conn.call("session.list_clients", json!({"session_id": session_id})).await {
                        let mut m = self.model.lock();
                        if let Some(s) = m.sessions.get_mut(&session_id) {
                            s.clients = v
                                .get("clients")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                                .filter_map(|c| {
                                    let id = c.get("client_id")?.as_str()?.to_owned();
                                    Some((id, describe_client(c)))
                                })
                                .collect();
                        }
                        drop(m);
                        sink.changed();
                    }
                }
                Err(e) => tracing::warn!("attach {session_id}: {e}"),
            }
        });
    }

    fn refresh_view(self: Arc<Self>, conn: Connection, session_id: String, sink: Arc<dyn Sink>) {
        tokio::spawn(async move {
            if let Ok((_, view)) = conn.view(&session_id).await {
                let new_blocks: Vec<String> = {
                    let mut m = self.model.lock();
                    let known: Vec<String> = view
                        .all_blocks()
                        .filter_map(|(_, b)| b.block_id.clone())
                        .filter(|id| !m.meta.get(id).is_some_and(|x| x.fetched))
                        .collect();
                    if let Some(s) = m.sessions.get_mut(&session_id)
                        && s.view.as_ref().is_none_or(|old| old.revision <= view.revision) {
                            s.summary.label = view.label.clone();
                            s.view = Some(view);
                        }
                    known
                };
                sink.changed();
                if !new_blocks.is_empty() {
                    self.fetch_meta(&conn, &session_id, new_blocks, &sink).await;
                }
            }
        });
    }

    /// Title, cwd, process and agent status for blocks we haven't seen yet.
    async fn fetch_meta(&self, conn: &Connection, session_id: &str, blocks: Vec<String>, sink: &Arc<dyn Sink>) {
        for chunk in blocks.chunks(8) {
            let futs = chunk.iter().map(|b| async move {
                let (t, p, st, pr) = tokio::join!(
                    conn.terminal(session_id, b, "title", Value::Null),
                    conn.terminal(session_id, b, "pwd", Value::Null),
                    conn.terminal(session_id, b, "program_status", Value::Null),
                    conn.terminal(session_id, b, "process", Value::Null),
                );
                (b.clone(), t.ok(), p.ok(), st.ok(), pr.ok())
            });
            let results = futures_util::future::join_all(futs).await;
            let mut m = self.model.lock();
            for (b, t, p, st, pr) in results {
                let meta = m.meta.entry(b).or_default();
                meta.fetched = true;
                if let Some(t) = t.as_ref().and_then(|v| v.get("title")).and_then(Value::as_str) {
                    meta.title = t.to_owned();
                }
                if let Some(p) = p.as_ref().and_then(|v| v.get("pwd")).and_then(Value::as_str) {
                    meta.pwd = p.to_owned();
                }
                if let Some(p) = pr.as_ref().and_then(|v| v.get("foreground")).and_then(|f| serde_json::from_value::<Process>(f.clone()).ok()) {
                    meta.process = p.label();
                }
                if let Some(code) = pr.as_ref().and_then(|v| v.pointer("/last_exit/exit_code")).and_then(Value::as_i64)
                    && pr.as_ref().and_then(|v| v.get("child")).is_none_or(Value::is_null) {
                        meta.exited = Some(code as i32);
                    }
                if let Some(recs) = st.as_ref().and_then(|v| v.get("records")).and_then(Value::as_array) {
                    meta.statuses.clear();
                    for r in recs {
                        if let Ok(rec) = serde_json::from_value::<ProgramStatus>(r.get("record").cloned().unwrap_or_else(|| r.clone())) {
                            let id = r.get("id").and_then(Value::as_str).map(str::to_owned).or_else(|| rec.id.clone()).unwrap_or_default();
                            meta.statuses.insert(id, rec);
                        }
                    }
                }
            }
            drop(m);
            sink.changed();
        }
    }

    fn apply(self: Arc<Self>, conn: &Connection, event: Event, sink: &Arc<dyn Sink>) {
        match event {
            Event::SessionCreated(s) => {
                {
                    let mut m = self.model.lock();
                    if !m.order.contains(&s.session_id) {
                        m.order.push(s.session_id.clone());
                    }
                    m.sessions.entry(s.session_id.clone()).or_default().summary = s.clone();
                }
                sink.changed();
                self.attach(conn.clone(), s.session_id, sink.clone());
            }
            Event::SessionDestroyed { session_id } => {
                let mut m = self.model.lock();
                m.order.retain(|s| *s != session_id);
                m.sessions.remove(&session_id);
                drop(m);
                sink.changed();
            }
            Event::SessionMoved { .. } => {
                let (this, conn, sink) = (self.clone(), conn.clone(), sink.clone());
                tokio::spawn(async move {
                    if let Ok(list) = conn.list_sessions().await {
                        this.model.lock().order = list.into_iter().map(|s| s.session_id).collect();
                        sink.changed();
                    }
                });
            }
            Event::Session { session_id, event } => self.apply_session(conn, session_id, event, sink),
            Event::ClientChanged(client) => {
                let Some(id) = client.get("client_id").and_then(Value::as_str) else { return };
                let d = describe_client(&client);
                for s in self.model.lock().sessions.values_mut() {
                    if let Some(c) = s.clients.get_mut(id) {
                        c.clone_from(&d);
                    }
                }
                sink.changed();
            }
            _ => {}
        }
    }

    fn apply_session(self: Arc<Self>, conn: &Connection, session_id: String, event: SessionEvent, sink: &Arc<dyn Sink>) {
        match event {
            SessionEvent::ViewChanged { revision } => {
                let stale = self.model.lock().sessions.get(&session_id).and_then(|s| s.view.as_ref()).is_none_or(|v| v.revision < revision);
                if stale {
                    self.refresh_view(conn.clone(), session_id, sink.clone());
                }
            }
            SessionEvent::LabelChanged { label } => {
                if let Some(s) = self.model.lock().sessions.get_mut(&session_id) {
                    s.summary.label = label;
                }
                sink.changed();
            }
            SessionEvent::BlockClosed { block_id } => {
                self.model.lock().meta.remove(&block_id);
                sink.changed();
            }
            SessionEvent::ClientConnected { client_id, display } => {
                // Listed right away; the inspect reply only fills in its name, and
                // only if it hasn't left by then (the CLI comes and goes in a blink).
                if let Some(s) = self.model.lock().sessions.get_mut(&session_id) {
                    s.clients.insert(client_id.clone(), describe_client(&json!({"principal": {"display": display}})));
                }
                sink.changed();
                let (this, conn, sink, sid) = (self.clone(), conn.clone(), sink.clone(), session_id.clone());
                tokio::spawn(async move {
                    if let Ok(v) = conn.call("client.inspect", json!({"client_id": client_id})).await {
                        let d = describe_client(v.get("client").unwrap_or(&v));
                        if let Some(c) = this.model.lock().sessions.get_mut(&sid).and_then(|s| s.clients.get_mut(&client_id)) {
                            *c = d;
                        }
                        sink.changed();
                    }
                });
            }
            SessionEvent::ClientDisconnected { client_id } => {
                if let Some(s) = self.model.lock().sessions.get_mut(&session_id) {
                    s.clients.remove(&client_id);
                }
                sink.changed();
            }
            SessionEvent::Block { block_id, event, .. } => self.apply_block(conn, session_id, block_id, event, sink),
            _ => {}
        }
    }

    fn apply_block(&self, conn: &Connection, session_id: String, block_id: String, event: TerminalEvent, sink: &Arc<dyn Sink>) {
        let server_id = self.id();
        if let TerminalEvent::Clipboard { text, id } = event {
            let Some(term) = self.terminals.lock().get(&block_id).cloned() else { return };
            match (text, id) {
                (Some(t), _) => {
                    if let Some(term) = term.upgrade() {
                        term.on_clipboard(t);
                    }
                }
                // Too large for the event: read it in parts.
                (None, Some(id)) => {
                    let conn = conn.clone();
                    tokio::spawn(async move {
                        if let Some(text) = read_clipboard(&conn, &session_id, &block_id, id).await
                            && let Some(term) = term.upgrade()
                        {
                            term.on_clipboard(text);
                        }
                    });
                }
                (None, None) => {}
            }
            return;
        }
        let mut note: Option<Notification> = None;
        {
            let mut m = self.model.lock();
            let meta = m.meta.entry(block_id.clone()).or_default();
            let label_for = |meta: &BlockMeta| notification_name(&meta.title, &meta.process);
            match event {
                TerminalEvent::TitleChanged { title } => meta.title = title,
                TerminalEvent::PwdChanged { pwd } => meta.pwd = pwd,
                TerminalEvent::ProcessChanged { foreground } => {
                    if let Some(p) = foreground {
                        meta.process = p.label();
                        meta.exited = None;
                    }
                }
                TerminalEvent::Progress { state, progress } => {
                    meta.progress = if state == "remove" || state == "none" { None } else { progress.or(Some(0)) };
                }
                TerminalEvent::Bell => {
                    meta.bell = true;
                    note = Some(Notification {
                        kind: NotificationKind::Bell,
                        server_id,
                        session_id,
                        block_id,
                        title: label_for(meta),
                        body: "Bell".into(),
                    });
                }
                TerminalEvent::ChildExited { exit_code, .. } => {
                    meta.exited = Some(exit_code);
                    note = Some(Notification {
                        kind: NotificationKind::ProcessExited,
                        server_id,
                        session_id,
                        block_id,
                        title: label_for(meta),
                        body: if exit_code == 0 { "Exited".into() } else { format!("Exited with status {exit_code}") },
                    });
                }
                TerminalEvent::Notification { title, body } => {
                    note = Some(Notification {
                        kind: NotificationKind::Program,
                        server_id,
                        session_id,
                        block_id,
                        title: if title.is_empty() { label_for(meta) } else { title },
                        body,
                    });
                }
                TerminalEvent::StatusChanged { id, record } => {
                    let before = meta.statuses.get(&id).map(|r| AgentState::parse(&r.state));
                    let after = AgentState::parse(&record.state);
                    let app = record.effective_app.clone().or_else(|| record.app.clone()).unwrap_or_else(|| label_for(meta));
                    let msg = record.msg.clone().or_else(|| record.title.clone()).unwrap_or_default();
                    if before != Some(after) {
                        let kind = match after {
                            AgentState::Blocked => Some(NotificationKind::AgentBlocked),
                            AgentState::Done if before == Some(AgentState::Working) || before == Some(AgentState::Blocked) => {
                                Some(NotificationKind::AgentDone)
                            }
                            AgentState::Error => Some(NotificationKind::AgentError),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            let body = match (&kind, record.kind.as_deref()) {
                                (NotificationKind::AgentBlocked, Some("permission")) if msg.is_empty() => {
                                    "Wants permission to continue".into()
                                }
                                (NotificationKind::AgentBlocked, Some("question")) if msg.is_empty() => "Has a question for you".into(),
                                (NotificationKind::AgentBlocked, Some("auth")) if msg.is_empty() => "Needs credentials".into(),
                                (NotificationKind::AgentDone, _) if msg.is_empty() => "Finished".into(),
                                _ => msg.clone(),
                            };
                            note = Some(Notification { kind, server_id, session_id, block_id, title: app, body });
                        }
                    }
                    meta.statuses.insert(id, record);
                }
                TerminalEvent::StatusRemoved { ids } => {
                    for id in ids {
                        meta.statuses.remove(&id);
                    }
                }
                _ => return,
            }
        }
        if let Some(n) = note {
            sink.notify(n);
        }
        sink.changed();
    }

    pub fn clear_bell(&self, block_id: &str) {
        if let Some(m) = self.model.lock().meta.get_mut(block_id) {
            m.bell = false;
        }
    }
}

/// Read a copied text that was too large for its event.
async fn read_clipboard(conn: &Connection, session_id: &str, block_id: &str, id: u64) -> Option<String> {
    let mut text = String::new();
    loop {
        let args = json!({"id": id, "offset": text.len(), "max_bytes": 983_040});
        let v = conn.terminal(session_id, block_id, "clipboard_read", args).await.ok()?;
        let data = v.get("data").and_then(Value::as_str).unwrap_or_default();
        text.push_str(data);
        if data.is_empty() || v.get("eof").and_then(Value::as_bool).unwrap_or(true) || text.len() > 64 << 20 {
            return Some(text);
        }
    }
}

pub(crate) enum Served {
    Stopped,
    Background,
    Lost(String),
}

fn describe_client(c: &Value) -> String {
    let info = c.get("info");
    let name = info.and_then(|i| i.get("name")).and_then(Value::as_str).unwrap_or("");
    let label = info.and_then(|i| i.get("label")).and_then(Value::as_str).unwrap_or("");
    let who = c.pointer("/principal/node").or_else(|| c.pointer("/principal/display")).and_then(Value::as_str).unwrap_or("");
    let base = if !label.is_empty() {
        label
    } else if !name.is_empty() {
        name
    } else {
        "client"
    };
    if who.is_empty() { base.to_owned() } else { format!("{base} · {who}") }
}

pub(crate) fn friendly(e: &raptor_client::ClientError) -> String {
    use raptor_client::ClientError as E;
    match e {
        E::Io(io) => match io.kind() {
            std::io::ErrorKind::ConnectionRefused => "Server isn't running there".into(),
            std::io::ErrorKind::NotFound => "No server at that address".into(),
            std::io::ErrorKind::TimedOut => "Timed out — is the server reachable?".into(),
            _ => format!("Can't reach the server ({io})"),
        },
        E::Timeout => "Timed out — check the address and your network (Tailscale?)".into(),
        E::WebSocket(w) => {
            let s = w.to_string();
            if s.contains("403") {
                "The server refused this device (Tailscale access rules?)".into()
            } else {
                format!("Connection failed: {s}")
            }
        }
        E::Protocol(p) if p.contains("no common API revision") => "Server version isn't supported".into(),
        other => other.to_string(),
    }
}

/// `0.0.1793420730-g6d0d9a` → `g6d0d9a` style builds read better as their commit.
pub(crate) fn short_version(v: &str) -> String {
    match v.rsplit_once("-g") {
        Some((base, commit)) if base.starts_with("0.0.") => format!("build {commit}"),
        _ => v.to_owned(),
    }
}

/// A short name for a terminal in a notification title. Shells like fish title
/// the terminal with the whole command line plus the directory
/// (`printf '\e]7501;…' ~`); a person wants just the command (`printf`).
fn notification_name(title: &str, process: &str) -> String {
    let t = title.trim();
    if t.is_empty() || t.chars().any(char::is_control) {
        return if process.is_empty() { "Terminal".into() } else { process.to_owned() };
    }
    let mut words: Vec<&str> = t.split_whitespace().collect();
    // Drop a trailing directory, which fish and zsh themes append.
    if words.len() > 1 && words.last().is_some_and(|w| w.starts_with('~') || w.starts_with('/')) {
        words.pop();
    }
    let rest = words.join(" ");
    // A command line (quotes, escapes, pipes, flags) names its program first.
    let commandish = rest.contains(['\'', '"', '\\', '|', ';', '$']) || rest.split_whitespace().skip(1).any(|w| w.starts_with('-'));
    let name = if commandish { words.first().copied().unwrap_or(t).to_owned() } else { rest };
    let name: String = name.chars().take(40).collect();
    if name.is_empty() { process.to_owned() } else { name }
}

#[cfg(test)]
mod name_tests {
    use super::notification_name as n;

    #[test]
    fn names_for_notifications() {
        assert_eq!(n("printf '\\e]7501;stat ~", "fish"), "printf");
        assert_eq!(n("exit ~", "fish"), "exit");
        assert_eq!(n("✳ Claude Code", "node"), "✳ Claude Code");
        assert_eq!(n("vim notes.md ~/src", "vim"), "vim notes.md");
        assert_eq!(n("cargo test -p core /home/me/x", "cargo"), "cargo");
        assert_eq!(n("", "htop"), "htop");
        assert_eq!(n("bad\u{7}bell", "zsh"), "zsh");
    }
}
