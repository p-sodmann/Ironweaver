use std::mem::size_of;

use super::{hash_table_bytes, EdgeId, IxMap};

/// Edge id -> edge slot. Ids come from a counter, so they are dense: most
/// live in a flat table indexed by id (sequential, cache friendly, 4 bytes
/// per id); ids far beyond the number of edges (explicit ids from replayed
/// ops or hand-written files) go to a hash map, so memory stays bounded.
#[derive(Clone, Debug, Default)]
pub(super) struct EdgeIndex {
    /// `slot + 1` of the edge with id `i`, 0 for none.
    dense: Vec<u32>,
    sparse: IxMap<EdgeId, u32>,
}

impl EdgeIndex {
    pub(super) fn with_capacity(edges: usize) -> Self {
        Self { dense: Vec::with_capacity(edges), sparse: IxMap::default() }
    }

    /// Heap storage, excluding the inline size counted by `Graph`.
    pub(super) fn heap_bytes(&self) -> usize {
        self.dense.capacity() * size_of::<u32>() + hash_table_bytes(self.sparse.capacity(), size_of::<(EdgeId, u32)>())
    }

    pub(super) fn get(&self, id: EdgeId) -> Option<u32> {
        match self.dense.get(id.0 as usize) {
            Some(&s) if s != 0 => Some(s - 1),
            // A growing dense prefix can cover an id inserted sparsely.
            // Empty dense entries must still consult that sparse mapping.
            _ => self.sparse.get(&id).copied(),
        }
    }

    /// Record `id` -> `slot`. `edges` is the number of live edges (sizes the
    /// dense table).
    /// The caller checks that `id` is free.
    pub(super) fn insert(&mut self, id: EdgeId, slot: u32, edges: usize) {
        let i = id.0 as usize;
        let limit = (4 * edges).max(1024) as u64;
        if i < self.dense.len() || id.0 < limit {
            if i >= self.dense.len() {
                let len = (i + 1).max(2 * self.dense.len());
                self.dense.resize(len, 0);
            }
            self.dense[i] = slot + 1;
        } else {
            self.sparse.insert(id, slot);
        }
    }

    pub(super) fn remove(&mut self, id: EdgeId) {
        match self.dense.get_mut(id.0 as usize) {
            Some(s) if *s != 0 => *s = 0,
            _ => {
                self.sparse.remove(&id);
            }
        }
    }

    pub(super) fn reserve(&mut self, ids: u64) {
        if ids <= (u32::MAX as u64) && (ids as usize) > self.dense.len() {
            self.dense.resize(ids as usize, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_bytes_counts_reserved_dense_and_sparse_storage() {
        let mut index = EdgeIndex::with_capacity(64);
        assert!(index.dense.is_empty());
        assert_eq!(index.dense.capacity(), 64);
        assert_eq!(index.heap_bytes(), 64 * size_of::<u32>());
        index.insert(EdgeId(1_000_000), 0, 1);
        let expected = 64 * size_of::<u32>() + hash_table_bytes(index.sparse.capacity(), size_of::<(EdgeId, u32)>());
        assert_eq!(index.heap_bytes(), expected);
        index.remove(EdgeId(1_000_000));
        // Removing entries retains the allocations, which still count.
        assert_eq!(index.heap_bytes(), expected);
        index.reserve(2048);
        assert_eq!(
            index.heap_bytes(),
            index.dense.capacity() * size_of::<u32>()
                + hash_table_bytes(index.sparse.capacity(), size_of::<(EdgeId, u32)>())
        );
    }

    #[test]
    fn sequential_ids_keep_slot_zero_and_removed_ids_distinct() {
        let mut index = EdgeIndex::default();
        for id in 0..2048 {
            index.insert(EdgeId(id), id as u32, id as usize + 1);
        }
        assert!(index.sparse.is_empty());
        for id in 0..2048 {
            assert_eq!(index.get(EdgeId(id)), Some(id as u32));
        }
        index.remove(EdgeId(0));
        index.remove(EdgeId(1024));
        assert_eq!(index.get(EdgeId(0)), None);
        assert_eq!(index.get(EdgeId(1024)), None);
        assert_eq!(index.get(EdgeId(1025)), Some(1025));
    }

    #[test]
    fn explicit_ids_obey_the_dense_growth_threshold() {
        let mut index = EdgeIndex::default();
        index.insert(EdgeId(1023), 0, 1);
        assert_eq!(index.dense.len(), 1024);
        index.insert(EdgeId(1024), 1, 1);
        index.insert(EdgeId(u64::MAX), 2, 1);
        assert_eq!(index.dense.len(), 1024);
        assert_eq!(index.get(EdgeId(1023)), Some(0));
        assert_eq!(index.get(EdgeId(1024)), Some(1));
        assert_eq!(index.get(EdgeId(u64::MAX)), Some(2));
        index.remove(EdgeId(u64::MAX));
        assert_eq!(index.get(EdgeId(u64::MAX)), None);
        // Once the live-edge count makes the threshold larger, an id
        // just below it goes dense, while the threshold itself stays sparse.
        index.insert(EdgeId(2048), 3, 512);
        index.insert(EdgeId(2047), 4, 512);
        assert_eq!(index.dense.len(), 2048);
        assert_eq!(index.sparse.get(&EdgeId(2048)), Some(&3));
        assert_eq!(index.get(EdgeId(2047)), Some(4));
    }

    #[test]
    fn dense_growth_preserves_sparse_lookup_removal_and_cloning() {
        let mut index = EdgeIndex::default();
        index.insert(EdgeId(1500), 0, 1);
        index.insert(EdgeId(1600), 1, 401);
        assert!(index.dense.len() > 1500);
        assert_eq!(index.dense[1500], 0);
        assert_eq!(index.get(EdgeId(1500)), Some(0));
        let mut cloned = index.clone();
        cloned.remove(EdgeId(1500));
        cloned.remove(EdgeId(1600));
        assert_eq!(cloned.get(EdgeId(1500)), None);
        assert_eq!(cloned.get(EdgeId(1600)), None);
        assert_eq!(index.get(EdgeId(1500)), Some(0));
        assert_eq!(index.get(EdgeId(1600)), Some(1));
        cloned.insert(EdgeId(1500), 2, 1);
        assert_eq!(cloned.get(EdgeId(1500)), Some(2));
    }

    #[test]
    fn reservation_preserves_entries_and_ignores_out_of_range_sizes() {
        let mut index = EdgeIndex::default();
        index.insert(EdgeId(0), 0, 1);
        index.insert(EdgeId(1500), 1, 1);
        index.reserve(2000);
        assert_eq!(index.dense.len(), 2000);
        assert_eq!(index.get(EdgeId(0)), Some(0));
        assert_eq!(index.get(EdgeId(1500)), Some(1));
        index.reserve(10);
        index.reserve(u32::MAX as u64 + 1);
        assert_eq!(index.dense.len(), 2000);
        assert_eq!(index.get(EdgeId(1999)), None);
        index.remove(EdgeId(1500));
        assert_eq!(index.get(EdgeId(1500)), None);
    }
}
