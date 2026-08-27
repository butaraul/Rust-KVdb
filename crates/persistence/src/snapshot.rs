//! Point-in-time snapshot of the whole B-Tree, written to `dump.rdb`.
//!
//! Format (little-endian):
//! ```text
//! magic:   8 bytes   b"KVDBSNP1"
//! count:   u64
//! entries: repeated { key_len: u32, key: [u8], val_len: u32, val: [u8] }
//! ```
//! Written to a temp file and renamed into place so a crash mid-write never
//! corrupts the previous snapshot.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use storage::BTree;

const MAGIC: &[u8; 8] = b"KVDBSNP1";

pub struct Snapshot;

impl Snapshot {
    pub fn save(tree: &BTree, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        let tmp_path = path.with_extension("tmp");
        {
            let file = File::create(&tmp_path)?;
            let mut w = BufWriter::new(file);
            w.write_all(MAGIC)?;
            w.write_all(&(tree.len() as u64).to_le_bytes())?;

            let mut io_err: Option<io::Error> = None;
            tree.for_each(|k, v| {
                if io_err.is_some() {
                    return;
                }
                if let Err(e) = (|| -> io::Result<()> {
                    w.write_all(&(k.len() as u32).to_le_bytes())?;
                    w.write_all(k)?;
                    w.write_all(&(v.len() as u32).to_le_bytes())?;
                    w.write_all(v)?;
                    Ok(())
                })() {
                    io_err = Some(e);
                }
            });
            if let Some(e) = io_err {
                return Err(e);
            }
            w.flush()?;
            w.get_ref().sync_all()?;
        }
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Loads a snapshot into a fresh `BTree`. Returns `Ok(None)` if no
    /// snapshot file exists yet (fresh startup).
    pub fn load(path: impl AsRef<Path>) -> io::Result<Option<BTree>> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(None);
        }
        let file = File::open(path)?;
        let mut r = BufReader::new(file);

        let mut magic = [0u8; 8];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "bad snapshot magic",
            ));
        }
        let mut count_buf = [0u8; 8];
        r.read_exact(&mut count_buf)?;
        let count = u64::from_le_bytes(count_buf);

        let mut tree = BTree::new();
        for _ in 0..count {
            let mut len_buf = [0u8; 4];
            r.read_exact(&mut len_buf)?;
            let klen = u32::from_le_bytes(len_buf) as usize;
            let mut key = vec![0u8; klen];
            r.read_exact(&mut key)?;

            r.read_exact(&mut len_buf)?;
            let vlen = u32::from_le_bytes(len_buf) as usize;
            let mut val = vec![0u8; vlen];
            r.read_exact(&mut val)?;

            tree.set(&key, &val);
        }
        Ok(Some(tree))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dump.rdb");

        let mut tree = BTree::new();
        for i in 0..500 {
            tree.set(format!("k{i}").as_bytes(), format!("v{i}").as_bytes());
        }
        Snapshot::save(&tree, &path).unwrap();

        let loaded = Snapshot::load(&path).unwrap().unwrap();
        assert_eq!(loaded.len(), 500);
        for i in 0..500 {
            assert_eq!(
                loaded.get(format!("k{i}").as_bytes()),
                Some(format!("v{i}").into_bytes())
            );
        }
    }

    #[test]
    fn load_missing_returns_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dump.rdb");
        assert!(Snapshot::load(&path).unwrap().is_none());
    }
}
