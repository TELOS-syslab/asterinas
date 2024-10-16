// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_BASE_NUMBER_SPAN, K_MAX_OVERRANGES},
    error_handler::CentralFreeListErr,
    linked_list::ElasticList,
};

pub struct CentralFreeLists {
    free_lists: [ElasticList; K_BASE_NUMBER_SPAN],
    pages: usize,
    max_pages: usize,
}

impl CentralFreeLists {
    pub const fn new() -> Self {
        const ARRAY_REPEAT_VALUE: ElasticList = ElasticList::new();

        Self {
            free_lists: [ARRAY_REPEAT_VALUE; K_BASE_NUMBER_SPAN],
            pages: 0,
            max_pages: 0,
        }
    }

    pub fn init(&mut self, max_pages: usize) {
        self.max_pages = max_pages;
        let list_max_len = max_pages / K_BASE_NUMBER_SPAN;

        assert_eq!(list_max_len > 0, true);

        let free_lists = &mut self.free_lists;

        for free_list in free_lists.iter_mut() {
            free_list.init(list_max_len, K_MAX_OVERRANGES);
        }
    }

    /// Allocate a span to `TransferCache`.
    /// 
    /// Return `Ok(ptr)` where `ptr` points to the allocated span.
    /// 
    /// Return `Err(CentralFreeListErr::Empty)` if the `ElasticList` of given `span` is empty.
    pub fn alloc_span(&mut self, pages: usize) -> Result<*mut usize, CentralFreeListErr> {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[pages - 1];

        match free_list.pop() {
            None => Err(CentralFreeListErr::Empty),
            Some(ptr) => {
                self.pages -= pages;
                Ok(ptr)
            }
        }
    }

    /// Allocate a span sized object from the current `CentralFreeList` directly.
    /// 
    /// Return `Ok(ptr)` where `ptr` points to the allocated span.
    /// 
    /// Return `Err(CentralFreeListErr::Empty)` if the `ElasticList` of given `span` is empty.
    pub fn alloc_span_object(&mut self, pages: usize) -> Result<*mut usize, CentralFreeListErr> {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[pages - 1];

        match free_list.pop() {
            None => Err(CentralFreeListErr::Empty),
            Some(ptr) => {
                self.pages -= pages;
                Ok(ptr)
            }
        }
    }

    /// Scavenge a free span to the `PageHeap` because of `CentralFreeListErr::Overranged` of
    /// `CentralFreeListErr::Oversized`.
    /// 
    /// Return `Ok(ptr)` where `ptr` points to the allocated span.
    /// 
    /// Return `Err(CentralFreeListErr::Empty)` if the `ElasticList` of given `span` is empty.
    pub fn scavenge_span(&mut self, pages: usize) -> Result<*mut usize, CentralFreeListErr> {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[pages - 1];

        match free_list.pop() {
            None => {
                free_list.reset();
                Err(CentralFreeListErr::Empty)
            }
            Some(ptr) => {
                self.pages -= pages;
                Ok(ptr)
            }
        }
    }

    /// Deallocate a span to the current `CentralFreeList` checking `CentralFreeList` after scavenging
    /// finished outside.
    pub fn dealloc_span_with_lazy_check(&mut self, pages: usize, ptr: *mut usize) {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[pages - 1];

        free_list.push(ptr);
        self.pages += pages;
    }

    /// Deallocate a span sized object to the current `CentralFreeList` directly.
    /// 
    /// Return `Ok(())` if no need to shrink the `CentralFreeList`.
    /// 
    /// Return `Err(CentralFreeListErr::Oversized)` if the free pages of `CentralFreeLists` overranged its `max_len`.
    /// Its spare free pages should be reclaimed.
    /// 
    /// Return `Err(CentralFreeListErr::Overranged)` if the `ElasticList` of give `span` overranged its `max_len`
    /// more than `max_overrang` times. The `ElasticList` should be shrinked.
    pub fn dealloc_span_object(&mut self, pages: usize, ptr: *mut usize) -> Result<(), CentralFreeListErr> {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[pages - 1];

        free_list.push(ptr);
        self.pages += pages;

        if free_list.overranged() {
            return Err(CentralFreeListErr::Overranged);
        }

        if self.oversized() {
            return Err(CentralFreeListErr::Oversized);
        }

        Ok(())
    }

    pub fn refill_span_without_check(&mut self, pages: usize, ptr: *mut usize) {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[pages - 1];

        free_list.push(ptr);
        self.pages += pages;
    }

    /// Return the index of the `ElasticList` with the smallest `color`.
    pub fn find_smallest_color(&self) -> Option<usize> {
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

    pub fn overranged(&self, pages: usize) -> bool {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        let free_list = &self.free_lists[pages - 1];

        free_list.overranged()
    }

    pub fn oversized(&self) -> bool {
        self.pages > self.max_pages
    }
}
