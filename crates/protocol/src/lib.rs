//! Zero-copy RESP (REdis Serialization Protocol) parser and encoder.
//!
//! Parsing borrows directly from the input buffer wherever possible
//! (`RespValue::Bulk(&[u8])`, `RespValue::Simple(&str)`), so no allocation
//! happens on the read path beyond the `Vec<RespValue>` shell for arrays.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespValue<'a> {
    Simple(&'a str),
    Error(&'a str),
    Integer(i64),
    Bulk(&'a [u8]),
    NullBulk,
    Array(Vec<RespValue<'a>>),
    NullArray,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Protocol(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Protocol(s) => write!(f, "protocol error: {s}"),
        }
    }
}
impl std::error::Error for ParseError {}

/// Result of attempting to parse one RESP frame from a buffer.
pub enum ParseOutcome<'a> {
    /// A complete value was parsed; `consumed` bytes should be dropped from
    /// the front of the buffer.
    Complete {
        value: RespValue<'a>,
        consumed: usize,
    },
    /// Not enough data yet; caller should read more bytes and retry.
    Incomplete,
}

/// Parses one RESP value from the front of `buf`. Returns `Incomplete` if
/// `buf` doesn't yet contain a full frame (the caller should read more from
/// the socket and try again) rather than erroring, since TCP is a byte
/// stream and frames can arrive split across reads.
pub fn parse(buf: &[u8]) -> Result<ParseOutcome<'_>, ParseError> {
    parse_value(buf, 0)
}

fn find_crlf(buf: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn parse_value(buf: &[u8], pos: usize) -> Result<ParseOutcome<'_>, ParseError> {
    if pos >= buf.len() {
        return Ok(ParseOutcome::Incomplete);
    }
    match buf[pos] {
        b'+' => parse_line(buf, pos + 1).map(|opt| match opt {
            Some((s, end)) => ParseOutcome::Complete {
                value: RespValue::Simple(s),
                consumed: end,
            },
            None => ParseOutcome::Incomplete,
        }),
        b'-' => parse_line(buf, pos + 1).map(|opt| match opt {
            Some((s, end)) => ParseOutcome::Complete {
                value: RespValue::Error(s),
                consumed: end,
            },
            None => ParseOutcome::Incomplete,
        }),
        b':' => match parse_line(buf, pos + 1)? {
            None => Ok(ParseOutcome::Incomplete),
            Some((s, end)) => {
                let n: i64 = s
                    .parse()
                    .map_err(|_| ParseError::Protocol(format!("invalid integer: {s}")))?;
                Ok(ParseOutcome::Complete {
                    value: RespValue::Integer(n),
                    consumed: end,
                })
            }
        },
        b'$' => match parse_line(buf, pos + 1)? {
            None => Ok(ParseOutcome::Incomplete),
            Some((s, header_end)) => {
                let len: i64 = s
                    .parse()
                    .map_err(|_| ParseError::Protocol(format!("invalid bulk length: {s}")))?;
                if len == -1 {
                    return Ok(ParseOutcome::Complete {
                        value: RespValue::NullBulk,
                        consumed: header_end,
                    });
                }
                if len < 0 {
                    return Err(ParseError::Protocol("negative bulk length".into()));
                }
                let len = len as usize;
                let data_start = header_end;
                let data_end = data_start + len;
                if buf.len() < data_end + 2 {
                    return Ok(ParseOutcome::Incomplete);
                }
                if &buf[data_end..data_end + 2] != b"\r\n" {
                    return Err(ParseError::Protocol(
                        "bulk string missing trailing CRLF".into(),
                    ));
                }
                Ok(ParseOutcome::Complete {
                    value: RespValue::Bulk(&buf[data_start..data_end]),
                    consumed: data_end + 2,
                })
            }
        },
        b'*' => match parse_line(buf, pos + 1)? {
            None => Ok(ParseOutcome::Incomplete),
            Some((s, header_end)) => {
                let count: i64 = s
                    .parse()
                    .map_err(|_| ParseError::Protocol(format!("invalid array length: {s}")))?;
                if count == -1 {
                    return Ok(ParseOutcome::Complete {
                        value: RespValue::NullArray,
                        consumed: header_end,
                    });
                }
                if count < 0 {
                    return Err(ParseError::Protocol("negative array length".into()));
                }
                let mut items = Vec::with_capacity(count as usize);
                let mut cursor = header_end;
                for _ in 0..count {
                    match parse_value(buf, cursor)? {
                        ParseOutcome::Incomplete => return Ok(ParseOutcome::Incomplete),
                        ParseOutcome::Complete { value, consumed } => {
                            items.push(value);
                            cursor = consumed;
                        }
                    }
                }
                Ok(ParseOutcome::Complete {
                    value: RespValue::Array(items),
                    consumed: cursor,
                })
            }
        },
        // Inline commands (plain text terminated by CRLF), for `nc`/telnet
        // style manual testing, à la real Redis.
        _ => match find_crlf(buf, pos) {
            None => Ok(ParseOutcome::Incomplete),
            Some(crlf) => {
                let line = std::str::from_utf8(&buf[pos..crlf])
                    .map_err(|_| ParseError::Protocol("invalid utf8 in inline command".into()))?;
                let items: Vec<RespValue<'_>> = line
                    .split_whitespace()
                    .map(|tok| RespValue::Bulk(tok.as_bytes()))
                    .collect();
                Ok(ParseOutcome::Complete {
                    value: RespValue::Array(items),
                    consumed: crlf + 2,
                })
            }
        },
    }
}

/// Reads a CRLF-terminated line starting at `pos` (not including the type
/// prefix byte), returning the line content and the offset just past the
/// CRLF. Returns `Ok(None)` if the buffer doesn't contain a full line yet.
fn parse_line(buf: &[u8], pos: usize) -> Result<Option<(&str, usize)>, ParseError> {
    match find_crlf(buf, pos) {
        None => Ok(None),
        Some(crlf) => {
            let s = std::str::from_utf8(&buf[pos..crlf])
                .map_err(|_| ParseError::Protocol("invalid utf8 in line".into()))?;
            Ok(Some((s, crlf + 2)))
        }
    }
}

// ------------------------------------------------------------------------
// Encoding: building RESP replies to send back to clients.

pub struct RespWriter;

impl RespWriter {
    pub fn simple(out: &mut Vec<u8>, s: &str) {
        out.push(b'+');
        out.extend_from_slice(s.as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    pub fn error(out: &mut Vec<u8>, s: &str) {
        out.push(b'-');
        out.extend_from_slice(s.as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    pub fn integer(out: &mut Vec<u8>, n: i64) {
        out.push(b':');
        out.extend_from_slice(n.to_string().as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    pub fn bulk(out: &mut Vec<u8>, data: &[u8]) {
        out.push(b'$');
        out.extend_from_slice(data.len().to_string().as_bytes());
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(data);
        out.extend_from_slice(b"\r\n");
    }

    pub fn null_bulk(out: &mut Vec<u8>) {
        out.extend_from_slice(b"$-1\r\n");
    }

    pub fn array_header(out: &mut Vec<u8>, len: usize) {
        out.push(b'*');
        out.extend_from_slice(len.to_string().as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    pub fn array_of_bulk(out: &mut Vec<u8>, items: &[Vec<u8>]) {
        Self::array_header(out, items.len());
        for item in items {
            Self::bulk(out, item);
        }
    }
}

/// A parsed client command: the array-of-bulk-strings shape every RESP
/// client command takes. Convenience extraction on top of [`RespValue`].
pub fn as_command<'a>(value: &'a RespValue<'a>) -> Result<Vec<&'a [u8]>, ParseError> {
    match value {
        RespValue::Array(items) => items
            .iter()
            .map(|v| match v {
                RespValue::Bulk(b) => Ok(*b),
                _ => Err(ParseError::Protocol(
                    "command arguments must be bulk strings".into(),
                )),
            })
            .collect(),
        _ => Err(ParseError::Protocol("expected array command".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(buf: &[u8]) -> (RespValue<'_>, usize) {
        match parse(buf).unwrap() {
            ParseOutcome::Complete { value, consumed } => (value, consumed),
            ParseOutcome::Incomplete => panic!("expected complete parse"),
        }
    }

    #[test]
    fn parses_simple_string() {
        let (v, n) = complete(b"+OK\r\n");
        assert_eq!(v, RespValue::Simple("OK"));
        assert_eq!(n, 5);
    }

    #[test]
    fn parses_bulk_string() {
        let (v, n) = complete(b"$5\r\nhello\r\n");
        assert_eq!(v, RespValue::Bulk(b"hello"));
        assert_eq!(n, 11);
    }

    #[test]
    fn parses_null_bulk() {
        let (v, _) = complete(b"$-1\r\n");
        assert_eq!(v, RespValue::NullBulk);
    }

    #[test]
    fn parses_command_array() {
        let buf = b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\nbar\r\n";
        let (v, n) = complete(buf);
        assert_eq!(n, buf.len());
        let cmd = as_command(&v).unwrap();
        assert_eq!(cmd, vec![b"SET".as_ref(), b"foo".as_ref(), b"bar".as_ref()]);
    }

    #[test]
    fn incomplete_buffer_waits() {
        let buf = b"*3\r\n$3\r\nSET\r\n$3\r\nfoo\r\n$3\r\nba";
        match parse(buf).unwrap() {
            ParseOutcome::Incomplete => {}
            ParseOutcome::Complete { .. } => panic!("expected incomplete"),
        }
    }

    #[test]
    fn inline_command() {
        let buf = b"PING\r\n";
        let (v, n) = complete(buf);
        assert_eq!(n, 6);
        let cmd = as_command(&v).unwrap();
        assert_eq!(cmd, vec![b"PING".as_ref()]);
    }

    #[test]
    fn parses_two_frames_back_to_back() {
        let buf = b"*1\r\n$4\r\nPING\r\n*1\r\n$4\r\nPING\r\n";
        let (v1, n1) = complete(buf);
        assert_eq!(as_command(&v1).unwrap(), vec![b"PING".as_ref()]);
        let (v2, n2) = complete(&buf[n1..]);
        assert_eq!(as_command(&v2).unwrap(), vec![b"PING".as_ref()]);
        assert_eq!(n1 + n2, buf.len());
    }

    #[test]
    fn writer_roundtrip() {
        let mut out = Vec::new();
        RespWriter::bulk(&mut out, b"hello");
        assert_eq!(out, b"$5\r\nhello\r\n");
        out.clear();
        RespWriter::array_of_bulk(&mut out, &[b"a".to_vec(), b"bb".to_vec()]);
        assert_eq!(out, b"*2\r\n$1\r\na\r\n$2\r\nbb\r\n");
    }
}
