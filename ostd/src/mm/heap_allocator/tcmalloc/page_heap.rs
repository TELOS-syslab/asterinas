// SPDX-License-Identifier: MPL-2.0

use super::common::{K_PAGE_SHIFT, K_PAGE_SIZE, K_PRIMARY_HEAP_LEN};

pub struct PageHeap {
    primary_heap: [(bool, usize); K_PRIMARY_HEAP_LEN],
}

impl PageHeap {
    pub const fn new() -> Self {
        Self {
            primary_heap: [(false, 0); K_PRIMARY_HEAP_LEN],
        }
    }

    pub fn init(&mut self, base: usize) {
        let primary_heap = &mut self.primary_heap;
        let mut offset = 0usize;
        
        for (assigned, page_addr) in primary_heap.iter_mut() {
            *assigned = false;
            *page_addr = base + offset;
            offset += K_PAGE_SIZE;
        }
    }

    pub fn try_to_match_span(&mut self, pages: usize) -> Option<usize> {
        let primary_heap = &mut self.primary_heap;
        let mut start = 0usize;
        let mut count = 0usize;
        for (index, page) in primary_heap.iter_mut().enumerate() {
            if count == 0 {
                start = index;
            }
            let assigned = page.0;
            
            match assigned {
                false => count += 1,
                true => count = 0,
            }

            if count == pages {
                break;
            }
        }

        match count == pages {
            false => None,
            true => {
                for page in primary_heap[start..start + pages].iter_mut() {
                    let assigned = &mut page.0;
                    *assigned = true;
                }

                let start_addr = primary_heap[start].1;
                Some(start_addr)
            }
        }
    }

    /// Try to allocate span with given `pages` from `PrimaryHeap`.
    /// 
    /// Return `Ok(addr)` if the `PrimaryHeap` meets the need.
    /// 
    /// Return `Err(())` to indicate to allocate from `OS`.
    pub fn alloc_pages(&mut self, pages: usize) -> Result<usize, ()> {
        match self.try_to_match_span(pages) {
            None => Err(()),
            Some(addr) => Ok(addr),
        }
    }

    /// Try to deallocate span with given `pages` to `PrimaryHeap`.
    /// 
    /// Return `Ok(())` if the `PrimaryHeap` meets the need.
    /// 
    /// Return `Err(())` to indicate to deallocate to `OS`.
    pub fn dealloc_pages(&mut self, addr: usize, pages: usize) -> Result<(), ()> {
        let primary_heap = &mut self.primary_heap;
        let base = primary_heap.first().unwrap().1;
        let bound = primary_heap.last().unwrap().1 + K_PAGE_SIZE;
        let span_base = addr;
        let span_bound = addr + (pages << K_PAGE_SHIFT);

        match span_base >= base && span_bound <= bound {
            false => Err(()),
            true => {
                let start = (addr - base) >> K_PAGE_SHIFT;

                for page in primary_heap[start..start + pages].iter_mut() {
                    let assigned = &mut page.0;
                    *assigned = false;
                }

                Ok(())
            },
        }
    }
}
