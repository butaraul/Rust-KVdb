//! Write-ahead log: every mutating command is appended to `appendonly.aof`
//! (RESP-encoded, exactly like a Redis AOF) before it is applied to the
//! B-Tree. On restart the latest snapshot is loaded and the WAL is replayed
//! on top of it to recover any writes since the last snapshot.

use protocol::{as_command, parse, ParseOutcome, RespWriter};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalOp {
    Set(Vec<u8>, Vec<u8>),
    Del(Vec<u8>),
}

pub struct Wal {
    writer: BufWriter<File>,
    path: PathBuf,
}

impl Wal {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Wal {
            writer: BufWriter::new(file),
            path,
        })
    }

    pub fn append_set(&mut self, key: &[u8], value: &[u8]) -> io::Result<()> {
        let mut buf = Vec::with_capacity(key.len() + value.len() + 32);
        RespWriter::array_header(&mut buf, 3);
        RespWriter::bulk(&mut buf, b"SET");
        RespWriter::bulk(&mut buf, key);
        RespWriter::bulk(&mut buf, value);
        self.writer.write_all(&buf)?;
        self.writer.flush()
    }

    pub fn append_del(&mut self, key: &[u8]) -> io::Result<()> {
        let mut buf = Vec::with_capacity(key.len() + 16);
        RespWriter::array_header(&mut buf, 2);
        RespWriter::bulk(&mut buf, b"DEL");
        RespWriter::bulk(&mut buf, key);
        self.writer.write_all(&buf)?;
        self.writer.flush()
    }

    /// Truncates the WAL to empty after a successful snapshot.
    pub fn truncate(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        let file = self.writer.get_mut();
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads and parses every command currently in the WAL file at `path`.
    /// Returns an empty vec if the file doesn't exist yet.
    pub fn replay(path: impl AsRef<Path>) -> io::Result<Vec<WalOp>> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;

        let mut ops = Vec::new();
        let mut cursor = 0usize;
        while cursor < contents.len() {
            match parse(&contents[cursor..]) {
                Ok(ParseOutcome::Complete { value, consumed }) => {
                    cursor += consumed;
                    let cmd = match as_command(&value) {
                        Ok(c) => c,
                        Err(_) => break,
                    };
                    match cmd.as_slice() {
                        [name, key, val] if name.eq_ignore_ascii_case(b"SET") => {
                            ops.push(WalOp::Set(key.to_vec(), val.to_vec()));
                        }
                        [name, key] if name.eq_ignore_ascii_case(b"DEL") => {
                            ops.push(WalOp::Del(key.to_vec()));
                        }
                        _ => {}
                    }
                }
                Ok(ParseOutcome::Incomplete) => {
                    // Trailing partial record (e.g. crash mid-write); stop
                    // replay here, matching Redis's tolerant AOF loading.
                    break;
                }
                Err(_) => break,
            }
        }
        Ok(ops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn append_and_replay_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("appendonly.aof");
        {
            let mut wal = Wal::open(&path).unwrap();
            wal.append_set(b"a", b"1").unwrap();
            wal.append_set(b"b", b"2").unwrap();
            wal.append_del(b"a").unwrap();
        }
        let ops = Wal::replay(&path).unwrap();
        assert_eq!(
            ops,
            vec![
                WalOp::Set(b"a".to_vec(), b"1".to_vec()),
                WalOp::Set(b"b".to_vec(), b"2".to_vec()),
                WalOp::Del(b"a".to_vec()),
            ]
        );
    }

    #[test]
    fn truncate_empties_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("appendonly.aof");
        let mut wal = Wal::open(&path).unwrap();
        wal.append_set(b"a", b"1").unwrap();
        wal.truncate().unwrap();
        drop(wal);
        let ops = Wal::replay(&path).unwrap();
        assert!(ops.is_empty());
    }

    #[test]
    fn missing_file_replays_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.aof");
        assert_eq!(Wal::replay(&path).unwrap(), Vec::new());
    }
}
