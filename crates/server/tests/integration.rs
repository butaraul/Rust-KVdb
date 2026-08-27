//! End-to-end test: real TCP socket, real RESP bytes, real `Engine`
//! (WAL + B-Tree), run through the actual mio event loop.

use mio::net::TcpListener as MioTcpListener;
use persistence::Engine;
use server::resp_server;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use web::Metrics;

fn start_server() -> std::net::SocketAddr {
    let dir = tempdir().unwrap();
    let engine = Engine::open(dir.path()).unwrap();
    let metrics = Arc::new(Metrics::new());

    let listener = MioTcpListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let addr = listener.local_addr().unwrap();

    std::thread::spawn(move || {
        // Keep the tempdir alive for the lifetime of the server thread.
        let _dir = dir;
        resp_server::run_with_listener(listener, engine, metrics).unwrap();
    });

    // Give the event loop a moment to reach poll().
    std::thread::sleep(Duration::from_millis(50));
    addr
}

fn resp_command(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("*{}\r\n", parts.len()).as_bytes());
    for p in parts {
        out.extend_from_slice(format!("${}\r\n", p.len()).as_bytes());
        out.extend_from_slice(p);
        out.extend_from_slice(b"\r\n");
    }
    out
}

fn send_and_read(stream: &mut TcpStream, cmd: &[u8]) -> String {
    stream.write_all(cmd).unwrap();
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap();
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

#[test]
fn set_get_del_over_real_socket() {
    let addr = start_server();
    let mut stream = TcpStream::connect(addr).unwrap();

    let reply = send_and_read(&mut stream, &resp_command(&[b"PING"]));
    assert_eq!(reply, "+PONG\r\n");

    let reply = send_and_read(&mut stream, &resp_command(&[b"SET", b"foo", b"bar"]));
    assert_eq!(reply, "+OK\r\n");

    let reply = send_and_read(&mut stream, &resp_command(&[b"GET", b"foo"]));
    assert_eq!(reply, "$3\r\nbar\r\n");

    let reply = send_and_read(&mut stream, &resp_command(&[b"DEL", b"foo"]));
    assert_eq!(reply, ":1\r\n");

    let reply = send_and_read(&mut stream, &resp_command(&[b"GET", b"foo"]));
    assert_eq!(reply, "$-1\r\n");
}

#[test]
fn keys_glob_over_real_socket() {
    let addr = start_server();
    let mut stream = TcpStream::connect(addr).unwrap();

    for i in 0..5 {
        let key = format!("user:{i}");
        send_and_read(&mut stream, &resp_command(&[b"SET", key.as_bytes(), b"v"]));
    }
    send_and_read(&mut stream, &resp_command(&[b"SET", b"order:1", b"v"]));

    let reply = send_and_read(&mut stream, &resp_command(&[b"KEYS", b"user:*"]));
    assert!(reply.starts_with("*5\r\n"));
    for i in 0..5 {
        assert!(reply.contains(&format!("user:{i}")));
    }
    assert!(!reply.contains("order:1"));
}

#[test]
fn many_concurrent_clients() {
    let addr = start_server();
    let mut handles = Vec::new();
    for i in 0..64 {
        handles.push(std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            let key = format!("k{i}");
            let val = format!("v{i}");
            let reply = send_and_read(
                &mut stream,
                &resp_command(&[b"SET", key.as_bytes(), val.as_bytes()]),
            );
            assert_eq!(reply, "+OK\r\n");
            let reply = send_and_read(&mut stream, &resp_command(&[b"GET", key.as_bytes()]));
            assert_eq!(reply, format!("${}\r\n{}\r\n", val.len(), val));
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
}
