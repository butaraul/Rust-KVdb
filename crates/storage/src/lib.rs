pub mod arena;
pub mod btree;
pub mod glob;

pub use arena::{Arena, Ref};
pub use btree::{BTree, BTreeStats};

use std::sync::{Arc, RwLock};

/// Shared, thread-safe handle to the storage engine used by both the RESP
/// server and the HTTP dashboard.
pub type SharedStore = Arc<RwLock<BTree>>;

pub fn new_shared_store() -> SharedStore {
    Arc::new(RwLock::new(BTree::new()))
}
