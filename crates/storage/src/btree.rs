//! A from-scratch on-heap B-Tree (CLRS-style, minimum degree `T`). Keys and
//! values are stored as `Ref` handles into an [`Arena`]; the tree structure
//! itself (nodes, child pointers) lives in a `Vec<Node>` node pool addressed
//! by `u32` indices, with a free list so deleted node slots are reused.

use crate::arena::{Arena, Ref};
use crate::glob::glob_match;
use std::cmp::Ordering;

/// Minimum degree. Max keys per node = 2T-1, min keys per non-root node = T-1.
const T: usize = 32;
const MAX_KEYS: usize = 2 * T - 1;

struct Node {
    is_leaf: bool,
    keys: Vec<Ref>,
    vals: Vec<Ref>,
    children: Vec<u32>,
}

impl Node {
    fn empty_leaf() -> Self {
        Node {
            is_leaf: true,
            keys: Vec::new(),
            vals: Vec::new(),
            children: Vec::new(),
        }
    }
}

pub struct BTreeStats {
    pub len: usize,
    pub arena_capacity_bytes: usize,
    pub arena_live_bytes: usize,
    pub node_count: usize,
}

pub struct BTree {
    arena: Arena,
    nodes: Vec<Node>,
    free_nodes: Vec<u32>,
    root: u32,
    len: usize,
}

impl BTree {
    pub fn new() -> Self {
        BTree {
            arena: Arena::new(),
            nodes: vec![Node::empty_leaf()],
            free_nodes: Vec::new(),
            root: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn stats(&self) -> BTreeStats {
        BTreeStats {
            len: self.len,
            arena_capacity_bytes: self.arena.capacity_bytes(),
            arena_live_bytes: self.arena.live_bytes,
            node_count: self.nodes.len() - self.free_nodes.len(),
        }
    }

    // ---------------------------------------------------------------- node pool

    fn alloc_node(&mut self, node: Node) -> u32 {
        if let Some(idx) = self.free_nodes.pop() {
            self.nodes[idx as usize] = node;
            idx
        } else {
            self.nodes.push(node);
            (self.nodes.len() - 1) as u32
        }
    }

    fn free_node(&mut self, idx: u32) {
        self.nodes[idx as usize] = Node::empty_leaf();
        self.free_nodes.push(idx);
    }

    // ------------------------------------------------------------------- search

    /// Binary-searches `node_id`'s keys for `key`. Returns (index, found)
    /// where `index` is either the match position or the insertion point.
    fn locate(&self, node_id: u32, key: &[u8]) -> (usize, bool) {
        let node = &self.nodes[node_id as usize];
        let mut lo = 0usize;
        let mut hi = node.keys.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.arena.read(node.keys[mid]).cmp(key) {
                Ordering::Equal => return (mid, true),
                Ordering::Less => lo = mid + 1,
                Ordering::Greater => hi = mid,
            }
        }
        (lo, false)
    }

    fn search_exact(&self, node_id: u32, key: &[u8]) -> Option<(u32, usize)> {
        let (idx, found) = self.locate(node_id, key);
        if found {
            return Some((node_id, idx));
        }
        let node = &self.nodes[node_id as usize];
        if node.is_leaf {
            None
        } else {
            let child = node.children[idx];
            self.search_exact(child, key)
        }
    }

    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.search_exact(self.root, key)
            .map(|(nid, idx)| self.arena.read(self.nodes[nid as usize].vals[idx]).to_vec())
    }

    pub fn contains(&self, key: &[u8]) -> bool {
        self.search_exact(self.root, key).is_some()
    }

    // ------------------------------------------------------------------- insert

    /// Inserts or replaces `key` -> `value`. Returns the previous value, if any.
    pub fn set(&mut self, key: &[u8], value: &[u8]) -> Option<Vec<u8>> {
        if let Some((node_id, idx)) = self.search_exact(self.root, key) {
            let old = self.nodes[node_id as usize].vals[idx];
            let old_bytes = self.arena.read(old).to_vec();
            self.arena.dealloc(old);
            let new_ref = self.arena.alloc(value);
            self.nodes[node_id as usize].vals[idx] = new_ref;
            return Some(old_bytes);
        }

        let root = self.root;
        if self.nodes[root as usize].keys.len() == MAX_KEYS {
            let new_root_id = self.alloc_node(Node {
                is_leaf: false,
                keys: Vec::new(),
                vals: Vec::new(),
                children: vec![root],
            });
            self.split_child(new_root_id, 0);
            self.root = new_root_id;
            self.insert_nonfull(new_root_id, key, value);
        } else {
            self.insert_nonfull(root, key, value);
        }
        self.len += 1;
        None
    }

    fn insert_nonfull(&mut self, node_id: u32, key: &[u8], value: &[u8]) {
        let (pos, _) = self.locate(node_id, key);
        let is_leaf = self.nodes[node_id as usize].is_leaf;
        if is_leaf {
            let key_ref = self.arena.alloc(key);
            let val_ref = self.arena.alloc(value);
            self.nodes[node_id as usize].keys.insert(pos, key_ref);
            self.nodes[node_id as usize].vals.insert(pos, val_ref);
        } else {
            let mut child_idx = pos;
            let child_id = self.nodes[node_id as usize].children[child_idx];
            if self.nodes[child_id as usize].keys.len() == MAX_KEYS {
                self.split_child(node_id, child_idx);
                let promoted = self.nodes[node_id as usize].keys[child_idx];
                if self.arena.read(promoted) < key {
                    child_idx += 1;
                }
            }
            let child_id = self.nodes[node_id as usize].children[child_idx];
            self.insert_nonfull(child_id, key, value);
        }
    }

    /// Splits the full child at `parent.children[i]` into two nodes,
    /// promoting the middle key into `parent` at index `i`.
    fn split_child(&mut self, parent_id: u32, i: usize) {
        let child_id = self.nodes[parent_id as usize].children[i];
        let is_leaf = self.nodes[child_id as usize].is_leaf;

        let right_keys = self.nodes[child_id as usize].keys.split_off(T);
        let mid_key = self.nodes[child_id as usize].keys.pop().unwrap();
        let right_vals = self.nodes[child_id as usize].vals.split_off(T);
        let mid_val = self.nodes[child_id as usize].vals.pop().unwrap();
        let right_children = if !is_leaf {
            self.nodes[child_id as usize].children.split_off(T)
        } else {
            Vec::new()
        };

        let right_id = self.alloc_node(Node {
            is_leaf,
            keys: right_keys,
            vals: right_vals,
            children: right_children,
        });

        let parent = &mut self.nodes[parent_id as usize];
        parent.children.insert(i + 1, right_id);
        parent.keys.insert(i, mid_key);
        parent.vals.insert(i, mid_val);
    }

    // ------------------------------------------------------------------- delete

    pub fn del(&mut self, key: &[u8]) -> Option<Vec<u8>> {
        self.search_exact(self.root, key)?;
        let removed = self.delete(self.root, key);
        if !self.nodes[self.root as usize].is_leaf && self.nodes[self.root as usize].keys.is_empty()
        {
            let old_root = self.root;
            self.root = self.nodes[old_root as usize].children[0];
            self.free_node(old_root);
        }
        if removed.is_some() {
            self.len -= 1;
        }
        removed
    }

    fn delete(&mut self, node_id: u32, key: &[u8]) -> Option<Vec<u8>> {
        let (idx, found) = self.locate(node_id, key);
        let is_leaf = self.nodes[node_id as usize].is_leaf;

        if found {
            if is_leaf {
                let k = self.nodes[node_id as usize].keys.remove(idx);
                let v = self.nodes[node_id as usize].vals.remove(idx);
                let bytes = self.arena.read(v).to_vec();
                self.arena.dealloc(k);
                self.arena.dealloc(v);
                Some(bytes)
            } else {
                self.delete_internal(node_id, idx, key)
            }
        } else {
            if is_leaf {
                return None;
            }
            self.ensure_child_has_min_keys(node_id, idx);
            let (idx2, _) = self.locate(node_id, key);
            let child = self.nodes[node_id as usize].children[idx2];
            self.delete(child, key)
        }
    }

    fn delete_internal(&mut self, node_id: u32, idx: usize, key: &[u8]) -> Option<Vec<u8>> {
        let left_child = self.nodes[node_id as usize].children[idx];
        let right_child = self.nodes[node_id as usize].children[idx + 1];

        if self.nodes[left_child as usize].keys.len() >= T {
            let (pk, pv) = self.subtree_max(left_child);
            let pk_bytes = self.arena.read(pk).to_vec();
            let pv_bytes = self.arena.read(pv).to_vec();

            let old_key_ref = self.nodes[node_id as usize].keys[idx];
            let old_val_ref = self.nodes[node_id as usize].vals[idx];
            let removed_bytes = self.arena.read(old_val_ref).to_vec();
            self.arena.dealloc(old_key_ref);
            self.arena.dealloc(old_val_ref);
            self.nodes[node_id as usize].keys[idx] = self.arena.alloc(&pk_bytes);
            self.nodes[node_id as usize].vals[idx] = self.arena.alloc(&pv_bytes);

            self.delete(left_child, &pk_bytes);
            Some(removed_bytes)
        } else if self.nodes[right_child as usize].keys.len() >= T {
            let (sk, sv) = self.subtree_min(right_child);
            let sk_bytes = self.arena.read(sk).to_vec();
            let sv_bytes = self.arena.read(sv).to_vec();

            let old_key_ref = self.nodes[node_id as usize].keys[idx];
            let old_val_ref = self.nodes[node_id as usize].vals[idx];
            let removed_bytes = self.arena.read(old_val_ref).to_vec();
            self.arena.dealloc(old_key_ref);
            self.arena.dealloc(old_val_ref);
            self.nodes[node_id as usize].keys[idx] = self.arena.alloc(&sk_bytes);
            self.nodes[node_id as usize].vals[idx] = self.arena.alloc(&sv_bytes);

            self.delete(right_child, &sk_bytes);
            Some(removed_bytes)
        } else {
            self.merge_children(node_id, idx);
            let merged = self.nodes[node_id as usize].children[idx];
            self.delete(merged, key)
        }
    }

    fn subtree_max(&self, node_id: u32) -> (Ref, Ref) {
        let mut cur = node_id;
        loop {
            let node = &self.nodes[cur as usize];
            if node.is_leaf {
                let last = node.keys.len() - 1;
                return (node.keys[last], node.vals[last]);
            }
            cur = *node.children.last().unwrap();
        }
    }

    fn subtree_min(&self, node_id: u32) -> (Ref, Ref) {
        let mut cur = node_id;
        loop {
            let node = &self.nodes[cur as usize];
            if node.is_leaf {
                return (node.keys[0], node.vals[0]);
            }
            cur = node.children[0];
        }
    }

    /// Ensures `node.children[idx]` has >= T keys before we descend into it,
    /// via sibling borrow or merge (CLRS case 3).
    fn ensure_child_has_min_keys(&mut self, node_id: u32, idx: usize) {
        let child = self.nodes[node_id as usize].children[idx];
        if self.nodes[child as usize].keys.len() >= T {
            return;
        }
        let num_children = self.nodes[node_id as usize].children.len();
        let has_left_sib = idx > 0;
        let has_right_sib = idx + 1 < num_children;

        let left_len = if has_left_sib {
            self.nodes[self.nodes[node_id as usize].children[idx - 1] as usize]
                .keys
                .len()
        } else {
            0
        };
        let right_len = if has_right_sib {
            self.nodes[self.nodes[node_id as usize].children[idx + 1] as usize]
                .keys
                .len()
        } else {
            0
        };

        if has_left_sib && left_len >= T {
            self.borrow_from_left(node_id, idx);
        } else if has_right_sib && right_len >= T {
            self.borrow_from_right(node_id, idx);
        } else if has_left_sib {
            self.merge_children(node_id, idx - 1);
        } else {
            self.merge_children(node_id, idx);
        }
    }

    fn borrow_from_left(&mut self, node_id: u32, idx: usize) {
        let left_id = self.nodes[node_id as usize].children[idx - 1];
        let child_id = self.nodes[node_id as usize].children[idx];

        let sep_key = self.nodes[node_id as usize].keys[idx - 1];
        let sep_val = self.nodes[node_id as usize].vals[idx - 1];
        self.nodes[child_id as usize].keys.insert(0, sep_key);
        self.nodes[child_id as usize].vals.insert(0, sep_val);

        let left_last_key = self.nodes[left_id as usize].keys.pop().unwrap();
        let left_last_val = self.nodes[left_id as usize].vals.pop().unwrap();
        self.nodes[node_id as usize].keys[idx - 1] = left_last_key;
        self.nodes[node_id as usize].vals[idx - 1] = left_last_val;

        if !self.nodes[left_id as usize].is_leaf {
            let moved_child = self.nodes[left_id as usize].children.pop().unwrap();
            self.nodes[child_id as usize]
                .children
                .insert(0, moved_child);
        }
    }

    fn borrow_from_right(&mut self, node_id: u32, idx: usize) {
        let child_id = self.nodes[node_id as usize].children[idx];
        let right_id = self.nodes[node_id as usize].children[idx + 1];

        let sep_key = self.nodes[node_id as usize].keys[idx];
        let sep_val = self.nodes[node_id as usize].vals[idx];
        self.nodes[child_id as usize].keys.push(sep_key);
        self.nodes[child_id as usize].vals.push(sep_val);

        let right_first_key = self.nodes[right_id as usize].keys.remove(0);
        let right_first_val = self.nodes[right_id as usize].vals.remove(0);
        self.nodes[node_id as usize].keys[idx] = right_first_key;
        self.nodes[node_id as usize].vals[idx] = right_first_val;

        if !self.nodes[right_id as usize].is_leaf {
            let moved_child = self.nodes[right_id as usize].children.remove(0);
            self.nodes[child_id as usize].children.push(moved_child);
        }
    }

    /// Merges `children[idx]`, the separator `keys[idx]`, and `children[idx+1]`
    /// into a single node stored at `children[idx]`.
    fn merge_children(&mut self, node_id: u32, idx: usize) {
        let left_id = self.nodes[node_id as usize].children[idx];
        let right_id = self.nodes[node_id as usize].children[idx + 1];

        let sep_key = self.nodes[node_id as usize].keys.remove(idx);
        let sep_val = self.nodes[node_id as usize].vals.remove(idx);
        self.nodes[node_id as usize].children.remove(idx + 1);

        let mut right_node =
            std::mem::replace(&mut self.nodes[right_id as usize], Node::empty_leaf());

        self.nodes[left_id as usize].keys.push(sep_key);
        self.nodes[left_id as usize].vals.push(sep_val);
        self.nodes[left_id as usize]
            .keys
            .append(&mut right_node.keys);
        self.nodes[left_id as usize]
            .vals
            .append(&mut right_node.vals);
        self.nodes[left_id as usize]
            .children
            .append(&mut right_node.children);

        self.free_node(right_id);
    }

    // ---------------------------------------------------------------- iteration

    /// In-order traversal over every (key, value) pair.
    pub fn for_each<F: FnMut(&[u8], &[u8])>(&self, mut f: F) {
        self.for_each_node(self.root, &mut f);
    }

    fn for_each_node<F: FnMut(&[u8], &[u8])>(&self, node_id: u32, f: &mut F) {
        let node = &self.nodes[node_id as usize];
        if node.is_leaf {
            for i in 0..node.keys.len() {
                f(self.arena.read(node.keys[i]), self.arena.read(node.vals[i]));
            }
        } else {
            for i in 0..node.keys.len() {
                self.for_each_node(node.children[i], f);
                f(self.arena.read(node.keys[i]), self.arena.read(node.vals[i]));
            }
            self.for_each_node(*node.children.last().unwrap(), f);
        }
    }

    /// Returns all keys whose bytes match the given glob pattern.
    pub fn keys_matching(&self, pattern: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        self.for_each(|k, _| {
            if glob_match(pattern, k) {
                out.push(k.to_vec());
            }
        });
        out
    }
}

impl Default for BTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::seq::SliceRandom;
    use rand::Rng;
    use std::collections::BTreeMap as StdMap;

    #[test]
    fn basic_set_get_del() {
        let mut t = BTree::new();
        assert_eq!(t.get(b"a"), None);
        assert_eq!(t.set(b"a", b"1"), None);
        assert_eq!(t.get(b"a"), Some(b"1".to_vec()));
        assert_eq!(t.set(b"a", b"2"), Some(b"1".to_vec()));
        assert_eq!(t.get(b"a"), Some(b"2".to_vec()));
        assert_eq!(t.del(b"a"), Some(b"2".to_vec()));
        assert_eq!(t.get(b"a"), None);
        assert_eq!(t.del(b"a"), None);
    }

    #[test]
    fn many_inserts_and_split() {
        let mut t = BTree::new();
        for i in 0..5000i32 {
            let k = format!("key{i:06}");
            let v = format!("val{i}");
            t.set(k.as_bytes(), v.as_bytes());
        }
        assert_eq!(t.len(), 5000);
        for i in 0..5000i32 {
            let k = format!("key{i:06}");
            let v = format!("val{i}");
            assert_eq!(t.get(k.as_bytes()), Some(v.into_bytes()));
        }
    }

    #[test]
    fn random_insert_delete_matches_reference() {
        let mut rng = rand::thread_rng();
        let mut t = BTree::new();
        let mut reference: StdMap<Vec<u8>, Vec<u8>> = StdMap::new();

        let keys: Vec<Vec<u8>> = (0..3000).map(|i| format!("k{i}").into_bytes()).collect();

        for _ in 0..20000 {
            let k = keys.choose(&mut rng).unwrap().clone();
            if rng.gen_bool(0.7) {
                let v = format!("v{}", rng.gen::<u32>()).into_bytes();
                let expect = reference.insert(k.clone(), v.clone());
                let got = t.set(&k, &v);
                assert_eq!(got, expect);
            } else {
                let expect = reference.remove(&k);
                let got = t.del(&k);
                assert_eq!(got, expect);
            }
        }

        assert_eq!(t.len(), reference.len());
        for (k, v) in reference.iter() {
            assert_eq!(t.get(k), Some(v.clone()));
        }
    }

    #[test]
    fn keys_glob_pattern() {
        let mut t = BTree::new();
        t.set(b"user:1", b"a");
        t.set(b"user:2", b"b");
        t.set(b"order:1", b"c");
        let mut matched = t.keys_matching(b"user:*");
        matched.sort();
        assert_eq!(matched, vec![b"user:1".to_vec(), b"user:2".to_vec()]);
    }

    #[test]
    fn delete_all_shrinks_to_empty() {
        let mut t = BTree::new();
        for i in 0..2000i32 {
            t.set(format!("k{i}").as_bytes(), b"v");
        }
        let mut keys: Vec<i32> = (0..2000).collect();
        let mut rng = rand::thread_rng();
        keys.shuffle(&mut rng);
        for i in keys {
            assert!(t.del(format!("k{i}").as_bytes()).is_some());
        }
        assert_eq!(t.len(), 0);
        assert!(t.is_empty());
        assert_eq!(t.stats().arena_live_bytes, 0);
    }
}
