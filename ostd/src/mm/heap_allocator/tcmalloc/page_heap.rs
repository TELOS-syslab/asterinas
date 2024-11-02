// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_PAGE_SHIFT, K_PAGE_SIZE, K_PRIMARY_HEAP_LEN},
    size_class::Span,
    status::{FlowMod, PageHeapStat},
};

pub struct PageHeap {
    primary_heap: [(bool, usize); K_PRIMARY_HEAP_LEN],
    stat: PageHeapStat,
    reg: PageHeapReg,
    transfer_span: Option<Span>,
}

impl PageHeap {
    pub const fn new() -> Self {
        Self {
            primary_heap: [(false, 0); K_PRIMARY_HEAP_LEN],
            stat: PageHeapStat::Ready,
            reg: PageHeapReg::new(),
            transfer_span: None,
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

    pub fn set_pages(&mut self, pages: usize) {
        self.reg.pages = Some(pages);
    }

    pub fn put_span(&mut self, transfer_span: Option<Span>) {
        self.transfer_span = Some(transfer_span.unwrap());
    }

    pub fn take_span(&mut self) -> Option<Span> {
        self.transfer_span.take()
    }

    pub fn stat_handler(&mut self, stat: Option<PageHeapStat>) -> FlowMod {
        match self.stat() {
            PageHeapStat::Alloc => {
                self.alloc_pages();
            }
            PageHeapStat::Dealloc => {
                self.dealloc_pages();
            }
            PageHeapStat::Insufficient => {
                self.refill_pages();
            }
            PageHeapStat::Ready => {
                self.seed(stat);
            }
            PageHeapStat::Uncovered => {
                self.scavenged();
            }
        }
        match self.stat() {
            PageHeapStat::Ready => FlowMod::Forward,
            PageHeapStat::Alloc | PageHeapStat::Dealloc => FlowMod::Circle,
            PageHeapStat::Insufficient | PageHeapStat::Uncovered => FlowMod::Backward,
        }
    }

    fn seed(&mut self, stat: Option<PageHeapStat>) {
        if let Some(stat) = stat {
            self.set_stat(stat);
        }
    }

    fn try_to_match_span(&mut self, pages: usize) -> Option<usize> {
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
    fn alloc_pages(&mut self) {
        let pages = self.reg.pages.unwrap();
        if let Some(start) = self.try_to_match_span(pages) {
            self.transfer_span = Some(Span::new(pages, start));
            self.set_stat(PageHeapStat::Ready);
        } else {
            self.set_stat(PageHeapStat::Insufficient);
        }
    }

    /// Try to deallocate span with given `pages` to `PrimaryHeap`.
    fn dealloc_pages(&mut self) {
        let addr = self.reg.ptr.unwrap() as usize;
        let pages = self.reg.pages.unwrap();
        let primary_heap = &mut self.primary_heap;
        let base = primary_heap.first().unwrap().1;
        let bound = primary_heap.last().unwrap().1 + K_PAGE_SIZE;
        let span_base = addr;
        let span_bound = addr + (pages << K_PAGE_SHIFT);

        if span_base >= base && span_bound <= bound {
            let start = (addr - base) >> K_PAGE_SHIFT;
            for page in primary_heap[start..start + pages].iter_mut() {
                let assigned = &mut page.0;
                *assigned = false;
            }
            self.set_stat(PageHeapStat::Ready);
        } else {
            self.transfer_span = Some(Span::new(pages, addr));
            self.set_stat(PageHeapStat::Uncovered);
        }
    }

    fn refill_pages(&mut self) {
        if self.transfer_span.is_none() {
            return;
        }
        self.set_stat(PageHeapStat::Ready);
    }

    fn scavenged(&mut self) {
        if self.transfer_span.is_none() {
            self.set_stat(PageHeapStat::Ready);
        }
    }

    pub fn stat(&self) -> PageHeapStat {
        self.stat.clone()
    }

    pub fn set_stat(&mut self, stat: PageHeapStat) {
        self.stat = stat;
    }
}

struct PageHeapReg {
    ptr: Option<*mut usize>,
    pages: Option<usize>,
}

impl PageHeapReg {
    const fn new() -> Self {
        Self {
            ptr: None,
            pages: None,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}
