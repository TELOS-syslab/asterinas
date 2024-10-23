// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_BASE_NUMBER_CLASSES, K_FULL_SCALE, K_PAGE_SIZE},
    linked_list::{BoundedList, BoundedLists},
    size_class::{get_num_to_move, get_pages, get_size, Span, TransferBatch},
    status::{FlowMod, TransferCacheStat},
};

pub struct TransferCache {
    free_lists: BoundedLists,
    num: usize,
    full_num: usize,
    size_class_idx: usize,
    stat: TransferCacheStat,
    transfer_batch: Option<TransferBatch>,
    transfer_span: Option<Span>,
    transfer_list: Option<*mut BoundedList>,
}

impl TransferCache {
    const fn new() -> Self {
        Self {
            free_lists: BoundedLists::new(),
            num: 0,
            full_num: 0,
            size_class_idx: 0,
            stat: TransferCacheStat::Ready,
            transfer_batch: None,
            transfer_span: None,
            transfer_list: None,
        }
    }

    fn init(&mut self, size_class_idx: usize) {
        self.size_class_idx = size_class_idx;
    }

    pub fn put_batch(&mut self, transfer_batch: Option<TransferBatch>) {
        self.transfer_batch = Some(transfer_batch.unwrap());
    }

    pub fn take_batch(&mut self) -> Option<TransferBatch> {
        self.transfer_batch.take()
    }

    pub fn put_span(&mut self, transfer_span: Option<Span>) {
        self.transfer_span = Some(transfer_span.unwrap());
    }

    pub fn put_list(&mut self, transfer_list: Option<*mut BoundedList>) {
        self.transfer_list = Some(transfer_list.unwrap());
    }

    pub fn stat_handler(&mut self, stat: Option<TransferCacheStat>) -> FlowMod {
        match self.stat() {
            TransferCacheStat::Alloc => {
                self.alloc_batch();
            }
            TransferCacheStat::Dealloc => {
                self.dealloc_batch();
            }
            TransferCacheStat::Empty => {
                self.refill_span();
            }
            TransferCacheStat::Finish => {
                self.taken();
            }
            TransferCacheStat::Lack => {
                self.refill_free_list();
            }
            TransferCacheStat::Oversized => {
                self.scavenge_span();
            }
            TransferCacheStat::Ready => {
                self.seed(stat);
            }
            TransferCacheStat::Scavenge => {
                self.scavenged();
            }
        }
        match self.stat() {
            TransferCacheStat::Finish => FlowMod::Forward,
            TransferCacheStat::Alloc
            | TransferCacheStat::Dealloc
            | TransferCacheStat::Oversized => FlowMod::Circle,
            TransferCacheStat::Empty | TransferCacheStat::Lack | TransferCacheStat::Scavenge => {
                FlowMod::Backward
            }
            TransferCacheStat::Ready => FlowMod::Exit,
        }
    }

    fn seed(&mut self, stat: Option<TransferCacheStat>) {
        if let Some(stat) = stat {
            self.set_stat(stat);
        }
    }

    /// Allocate a batch of objects from the current `TransferCache`.
    fn alloc_batch(&mut self) {
        let mut transfer_batch = TransferBatch::new(get_num_to_move(self.size_class_idx).unwrap());
        loop {
            if let Some(hot) = self.hot() {
                match self.pop(hot) {
                    None => break,
                    Some(ptr) => {
                        if transfer_batch.push(ptr) {
                            break;
                        }
                    }
                }
            } else {
                break;
            }
        }
        if transfer_batch.is_empty() {
            if self.free_lists.lack() {
                self.set_stat(TransferCacheStat::Lack);
            } else {
                self.set_stat(TransferCacheStat::Empty);
            }
        } else {
            self.transfer_batch = Some(transfer_batch);
            self.set_stat(TransferCacheStat::Finish);
        }
    }

    /// Deallocate a batch of objects from `CpuCache` to the current `TransferCache`.
    fn dealloc_batch(&mut self) {
        if self.transfer_batch.is_none() {
            return;
        }
        let mut transfer_batch = self.transfer_batch.take().unwrap();
        while let Some(ptr) = transfer_batch.pop() {
            self.push(ptr);
        }
        if self.oversized() {
            self.set_stat(TransferCacheStat::Oversized);
        } else {
            self.set_stat(TransferCacheStat::Ready);
        }
    }

    /// Scavenge a span (full `BoundedList` in this scope) to `CentralFreeList`
    fn scavenge_span(&mut self) {
        let cold = self.cold().unwrap();
        let len = get_pages(self.size_class_idx).unwrap();
        let start = self.clear(cold);
        self.transfer_span = Some(Span::new(len, start));
    }

    fn scavenged(&mut self) {
        if self.transfer_span.is_none() {
            if self.oversized() {
                self.set_stat(TransferCacheStat::Oversized);
            } else {
                self.set_stat(TransferCacheStat::Ready);
            }
        }
    }

    /// Split a span into objects to refill the current `TransferCache`.
    fn refill_span(&mut self) {
        if self.transfer_span.is_none() {
            return;
        }
        let unused = self.unused().unwrap();
        let transfer_span = self.transfer_span.take().unwrap();
        self.fill(unused, transfer_span);
        self.set_stat(TransferCacheStat::Alloc);
    }

    fn refill_free_list(&mut self) {
        if self.transfer_list.is_none() {
            return;
        }
        let transfer_list = self.transfer_list.take().unwrap();
        // TODO: SAFETY
        unsafe { (*transfer_list).reset() };
        self.free_lists.push(transfer_list);
        self.set_stat(TransferCacheStat::Empty);
    }

    fn taken(&mut self) {
        if self.transfer_batch.is_none() {
            self.set_stat(TransferCacheStat::Ready);
        }
    }

    /// Return the idex of `BoundedList` which covering the given `ptr`.
    pub fn match_span(&self, ptr: *mut usize) -> Option<usize> {
        let addr = ptr as usize;
        let free_lists = &self.free_lists;
        for (idx, free_list) in free_lists.iter().enumerate() {
            if free_list.infra_metas(addr) {
                return Some(idx);
            }
        }
        None
    }

    fn hot(&self) -> Option<usize> {
        let mut hot = core::usize::MAX;
        let mut color = 0usize;
        for (idx, free_list) in self.free_lists.iter().enumerate() {
            if !free_list.is_empty()
            /*&& free_list.color() >= color*/
            {
                hot = idx;
                color = free_list.color();
            }
        }
        if hot < core::usize::MAX {
            Some(hot)
        } else {
            None
        }
    }

    /// Return the index of the first unused empty `BoundedList`.
    fn unused(&self) -> Option<usize> {
        let free_lists = &self.free_lists;
        for (idx, free_list) in free_lists.iter().enumerate() {
            if free_list.unused() {
                return Some(idx);
            }
        }
        None
    }

    /// Return the index of the full `BoundedList` with smallest color.
    fn cold(&self) -> Option<usize> {
        let free_lists = &self.free_lists;
        let mut min_idx = 0usize;
        let mut min_color = core::usize::MAX;
        for (idx, free_list) in free_lists.iter().enumerate() {
            if free_list.is_full() && free_list.color() < min_color {
                min_idx = idx;
                min_color = free_list.color();
            }
        }
        match min_color < core::usize::MAX {
            false => None,
            true => Some(min_idx),
        }
    }

    fn push(&mut self, ptr: *mut usize) {
        let idx = self.match_span(ptr).unwrap();
        if self
            .free_lists
            .iter_mut()
            .enumerate()
            .find(|(index, _)| *index == idx)
            .map(|(_, item)| item)
            .unwrap()
            .push(ptr)
        {
            self.full_num += 1;
        }
    }

    fn pop(&mut self, idx: usize) -> Option<*mut usize> {
        if let Some((ptr, flag)) = self
            .free_lists
            .iter_mut()
            .enumerate()
            .find(|(index, _)| *index == idx)
            .map(|(_, item)| item)
            .unwrap()
            .pop()
        {
            if flag {
                self.full_num -= 1;
            }
            Some(ptr)
        } else {
            None
        }
    }

    fn fill(&mut self, idx: usize, span: Span) {
        let free_list = self
            .free_lists
            .iter_mut()
            .enumerate()
            .find(|(index, _)| *index == idx)
            .map(|(_, item)| item)
            .unwrap();
        free_list.set_max_len(core::usize::MAX);
        let size = get_size(self.size_class_idx).unwrap();
        let start = span.start();
        let end = start + span.len() * K_PAGE_SIZE;
        for addr in (start..=end - size).rev() {
            free_list.push(addr as *mut usize);
        }
        free_list.init(start, end);
        self.num += 1;
        self.full_num += 1;
    }

    fn clear(&mut self, idx: usize) -> usize {
        let free_list = self
            .free_lists
            .iter_mut()
            .enumerate()
            .find(|(index, _)| *index == idx)
            .map(|(_, item)| item)
            .unwrap();
        while let Some(_) = free_list.pop() {}
        let base = free_list.base();
        free_list.reset();
        base
    }

    fn oversized(&self) -> bool {
        self.full_num > self.num / K_FULL_SCALE + 1
    }

    pub fn stat(&self) -> TransferCacheStat {
        self.stat.clone()
    }

    pub fn set_stat(&mut self, stat: TransferCacheStat) {
        self.stat = stat;
    }
}

pub struct TransferCaches {
    transfer_caches: [TransferCache; K_BASE_NUMBER_CLASSES],
}

impl TransferCaches {
    pub const fn new() -> Self {
        const ARRAY_REPEAT_VALUE: TransferCache = TransferCache::new();
        Self {
            transfer_caches: [ARRAY_REPEAT_VALUE; K_BASE_NUMBER_CLASSES],
        }
    }

    pub fn init(&mut self) {
        let _ = self
            .transfer_caches
            .iter_mut()
            .enumerate()
            .map(|(idx, transfer_cache)| transfer_cache.init(idx));
    }

    pub fn get_current_transfer_cache(&mut self, size_class_idx: usize) -> &mut TransferCache {
        assert!(size_class_idx < K_BASE_NUMBER_CLASSES);
        &mut self.transfer_caches[size_class_idx]
    }
}
