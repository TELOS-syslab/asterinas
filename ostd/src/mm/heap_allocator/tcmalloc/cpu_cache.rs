// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_BASE_NUMBER_CLASSES, K_MAX_CPU_CACHE_SIZE, K_MAX_OVERRANGES},
    linked_list::ElasticList,
    size_class::{get_capacity, get_num_to_move, get_size, TransferBatch},
    status::{CpuCacheStat, FlowMod},
};

pub struct CpuCache {
    free_lists: [ElasticList; K_BASE_NUMBER_CLASSES],
    size: usize,
    max_size: usize,
    stat: CpuCacheStat,
    transfer_batch: Option<TransferBatch>,
    transfer_object: Option<*mut u8>,
}

impl CpuCache {
    const fn new() -> Self {
        const ARRAY_REPEAT_VALUE: ElasticList = ElasticList::new();

        Self {
            free_lists: [ARRAY_REPEAT_VALUE; K_BASE_NUMBER_CLASSES],
            size: 0,
            max_size: 0,
            stat: CpuCacheStat::Ready,
            transfer_batch: None,
            transfer_object: None,
        }
    }

    fn init(&mut self) {
        let free_lists = &mut self.free_lists;

        for (idx, free_list) in free_lists.iter_mut().enumerate() {
            free_list.init(get_capacity(idx).unwrap(), K_MAX_OVERRANGES);
        }

        self.max_size = K_MAX_CPU_CACHE_SIZE;
    }

    pub fn put_batch(&mut self, transfer_batch: Option<TransferBatch>) {
        self.transfer_batch = Some(transfer_batch.unwrap());
    }
    pub fn take_object(&mut self) -> Option<*mut u8> {
        self.transfer_object.take()
    }

    pub fn stat_handler(&mut self, stat: Option<CpuCacheStat>) -> FlowMod {
        match self.stat() {
            CpuCacheStat::Alloc(idx, align) => {
                self.alloc_aligned_object(idx, align);
            }
            CpuCacheStat::Dealloc(idx, ptr) => {
                self.dealloc_object(idx, ptr);
            }
            CpuCacheStat::Finish => {
                self.taken();
            }
            CpuCacheStat::Insufficient(idx, align) => {
                self.refill_batch(idx, align);
            }
            CpuCacheStat::Overranged(idx) => {
                self.scavenge_batch(idx);
            }
            CpuCacheStat::Oversized => {
                self.scavenge_batch(self.cold().unwrap());
            }
            CpuCacheStat::Ready => {
                self.seed(stat);
            }
            CpuCacheStat::Scavenge(idx) => {
                self.scavenged(idx);
            }
        }
        match self.stat() {
            CpuCacheStat::Finish => FlowMod::Forward,
            CpuCacheStat::Alloc(_, _)
            | CpuCacheStat::Dealloc(_, _)
            | CpuCacheStat::Overranged(_)
            | CpuCacheStat::Oversized => FlowMod::Circle,
            CpuCacheStat::Insufficient(_, _) | CpuCacheStat::Scavenge(_) => FlowMod::Backward,
            CpuCacheStat::Ready => FlowMod::Exit,
        }
    }

    fn seed(&mut self, stat: Option<CpuCacheStat>) {
        if let Some(stat) = stat {
            self.set_stat(stat);
        }
    }

    /// Allocate an aligned object from the current `CpuCache`.
    fn alloc_aligned_object(&mut self, idx: usize, align: usize) {
        match self.pop_aligned(idx, align) {
            None => self.set_stat(CpuCacheStat::Insufficient(idx, align)),
            Some(ptr) => {
                self.transfer_object = Some(ptr as *mut u8);
                self.set_stat(CpuCacheStat::Finish);
            }
        }
    }

    /// Scavenge a batch of free object to `TransferCache`.
    fn scavenge_batch(&mut self, idx: usize) {
        let mut transfer_batch = TransferBatch::new(get_num_to_move(idx).unwrap());
        loop {
            match self.pop(idx) {
                None => {
                    self.free_lists[idx].reset();
                    break;
                }
                Some(ptr) => {
                    if transfer_batch.push(ptr) {
                        break;
                    }
                }
            }
        }
        self.transfer_batch = Some(transfer_batch);
        self.set_stat(CpuCacheStat::Scavenge(idx));
    }

    fn scavenged(&mut self, idx: usize) {
        if self.transfer_batch.is_none() {
            if self.overranged(idx) {
                self.set_stat(CpuCacheStat::Overranged(idx));
            } else if self.oversized() {
                self.set_stat(CpuCacheStat::Oversized);
            } else {
                self.set_stat(CpuCacheStat::Ready);
            }
        }
    }

    /// Deallocate an object to the current `CpuCache`.
    fn dealloc_object(&mut self, idx: usize, ptr: *mut u8) {
        if self.push(idx, ptr as *mut usize) {
            self.set_stat(CpuCacheStat::Overranged(idx));
        } else if self.oversized() {
            self.set_stat(CpuCacheStat::Oversized);
        } else {
            self.set_stat(CpuCacheStat::Ready);
        }
    }

    /// Refill a batch of object from `TransferCache`.
    fn refill_batch(&mut self, idx: usize, align: usize) {
        if self.transfer_batch.is_none() {
            return;
        }
        let mut transfer_batch = self.transfer_batch.take().unwrap();
        while let Some(ptr) = transfer_batch.pop() {
            self.push(idx, ptr);
        }
        self.set_stat(CpuCacheStat::Alloc(idx, align));
    }

    fn taken(&mut self) {
        if self.transfer_object.is_none() {
            self.set_stat(CpuCacheStat::Ready);
        }
    }

    /// Return the index of the `ElasticList` with the smallest `color`.
    fn cold(&self) -> Option<usize> {
        let free_lists = &self.free_lists;
        let mut min_idx = 0usize;
        let mut min_color = core::usize::MAX;

        for (idx, free_list) in free_lists.iter().enumerate() {
            if !free_list.is_empty() && free_list.color() < min_color {
                min_idx = idx;
                min_color = free_list.color();
            }
        }

        match min_color < core::usize::MAX {
            false => None,
            true => Some(min_idx),
        }
    }

    fn push(&mut self, size_class_idx: usize, ptr: *mut usize) -> bool {
        self.free_lists[size_class_idx].push(ptr);
        self.size += get_size(size_class_idx).unwrap();
        self.free_lists[size_class_idx].overranged()
    }

    fn pop(&mut self, size_class_idx: usize) -> Option<*mut usize> {
        if let Some(ptr) = self.free_lists[size_class_idx].pop() {
            self.size -= get_size(size_class_idx).unwrap();
            Some(ptr)
        } else {
            None
        }
    }

    fn pop_aligned(&mut self, size_class_idx: usize, align: usize) -> Option<*mut usize> {
        if let Some(ptr) = self.free_lists[size_class_idx].pop_aligned(align) {
            self.size -= get_size(size_class_idx).unwrap();
            Some(ptr)
        } else {
            None
        }
    }

    fn overranged(&self, size_class_idx: usize) -> bool {
        self.free_lists[size_class_idx].overranged()
    }

    fn oversized(&self) -> bool {
        self.size > self.max_size
    }

    pub fn stat(&self) -> CpuCacheStat {
        self.stat.clone()
    }

    pub fn set_stat(&mut self, stat: CpuCacheStat) {
        self.stat = stat;
    }
}

/// Constant `C` refers to the number of logical CPUs.
pub struct CpuCaches<const C: usize> {
    cpu_caches: [CpuCache; C],
}

impl<const C: usize> CpuCaches<C> {
    pub const fn new() -> Self {
        const ARRAY_REPEAT_VALUE: CpuCache = CpuCache::new();

        Self {
            cpu_caches: [ARRAY_REPEAT_VALUE; C],
        }
    }

    pub fn init(&mut self) {
        let cpu_caches = &mut self.cpu_caches;

        for cpu_cache in cpu_caches.iter_mut() {
            cpu_cache.init();
        }
    }

    pub fn get_current_cpu_cache(&mut self, cpu: usize) -> &mut CpuCache {
        assert_eq!(cpu < C, true);

        &mut self.cpu_caches[cpu]
    }
}
