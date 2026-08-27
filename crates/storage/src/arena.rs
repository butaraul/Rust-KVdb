//! A simple first-fit, coalescing free-list allocator that carves byte
//! ranges out of a single `Vec<u8>`. All keys and values stored in the
//! [`crate::btree::BTree`] live in this arena; the tree itself only ever
//! holds `Ref { offset, len }` handles into it.

/// A handle to a byte range inside an [`Arena`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ref {
    pub offset: u32,
    pub len: u32,
}

impl Ref {
    pub const NULL: Ref = Ref { offset: 0, len: 0 };
}

#[derive(Debug, Clone, Copy)]
struct FreeBlock {
    offset: u32,
    size: u32,
}

pub struct Arena {
    data: Vec<u8>,
    /// Free blocks kept sorted by offset so adjacent blocks can be
    /// coalesced in O(1) after insertion point is found.
    free_list: Vec<FreeBlock>,
    pub live_bytes: usize,
}

impl Arena {
    pub fn new() -> Self {
        Arena {
            data: Vec::new(),
            free_list: Vec::new(),
            live_bytes: 0,
        }
    }

    pub fn capacity_bytes(&self) -> usize {
        self.data.len()
    }

    /// Allocate space for `bytes` and copy them in, returning a `Ref`.
    pub fn alloc(&mut self, bytes: &[u8]) -> Ref {
        let size = bytes.len() as u32;
        if size == 0 {
            return Ref::NULL;
        }
        let offset = self.find_or_grow(size);
        self.data[offset as usize..offset as usize + size as usize].copy_from_slice(bytes);
        self.live_bytes += size as usize;
        Ref { offset, len: size }
    }

    /// Release the bytes referenced by `r` back to the free list, coalescing
    /// with neighboring free blocks where possible.
    pub fn dealloc(&mut self, r: Ref) {
        if r.len == 0 {
            return;
        }
        self.live_bytes -= r.len as usize;
        let pos = self.free_list.partition_point(|b| b.offset < r.offset);

        let mut new_block = FreeBlock {
            offset: r.offset,
            size: r.len,
        };

        // Try to merge with the following block.
        if pos < self.free_list.len() {
            let next = self.free_list[pos];
            if new_block.offset + new_block.size == next.offset {
                new_block.size += next.size;
                self.free_list.remove(pos);
            }
        }
        // Try to merge with the preceding block.
        if pos > 0 {
            let prev_idx = pos - 1;
            let prev = self.free_list[prev_idx];
            if prev.offset + prev.size == new_block.offset {
                self.free_list[prev_idx].size += new_block.size;
                return;
            }
        }
        self.free_list.insert(pos, new_block);
    }

    pub fn read(&self, r: Ref) -> &[u8] {
        if r.len == 0 {
            return &[];
        }
        &self.data[r.offset as usize..r.offset as usize + r.len as usize]
    }

    fn find_or_grow(&mut self, size: u32) -> u32 {
        if let Some((idx, block)) = self
            .free_list
            .iter()
            .enumerate()
            .find(|(_, b)| b.size >= size)
            .map(|(i, b)| (i, *b))
        {
            if block.size == size {
                self.free_list.remove(idx);
            } else {
                self.free_list[idx].offset += size;
                self.free_list[idx].size -= size;
            }
            return block.offset;
        }
        let offset = self.data.len() as u32;
        self.data.resize(self.data.len() + size as usize, 0);
        offset
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_read_roundtrip() {
        let mut a = Arena::new();
        let r1 = a.alloc(b"hello");
        let r2 = a.alloc(b"world!!");
        assert_eq!(a.read(r1), b"hello");
        assert_eq!(a.read(r2), b"world!!");
    }

    #[test]
    fn dealloc_reuses_space() {
        let mut a = Arena::new();
        let r1 = a.alloc(b"0123456789");
        let cap_before = a.capacity_bytes();
        a.dealloc(r1);
        let r2 = a.alloc(b"abcdefghij");
        assert_eq!(a.capacity_bytes(), cap_before);
        assert_eq!(a.read(r2), b"abcdefghij");
    }

    #[test]
    fn coalesces_adjacent_free_blocks() {
        let mut a = Arena::new();
        let r1 = a.alloc(b"aaaaa");
        let r2 = a.alloc(b"bbbbb");
        let r3 = a.alloc(b"ccccc");
        a.dealloc(r1);
        a.dealloc(r3);
        a.dealloc(r2);
        assert_eq!(a.free_list.len(), 1);
        assert_eq!(a.free_list[0].size, 15);
    }
}
