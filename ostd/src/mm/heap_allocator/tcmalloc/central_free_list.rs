// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_BASE_NUMBER_SPAN, K_MAX_OVERRANGES},
    error_handler::CentralFreeListErr,
    linked_list::LinkedList
};

pub struct CentralFreeLists {
    free_lists: [FreeList; K_BASE_NUMBER_SPAN],
    len: usize,
    max_len: usize,
}

impl CentralFreeLists {
    pub const fn new() -> Self {
        Self {
            free_lists: [FreeList::new(); K_BASE_NUMBER_SPAN],
            len: 0,
            max_len: 0,
        }
    }

    pub fn init(&mut self, max_len: usize) {
        self.max_len = max_len;
        let list_max_len = max_len / K_BASE_NUMBER_SPAN;
        assert_eq!(list_max_len >= 1, true);

        let free_lists = &mut self.free_lists;

        for free_list in free_lists.iter_mut() {
            free_list.init(list_max_len, K_MAX_OVERRANGES);
        }
    }

    /// Return `Ok(ptr)` where `ptr` points to the allocated span.
    /// 
    /// Return `Err(CentralFreeListErr::Unsupported)` if the number of pages exceeded the maximum span.
    /// This allocation should be done by the page allocator.
    /// 
    /// Return `Err(CentralFreeListErr::Empty)` if the `FreeList` of given `span` is empty.
    pub fn alloc_span(&mut self, pages: usize) -> Result<*mut usize, CentralFreeListErr> {
        assert_eq!(pages > 0, true);

        let free_lists = &mut self.free_lists;

        match pages <= K_BASE_NUMBER_SPAN {
            false => Err(CentralFreeListErr::Unsupported(0, pages)),
            true => {
                let free_list = &mut free_lists[pages - 1];

                match free_list.pop() {
                    None => Err(CentralFreeListErr::Empty),
                    Some(ptr) => {
                        self.len -= 1;
                        Ok(ptr)
                    },
                }
            }
        }
    }

    /// Return `Ok(ptr)` where `ptr` points to the allocated span.
    /// 
    /// Return `Err(CentralFreeListErr::Unsupported)` if the number of pages exceeded the maximum span.
    /// This allocation should be done by the page allocator.
    /// 
    /// Return `Err(CentralFreeListErr::Empty)` if the `FreeList` of given `span` is empty.
    pub fn alloc_span_with_index(&mut self, idx: usize) -> Result<*mut usize, CentralFreeListErr> {
        assert_eq!(idx < K_BASE_NUMBER_SPAN, true);

        let free_list = &mut self.free_lists[idx];

        match free_list.pop() {
            None => Err(CentralFreeListErr::Empty),
            Some(ptr) => {
                self.len -= 1;
                Ok(ptr)
            },
        }
    }

    /// Return `Ok(())` if no need to shrink the `CentralFreeList`.
    /// 
    /// Return `Err(CentralFreeListErr::Unsupported)` if the number of pages exceeded the maximum span.
    /// This deallocation should be done by the page allocator.
    /// 
    /// Return `Err(CentralFreeListErr::Oversized)` if the free pages of `CentralFreeLists` overranged its `max_len`.
    /// Its spare free pages should be reclaimed.
    /// 
    /// Return `Err(CentralFreeListErr::Overranged)` if the `FreeList` of give `span` overranged its `max_len`
    /// more than `max_overrang` times. The `FreeList` should be shrinked.
    pub fn dealloc_span(&mut self, pages: usize, ptr: *mut usize) -> Result<(), CentralFreeListErr> {
        assert_eq!(pages > 0, true);

        let free_lists = &mut self.free_lists;

        match pages <= K_BASE_NUMBER_SPAN {
            false => Err(CentralFreeListErr::Unsupported(ptr as usize, pages)),
            true => {
                let free_list = &mut free_lists[pages - 1];

                match free_list.push(ptr) {
                    false => {
                        self.len += 1;
                        match self.len > self.max_len {
                            false => Ok(()),
                            true => Err(CentralFreeListErr::Oversized),
                        }
                    },
                    true => Err(CentralFreeListErr::Overranged),
                }
            }
        }
    }

    /// Return the index of the `FreeList` with the smallest `color`.
    pub fn find_smallest_color(&self) -> Option<usize> {
        let free_lists = &self.free_lists;
        let mut min_idx = 0usize;
        let mut min_color = core::usize::MAX;

        for (idx, free_list) in free_lists.iter().enumerate() {
            if free_list.len > 0 && free_list.color < min_color {
                min_idx = idx;
                min_color = free_list.color;
            }
        }

        match min_color < core::usize::MAX {
            false => None,
            true => Some(min_idx),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn max_len(&self) -> usize {
        self.max_len
    }
}

#[derive(Clone, Copy)]
struct FreeList {
    list: LinkedList,
    len: usize,
    max_len: usize,
    color: usize,
    overrange: usize,
    max_overrange: usize,
}

impl FreeList {
    const fn new() -> Self {
        Self {
            list: LinkedList::new(),
            len: 0,
            max_len: 0,
            color: 0,
            overrange: 0,
            max_overrange: 0,
        }
    }

    fn init(&mut self, max_len: usize, max_overrange: usize) {
        self.max_len = max_len;
        self.max_overrange = max_overrange;
    }

    fn push(&mut self, item: *mut usize) -> bool {
        let list = &mut self.list;

        // TODO: SAFETY
        unsafe { list.push(item) };

        self.len += 1;
        if self.len > self.max_len {
            self.overrange += 1;
        }
        if self.overrange > self.max_overrange {
            self.overrange = 0;
            true
        }
        else {
            false
        }
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
}