// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_BASE_NUMBER_CLASSES, K_FULL_SCALE, K_MAX_NUMBER_SPAN},
    error_handler::TransferCacheErr,
    linked_list::LinkedList,
};

#[derive(Clone, Copy)]
pub struct TransferCache {
    free_lists: [FreeList; K_MAX_NUMBER_SPAN],
    num: usize,
    full_num: usize,
}

impl TransferCache {
    const fn new() -> Self {
        Self {
            free_lists: [FreeList::new(); K_MAX_NUMBER_SPAN],
            num: 0,
            full_num: 0,
        }
    }

    pub fn init_with_index(&mut self, idx: usize, base: usize, bound: usize) {
        assert_eq!(idx < K_MAX_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[idx];

        free_list.init(base, bound);
    }

    /// Return `Ok(ptr)` where `ptr` points to the allocated object.
    /// 
    /// Return `Err(TransferCacheErr::Empty)` if all `FreeLists` in the `TransferCache` are empty.
    pub fn alloc_object(&mut self) -> Result<*mut usize, TransferCacheErr> {
        let free_lists = &mut self.free_lists;
        let mut rslt = core::ptr::null_mut();
        
        for free_list in free_lists.iter_mut() {
            let full = free_list.is_full();
            match free_list.pop() {
                None => {},
                Some(ptr) => {
                    rslt = ptr;
                    if full == true {
                        self.full_num -= 1;
                    }
                    break;
                }
            }
        }

        match rslt.is_null() {
            false => Ok(rslt),
            true => Err(TransferCacheErr::Empty),
        }
    }

    /// Return `Err(TransferCacheErr::Empty)` if all `FreeLists` in the `TransferCache` are empty.
    pub fn alloc_object_with_index(&mut self, idx: usize) -> Result<(), TransferCacheErr> {
        assert_eq!(idx < K_MAX_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[idx];
        
        match free_list.pop() {
            None => {
                free_list.set_max_len(0);
                free_list.color = 0;
                free_list.base = 0;
                free_list.bound = 0;
                Err(TransferCacheErr::Empty)
            },
            Some(_) => Ok(()),
        }
    }

    /// Return `Ok(())` if no need to shrink the `TransferCache`.
    /// 
    /// Return `Err(TransferCacheErr::Full)` if the number of full `FreeLists` exceeded the limitation.
    /// The `TransferCache` should be shrinked.
    pub fn dealloc_object(&mut self, ptr: *mut usize) -> Result<(), TransferCacheErr> {
        let free_lists = &mut self.free_lists;
        let addr = ptr as usize;

        for free_list in free_lists.iter_mut() {
            if addr >= free_list.base && addr < free_list.bound {
                free_list.push(ptr);
                if free_list.is_full() {
                    self.full_num += 1;
                    if self.full_num >= self.num / K_FULL_SCALE {
                        return Err(TransferCacheErr::Full);
                    }
                }

                return Ok(());
            }
        }

        unreachable!("[tcmalloc] deallocated to unexpected TransferCache!");
    }

    /// Split span.
    pub fn dealloc_span(&mut self, base: usize, bound: usize, size: usize) -> Result<(), ()> {
        assert_eq!(base < bound, true);

        let idx = self.find_empty().unwrap();
        let len = (bound - base - 1) / size;

        let free_list = &mut self.free_lists[idx];
        free_list.init(base, bound);
        free_list.set_max_len(len);

        for addr in (base..bound - size).step_by(size) {
            free_list.push(addr as *mut usize);
        }

        self.full_num += 1;

        Ok(())
    }

    /// Return the index of the first unused empty `FreeList`.
    pub fn find_empty(&self) -> Option<usize> {
        let free_lists = &self.free_lists;

        for (idx, free_list) in free_lists.iter().enumerate() {
            if free_list.is_empty() && free_list.color == 0 {
                return Some(idx);
            }
        }

        None
    }

    pub fn find_full(&self) -> Option<usize> {
        let free_lists = &self.free_lists;

        for (idx, free_list) in free_lists.iter().enumerate() {
            if free_list.is_full() {
                return Some(idx);
            }
        }

        None
    }

    pub fn get_base(&self, idx: usize) -> usize {
        assert_eq!(idx < K_MAX_NUMBER_SPAN, true);

        self.free_lists[idx].base
    }

    pub fn num(&self) -> usize {
        self.num
    }

    pub fn full_num(&self) -> usize {
        self.full_num
    }
}

pub struct TransferCaches {
    transfer_caches: [TransferCache; K_BASE_NUMBER_CLASSES],
}

impl TransferCaches {
    pub const fn new() -> Self {
        Self {
            transfer_caches: [TransferCache::new(); K_BASE_NUMBER_CLASSES],
        }
    }

    pub fn get_current_transfer_cache(&mut self, size_class_idx: usize) -> &mut TransferCache {
        assert_eq!(size_class_idx < K_BASE_NUMBER_CLASSES, true);

        &mut self.transfer_caches[size_class_idx]
    }
}

#[derive(Clone, Copy)]
struct FreeList {
    list: LinkedList,
    len: usize,
    max_len: usize,
    color: usize,
    base: usize,
    bound: usize,
}

impl FreeList {
    const fn new() -> Self {
        Self {
            list: LinkedList::new(),
            len: 0,
            max_len: 0,
            color: 0,
            base: 0,
            bound: 0,
        }
    }

    fn init(&mut self, base: usize, bound: usize) {
        assert_eq!(base >= bound, false);

        self.base = base;
        self.bound = bound;
    }

    fn push(&mut self, item: *mut usize) {
        let list = &mut self.list;
        // TODO: SAFETY
        unsafe { list.push(item) };

        self.len += 1;
        assert_eq!(self.len > self.max_len, false);
    }

    fn pop(&mut self) -> Option<*mut usize> {
        match self.list.pop() {
            None => None,
            Some(ptr) => {
                self.len -= 1;
                self.color += 1;
                Some(ptr)
            }
        }
    }

    fn set_max_len(&mut self, max_len: usize) {
        self.max_len = max_len;
    }

    fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    fn is_full(&self) -> bool {
        self.len >= self.max_len
    }
}
