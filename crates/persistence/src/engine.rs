use crate::snapshot::Snapshot;
use crate::wal::{Wal, WalOp};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use storage::{BTree, BTreeStats};

/// Ties the B-Tree, WAL, and snapshot file together.
///
/// Lock ordering is always **WAL then store**, in both the write path and
/// the snapshot path, which is what makes the snapshot-then-truncate
/// sequence safe without losing or double-applying any write: a writer
/// holds the WAL lock across *both* the WAL append and the B-Tree apply, so
/// by the time the snapshot loop acquires the WAL lock, every write that
/// got logged has also already been applied (and no write can start
/// applying without the snapshot loop having released the WAL lock first).
pub struct Engine {
    store: RwLock<BTree>,
    wal: Mutex<Wal>,
    data_dir: PathBuf,
}

impl Engine {
    pub fn open(data_dir: impl Into<PathBuf>) -> io::Result<Arc<Engine>> {
        let data_dir = data_dir.into();
        fs::create_dir_all(&data_dir)?;
        let snapshot_path = data_dir.join("dump.rdb");
        let wal_path = data_dir.join("appendonly.aof");

        let mut tree = Snapshot::load(&snapshot_path)?.unwrap_or_default();
        let ops = Wal::replay(&wal_path)?;
        let replayed = ops.len();
        for op in ops {
            match op {
                WalOp::Set(k, v) => {
                    tree.set(&k, &v);
                }
                WalOp::Del(k) => {
                    tree.del(&k);
                }
            }
        }
        tracing::info!(
            keys = tree.len(),
            replayed_ops = replayed,
            "storage engine loaded"
        );

        let wal = Wal::open(&wal_path)?;
        Ok(Arc::new(Engine {
            store: RwLock::new(tree),
            wal: Mutex::new(wal),
            data_dir,
        }))
    }

    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.store.read().unwrap().get(key)
    }

    pub fn set(&self, key: &[u8], value: &[u8]) -> io::Result<Option<Vec<u8>>> {
        let mut wal = self.wal.lock().unwrap();
        wal.append_set(key, value)?;
        let mut tree = self.store.write().unwrap();
        Ok(tree.set(key, value))
    }

    pub fn del(&self, key: &[u8]) -> io::Result<Option<Vec<u8>>> {
        let mut wal = self.wal.lock().unwrap();
        wal.append_del(key)?;
        let mut tree = self.store.write().unwrap();
        Ok(tree.del(key))
    }

    pub fn keys(&self, pattern: &[u8]) -> Vec<Vec<u8>> {
        self.store.read().unwrap().keys_matching(pattern)
    }

    pub fn len(&self) -> usize {
        self.store.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn stats(&self) -> BTreeStats {
        self.store.read().unwrap().stats()
    }

    /// Synchronously writes `dump.rdb` from the current store state and
    /// truncates the WAL. Safe to call concurrently with readers/writers.
    pub fn snapshot_now(&self) -> io::Result<()> {
        let path = self.data_dir.join("dump.rdb");
        let mut wal = self.wal.lock().unwrap();
        let tree = self.store.read().unwrap();
        Snapshot::save(&tree, &path)?;
        drop(tree);
        wal.truncate()?;
        Ok(())
    }

    /// Spawns a background thread that snapshots on a fixed interval
    /// (default: every 60s, per spec) for the lifetime of the process.
    pub fn spawn_snapshot_loop(self: &Arc<Self>, interval: Duration) -> JoinHandle<()> {
        let engine = Arc::clone(self);
        thread::spawn(move || loop {
            thread::sleep(interval);
            match engine.snapshot_now() {
                Ok(()) => tracing::info!("snapshot written and WAL truncated"),
                Err(e) => tracing::error!(error = %e, "snapshot failed"),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn recovers_from_snapshot_plus_wal() {
        let dir = tempdir().unwrap();
        {
            let engine = Engine::open(dir.path()).unwrap();
            engine.set(b"a", b"1").unwrap();
            engine.set(b"b", b"2").unwrap();
            engine.snapshot_now().unwrap();
            engine.set(b"c", b"3").unwrap(); // only in WAL, not in snapshot
            engine.del(b"a").unwrap();
        }
        // Reopen: should load snapshot (a=1,b=2) then replay WAL (c=3, del a).
        let engine = Engine::open(dir.path()).unwrap();
        assert_eq!(engine.get(b"a"), None);
        assert_eq!(engine.get(b"b"), Some(b"2".to_vec()));
        assert_eq!(engine.get(b"c"), Some(b"3".to_vec()));
        assert_eq!(engine.len(), 2);
    }

    #[test]
    fn snapshot_truncates_wal() {
        let dir = tempdir().unwrap();
        let engine = Engine::open(dir.path()).unwrap();
        engine.set(b"x", b"1").unwrap();
        engine.snapshot_now().unwrap();
        let ops = Wal::replay(dir.path().join("appendonly.aof")).unwrap();
        assert!(ops.is_empty());
    }
}
