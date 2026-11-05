//! `raptor-cli`: a small debugging tool for the Raptor core.

use std::io::Write;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use raptor_client::{ConnectOptions, Connection, DataEvent, DataStream, Endpoint};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (server, rest) = match args.iter().position(|a| a == "-S") {
        Some(i) => (Some(args[i + 1].clone()), [&args[..i], &args[i + 2..]].concat()),
        None => (None, args.clone()),
    };
    let endpoint = match server {
        Some(s) => Endpoint::parse(&s)?,
        None => Endpoint::local_default().context("no local server found; pass -S <address>")?,
    };
    let cmd = rest.first().map(String::as_str).unwrap_or("ls");
    let (conn, _events) = Connection::connect(&endpoint, ConnectOptions::default()).await?;
    eprintln!("connected to {} (api {}, client {:?})", endpoint.display(), conn.welcome().api_revision, conn.client_id());
    match cmd {
        "ls" => {
            let b = conn.bootstrap(None, None).await?;
            for s in b.sessions() {
                println!("{}  {}", s.session_id, s.label);
            }
            for a in b.attached() {
                for (w, blk) in a.view.all_blocks() {
                    println!("  {} {} / {} {}", a.session.label, w.label, blk.block_id.as_deref().unwrap_or(""), blk.label);
                }
            }
        }
        "dump" => {
            // dump [bytes]: print the raw start of the first block's stream.
            let limit: usize = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(4096);
            let b = conn.bootstrap(None, None).await?;
            let t = b.data_tickets.unwrap_or_default().into_iter().next().context("no terminal blocks")?;
            let (_ds, mut rx) = DataStream::open(endpoint.clone(), t.ticket);
            let mut got = Vec::new();
            let deadline = tokio::time::sleep(Duration::from_secs(3));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    ev = rx.recv() => match ev {
                        Some(DataEvent::Output(o)) => { got.extend(o); if got.len() >= limit { break; } }
                        Some(e) => eprintln!("event: {e:?}"),
                        None => break,
                    },
                    _ = &mut deadline => break,
                }
            }
            got.truncate(limit);
            std::io::stdout().write_all(format!("{}", got.escape_ascii()).as_bytes())?;
            println!();
        }
        "screen" => {
            // screen [n]: restore the n-th terminal and print what it shows.
            let n: usize = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let b = conn.bootstrap(None, None).await?;
            let t = b.data_tickets.unwrap_or_default().into_iter().nth(n).context("no such terminal")?;
            let (_ds, mut rx) = DataStream::open(endpoint.clone(), t.ticket);
            let mut term = raptor_term::Terminal::new(80, 24);
            let started = std::time::Instant::now();
            let deadline = tokio::time::sleep(Duration::from_secs(3));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    ev = rx.recv() => match ev {
                        Some(DataEvent::Snapshot(s)) => {
                            eprintln!("snapshot: {} bytes after {:?}", s.len(), started.elapsed());
                            term.restore(&s)?;
                        }
                        Some(DataEvent::Output(o)) => term.write_vt(&o),
                        Some(DataEvent::Resize { cols, rows }) => eprintln!("resize {cols}x{rows}"),
                        Some(e) => eprintln!("event: {e:?}"),
                        None => break,
                    },
                    _ = &mut deadline => break,
                }
            }
            let (c, r) = term.size();
            eprintln!("terminal {c}x{r}, title {:?}, pwd {:?}, scrollback {:?}", term.title(), term.pwd(), term.scrollbar());
            for line in term.screen_lines() {
                println!("|{line}");
            }
        }
        other => bail!("unknown command {other}"),
    }
    Ok(())
}
