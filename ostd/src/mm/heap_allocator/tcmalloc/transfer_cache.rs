// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_BASE_NUMBER_CLASSES, K_FULL_SCALE, K_MAX_NUMBER_SPAN},
    error_handler::TransferCacheErr,
    linked_list::BoundedList,
};

pub struct TransferCache {
    free_lists: [BoundedList; K_MAX_NUMBER_SPAN],
    num: usize,
    full_num: usize,
}

impl TransferCache {
    const fn new() -> Self {
        const ARRAY_REPEAT_VALUE: BoundedList = BoundedList::new();

        Self {
            free_lists: [ARRAY_REPEAT_VALUE; K_MAX_NUMBER_SPAN],
            num: 0,
            full_num: 0,
        }
    }

    /// Allocate an object from the current `TransferCache` checking the `TransferCacheErr`
    /// after refilling finished outside.
    /// 
    /// Return `Ok(ptr)` where `ptr` points to the allocated object.
    /// 
    /// Return `Err(TransferCacheErr::Empty)` if all `BoundedLists` in the `TransferCache` are empty.
    pub fn alloc_object_with_lazy_check(&mut self) -> Result<*mut usize, TransferCacheErr> {
        let free_lists = &mut self.free_lists;
        let mut rslt = core::ptr::null_mut();
        
        for free_list in free_lists.iter_mut() {
            let full = free_list.is_full();

            if let Some(ptr) = free_list.pop() {
                rslt = ptr;

                if full {
                    self.full_num -= 1;
                }

                break;
            }
        }

        match rslt.is_null() {
            false => Ok(rslt),
            true => Err(TransferCacheErr::Empty),
        }
    }

    /// Scavenge an object of a span (full `BoundedList` in this scope) to `CentralFreeList`
    /// because of `TransferCacheErr::Full`.
    /// 
    /// Return `Err(TransferCacheErr::Empty)` if the `BoundedLists` is empty.
    pub fn scavenge_object_with_index(&mut self, idx: usize) -> Result<(), TransferCacheErr> {
        assert_eq!(idx < K_MAX_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[idx];
        
        match free_list.pop() {
            None => {
                free_list.reset();
                self.num -= 1;
                self.full_num -= 1;
                Err(TransferCacheErr::Empty)
            },
            Some(_) => Ok(()),
        }
    }

    /// Deallocate an object from `CpuCache` to the current `TransferCache` checking
    /// the `TransferCacheErr` after scavenging finished outside.
    pub fn dealloc_object_with_lazy_check(&mut self, ptr: *mut usize, idx: usize) {
        assert_eq!(idx < K_MAX_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[idx];

        free_list.push(ptr);

        if free_list.is_full() {
            self.full_num += 1;
        }
    }

    /// Deallocate an object from `CpuCache` to the current `TransferCache` without checking
    /// the `TransferCacheErr`.
    pub fn dealloc_object_without_check(&mut self, ptr: *mut usize, idx: usize) {
        assert_eq!(idx < K_MAX_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[idx];

        free_list.push(ptr);

        if free_list.is_full() {
            self.full_num += 1;
        }
    }

    /// Split a span into objects to refill the current `TransferCache`.
    pub fn refill_object_without_check(&mut self, base: usize, bound: usize, size: usize) {
        assert_eq!(base < bound, true);

        let idx = self.find_empty().unwrap();
        let max_len = (bound - base - 1) / size;

        let free_list = &mut self.free_lists[idx];

        free_list.init(base, bound);
        free_list.set_max_len(max_len);

        for addr in (base..bound - size).step_by(size) {
            free_list.push(addr as *mut usize);
        }

        self.num += 1;
        self.full_num += 1;
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

    /// Return the index of the first unused empty `BoundedList`.
    pub fn find_empty(&self) -> Option<usize> {
        let free_lists = &self.free_lists;

        for (idx, free_list) in free_lists.iter().enumerate() {
            if free_list.unused() {
                return Some(idx);
            }
        }

        None
    }

    /// Return the index of the full `BoundedList` with smallest color.
    pub fn find_full_with_color(&self) -> Option<usize> {
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

    pub fn no_full(&self) -> bool {
        self.full_num == 0
    }

    pub fn get_base(&self, idx: usize) -> usize {
        assert_eq!(idx < K_MAX_NUMBER_SPAN, true);

        self.free_lists[idx].base()
    }

    pub fn pseudo_full(&self) -> bool {
        self.full_num > self.num / K_FULL_SCALE
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

    pub fn get_current_transfer_cache(&mut self, size_class_idx: usize) -> &mut TransferCache {
        assert_eq!(size_class_idx < K_BASE_NUMBER_CLASSES, true);

        &mut self.transfer_caches[size_class_idx]
    }
}