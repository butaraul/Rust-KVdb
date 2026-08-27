//! Per-client connection state for the mio event loop: a read buffer that
//! frames RESP commands off the front, and a write buffer for replies that
//! didn't fit in one non-blocking `write()` call.

use mio::net::TcpStream;
use std::io::{self, ErrorKind, Read, Write};

pub struct Connection {
    pub stream: TcpStream,
    pub read_buf: Vec<u8>,
    pub write_buf: Vec<u8>,
    pub write_pos: usize,
    pub closing: bool,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Self {
        Connection {
            stream,
            read_buf: Vec::with_capacity(4096),
            write_buf: Vec::new(),
            write_pos: 0,
            closing: false,
        }
    }

    /// Reads as much as is available without blocking. Returns `Ok(true)`
    /// if the peer closed the connection.
    pub fn read_available(&mut self) -> io::Result<bool> {
        let mut tmp = [0u8; 16 * 1024];
        loop {
            match self.stream.read(&mut tmp) {
                Ok(0) => return Ok(true),
                Ok(n) => self.read_buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    pub fn queue_write(&mut self, data: &[u8]) {
        self.write_buf.extend_from_slice(data);
    }

    /// Flushes as much of `write_buf` as the socket accepts without
    /// blocking. Returns `Ok(true)` if everything was flushed.
    pub fn flush(&mut self) -> io::Result<bool> {
        while self.write_pos < self.write_buf.len() {
            match self.stream.write(&self.write_buf[self.write_pos..]) {
                Ok(0) => return Err(io::Error::new(ErrorKind::WriteZero, "write returned 0")),
                Ok(n) => self.write_pos += n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        self.write_buf.clear();
        self.write_pos = 0;
        Ok(true)
    }

    pub fn has_pending_write(&self) -> bool {
        self.write_pos < self.write_buf.len()
    }
}
