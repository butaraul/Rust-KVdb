//! Non-blocking RESP server built directly on `mio` (epoll on Linux,
//! kqueue on macOS/BSD) — no async runtime involved on this side. Handles
//! up to `MAX_CLIENTS` concurrent connections with a single-threaded
//! readiness loop, à la Redis's own event loop.

use crate::commands::dispatch;
use crate::conn::Connection;
use mio::event::Event;
use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use persistence::Engine;
use protocol::{as_command, parse, ParseOutcome};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use web::Metrics;

const SERVER_TOKEN: Token = Token(0);
pub const MAX_CLIENTS: usize = 1000;

pub fn run(addr: SocketAddr, engine: Arc<Engine>, metrics: Arc<Metrics>) -> io::Result<()> {
    let listener = TcpListener::bind(addr)?;
    run_with_listener(listener, engine, metrics)
}

/// Runs the event loop against an already-bound listener. Split out from
/// [`run`] so tests (and anything else that wants to know the actual bound
/// port) can bind `127.0.0.1:0`, read the OS-assigned address back via
/// `TcpListener::local_addr`, and only then hand the listener to the loop.
pub fn run_with_listener(
    mut listener: TcpListener,
    engine: Arc<Engine>,
    metrics: Arc<Metrics>,
) -> io::Result<()> {
    let addr = listener.local_addr()?;
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(1024);

    poll.registry()
        .register(&mut listener, SERVER_TOKEN, Interest::READABLE)?;

    let mut conns: HashMap<Token, Connection> = HashMap::new();
    let mut next_token = 1usize;

    tracing::info!(%addr, "RESP server listening (mio, max {MAX_CLIENTS} clients)");

    loop {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            match event.token() {
                SERVER_TOKEN => {
                    accept_loop(&listener, poll.registry(), &mut conns, &mut next_token)?;
                }
                token => {
                    if let Err(e) = handle_client_event(
                        token,
                        event,
                        &mut conns,
                        poll.registry(),
                        &engine,
                        &metrics,
                    ) {
                        tracing::debug!(?token, error = %e, "client connection error");
                    }
                    if conns.get(&token).map(|c| c.closing).unwrap_or(false) {
                        if let Some(mut c) = conns.remove(&token) {
                            let _ = poll.registry().deregister(&mut c.stream);
                        }
                    }
                }
            }
        }
    }
}

fn accept_loop(
    listener: &TcpListener,
    registry: &mio::Registry,
    conns: &mut HashMap<Token, Connection>,
    next_token: &mut usize,
) -> io::Result<()> {
    loop {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if conns.len() >= MAX_CLIENTS {
                    tracing::warn!(%peer, "connection limit reached, refusing client");
                    // Best-effort: just drop it; the stream closes on drop.
                    continue;
                }
                let token = Token(*next_token);
                *next_token += 1;
                registry.register(&mut stream, token, Interest::READABLE)?;
                conns.insert(token, Connection::new(stream));
                tracing::debug!(%peer, ?token, "client connected");
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

fn handle_client_event(
    token: Token,
    event: &Event,
    conns: &mut HashMap<Token, Connection>,
    registry: &mio::Registry,
    engine: &Arc<Engine>,
    metrics: &Arc<Metrics>,
) -> io::Result<()> {
    let conn = match conns.get_mut(&token) {
        Some(c) => c,
        None => return Ok(()),
    };

    if event.is_readable() {
        let peer_closed = conn.read_available()?;
        process_buffered_commands(conn, engine, metrics);
        if peer_closed {
            conn.closing = true;
        }
    }

    if conn.has_pending_write() {
        conn.flush()?;
    }

    let desired = if conn.has_pending_write() {
        Interest::READABLE | Interest::WRITABLE
    } else {
        Interest::READABLE
    };
    registry.reregister(&mut conn.stream, token, desired)?;

    Ok(())
}

fn process_buffered_commands(conn: &mut Connection, engine: &Arc<Engine>, metrics: &Arc<Metrics>) {
    let mut consumed_total = 0usize;
    loop {
        let remaining = &conn.read_buf[consumed_total..];
        if remaining.is_empty() {
            break;
        }
        match parse(remaining) {
            Ok(ParseOutcome::Complete { value, consumed }) => {
                consumed_total += consumed;
                match as_command(&value) {
                    Ok(cmd) => {
                        let mut reply = Vec::new();
                        dispatch(engine, metrics, &cmd, &mut reply);
                        conn.queue_write(&reply);
                    }
                    Err(e) => {
                        let mut reply = Vec::new();
                        protocol::RespWriter::error(&mut reply, &format!("ERR {e}"));
                        conn.queue_write(&reply);
                    }
                }
            }
            Ok(ParseOutcome::Incomplete) => break,
            Err(e) => {
                let mut reply = Vec::new();
                protocol::RespWriter::error(&mut reply, &format!("ERR {e}"));
                conn.queue_write(&reply);
                conn.closing = true;
                break;
            }
        }
    }
    if consumed_total > 0 {
        conn.read_buf.drain(0..consumed_total);
    }
}
