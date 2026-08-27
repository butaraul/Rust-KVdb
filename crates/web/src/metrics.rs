//! In-process metrics: total op count and a per-key access counter used to
//! surface the "top 10 most accessed keys" on the dashboard.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub struct Metrics {
    total_ops: AtomicU64,
    access_counts: Mutex<HashMap<Vec<u8>, u64>>,
}

impl Metrics {
    pub fn new() -> Self {
        Metrics {
            total_ops: AtomicU64::new(0),
            access_counts: Mutex::new(HashMap::new()),
        }
    }

    pub fn record_op(&self, key: &[u8]) {
        self.total_ops.fetch_add(1, Ordering::Relaxed);
        let mut m = self.access_counts.lock().unwrap();
        *m.entry(key.to_vec()).or_insert(0) += 1;
    }

    pub fn total_ops(&self) -> u64 {
        self.total_ops.load(Ordering::Relaxed)
    }

    pub fn top_keys(&self, n: usize) -> Vec<(Vec<u8>, u64)> {
        let m = self.access_counts.lock().unwrap();
        let mut v: Vec<(Vec<u8>, u64)> = m.iter().map(|(k, c)| (k.clone(), *c)).collect();
        v.sort_unstable_by_key(|b| std::cmp::Reverse(b.1));
        v.truncate(n);
        v
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize, Clone)]
pub struct TopKey {
    pub key: String,
    pub count: u64,
}

#[derive(Serialize, Clone)]
pub struct MetricsPayload {
    pub total_ops: u64,
    pub ops_per_sec: f64,
    pub memory_bytes: u64,
    pub memory_live_bytes: u64,
    pub key_count: u64,
    pub top_keys: Vec<TopKey>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_top_keys() {
        let m = Metrics::new();
        for _ in 0..5 {
            m.record_op(b"hot");
        }
        for _ in 0..2 {
            m.record_op(b"warm");
        }
        m.record_op(b"cold");
        assert_eq!(m.total_ops(), 8);
        let top = m.top_keys(2);
        assert_eq!(top[0], (b"hot".to_vec(), 5));
        assert_eq!(top[1], (b"warm".to_vec(), 2));
    }
}
