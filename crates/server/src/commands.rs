//! Maps parsed RESP commands onto the storage engine and encodes replies.

use persistence::Engine;
use protocol::RespWriter;
use std::sync::Arc;
use web::Metrics;

pub fn dispatch(engine: &Arc<Engine>, metrics: &Arc<Metrics>, cmd: &[&[u8]], out: &mut Vec<u8>) {
    if cmd.is_empty() {
        RespWriter::error(out, "ERR empty command");
        return;
    }
    let name = cmd[0].to_ascii_uppercase();
    match name.as_slice() {
        b"PING" => {
            if cmd.len() >= 2 {
                RespWriter::bulk(out, cmd[1]);
            } else {
                RespWriter::simple(out, "PONG");
            }
        }
        b"SET" => {
            if cmd.len() != 3 {
                RespWriter::error(out, "ERR wrong number of arguments for 'SET'");
                return;
            }
            match engine.set(cmd[1], cmd[2]) {
                Ok(_) => {
                    metrics.record_op(cmd[1]);
                    RespWriter::simple(out, "OK");
                }
                Err(e) => RespWriter::error(out, &format!("ERR {e}")),
            }
        }
        b"GET" => {
            if cmd.len() != 2 {
                RespWriter::error(out, "ERR wrong number of arguments for 'GET'");
                return;
            }
            metrics.record_op(cmd[1]);
            match engine.get(cmd[1]) {
                Some(v) => RespWriter::bulk(out, &v),
                None => RespWriter::null_bulk(out),
            }
        }
        b"DEL" => {
            if cmd.len() != 2 {
                RespWriter::error(out, "ERR wrong number of arguments for 'DEL'");
                return;
            }
            metrics.record_op(cmd[1]);
            match engine.del(cmd[1]) {
                Ok(Some(_)) => RespWriter::integer(out, 1),
                Ok(None) => RespWriter::integer(out, 0),
                Err(e) => RespWriter::error(out, &format!("ERR {e}")),
            }
        }
        b"KEYS" => {
            if cmd.len() != 2 {
                RespWriter::error(out, "ERR wrong number of arguments for 'KEYS'");
                return;
            }
            metrics.record_op(b"__KEYS__");
            let matches = engine.keys(cmd[1]);
            RespWriter::array_of_bulk(out, &matches);
        }
        b"DBSIZE" => {
            RespWriter::integer(out, engine.len() as i64);
        }
        _ => {
            let unknown = String::from_utf8_lossy(cmd[0]);
            RespWriter::error(out, &format!("ERR unknown command '{unknown}'"));
        }
    }
}
