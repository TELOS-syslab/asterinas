// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_BASE_NUMBER_SPAN, K_MAX_OVERRANGES, K_PAGE_SIZE},
    linked_list::{BoundedList, ElasticList},
    size_class::{Span, TransferBatch},
    status::{CentralFreeListMetaStat, CentralFreeListStat, FlowMod},
};

pub struct CentralFreeList {
    free_list: ElasticList,
    span_idx: usize,
    stat: CentralFreeListStat,
    transfer_batch: Option<TransferBatch>,
    transfer_span: Option<Span>,
}

impl CentralFreeList {
    const fn new() -> Self {
        Self {
            free_list: ElasticList::new(),
            span_idx: 0,
            stat: CentralFreeListStat::Ready,
            transfer_batch: None,
            transfer_span: None,
        }
    }

    fn init(&mut self, span_idx: usize, max_pages: usize) {
        self.span_idx = span_idx;
        self.free_list
            .init(max_pages / (span_idx + 1), K_MAX_OVERRANGES);
    }

    pub fn put_span(&mut self, transfer_span: Option<Span>) {
        self.transfer_span = Some(transfer_span.unwrap());
    }

    pub fn take_span(&mut self) -> Option<Span> {
        self.transfer_span.take()
    }

    pub fn stat_handler(&mut self, stat: Option<CentralFreeListStat>) -> FlowMod {
        match self.stat() {
            CentralFreeListStat::Alloc => {
                self.alloc_span();
            }
            CentralFreeListStat::Dealloc => {
                self.dealloc_span();
            }
            CentralFreeListStat::Empty => {
                self.refill_span();
            }
            CentralFreeListStat::Overranged => {
                self.scavenge_batch();
            }
            CentralFreeListStat::Ready => {
                self.seed(stat);
            }
            CentralFreeListStat::Scavenge => {
                self.scavenged();
            }
        }
        match self.stat() {
            CentralFreeListStat::Ready => FlowMod::Forward,
            CentralFreeListStat::Alloc
            | CentralFreeListStat::Dealloc
            | CentralFreeListStat::Overranged => FlowMod::Circle,
            CentralFreeListStat::Empty | CentralFreeListStat::Scavenge => FlowMod::Backward,
        }
    }

    fn seed(&mut self, stat: Option<CentralFreeListStat>) {
        if let Some(stat) = stat {
            self.set_stat(stat);
        }
    }

    /// Allocate a span to `TransferCache`.
    fn alloc_span(&mut self) {
        if let Some(ptr) = self.free_list.pop() {
            self.transfer_span = Some(Span::new(self.span_idx + 1, ptr as usize));
            self.set_stat(CentralFreeListStat::Ready);
        } else {
            self.set_stat(CentralFreeListStat::Empty);
        }
    }

    /// Deallocate a span to the current `CentralFreeList`.
    pub fn dealloc_span(&mut self) {
        if self.transfer_span.is_none() {
            return;
        }
        let transfer_span = self.transfer_span.take().unwrap();
        self.free_list.push(transfer_span.start() as *mut usize);
        if self.free_list.overranged() {
            self.set_stat(CentralFreeListStat::Overranged);
        } else {
            self.set_stat(CentralFreeListStat::Ready);
        }
    }

    /// Scavenge a batch of free span to the `PageHeap`.
    fn scavenge_batch(&mut self) {
        let mut transfer_cache = TransferBatch::new(self.free_list.len());
        loop {
            if let Some(ptr) = self.free_list.pop() {
                if transfer_cache.push(ptr) {
                    break;
                }
            } else {
                break;
            }
        }
        self.free_list.reset();
        self.transfer_batch = Some(transfer_cache);
        self.set_stat(CentralFreeListStat::Scavenge);
    }

    fn scavenged(&mut self) {
        if self.transfer_batch.is_none() {
            self.set_stat(CentralFreeListStat::Ready);
        }
    }

    fn refill_span(&mut self) {
        if self.transfer_span.is_none() {
            return;
        }
        self.set_stat(CentralFreeListStat::Ready);
    }

    pub fn stat(&self) -> CentralFreeListStat {
        self.stat.clone()
    }

    pub fn set_stat(&mut self, stat: CentralFreeListStat) {
        self.stat = stat;
    }
}

pub struct CentralFreeLists {
    free_lists: [CentralFreeList; K_BASE_NUMBER_SPAN],
    free_bounded_lists: ElasticList,
    stat: CentralFreeListMetaStat,
    transfer_span: Option<Span>,
    transfer_list: Option<*mut BoundedList>,
}

impl CentralFreeLists {
    pub const fn new() -> Self {
        const ARRAY_REPEAT_VALUE: CentralFreeList = CentralFreeList::new();
        Self {
            free_lists: [ARRAY_REPEAT_VALUE; K_BASE_NUMBER_SPAN],
            free_bounded_lists: ElasticList::new(),
            stat: CentralFreeListMetaStat::Ready,
            transfer_span: None,
            transfer_list: None,
        }
    }

    pub fn init(&mut self, max_size: usize) {
        let _ = self
            .free_lists
            .iter_mut()
            .enumerate()
            .map(|(idx, free_list)| free_list.init(idx, max_size));
    }

    pub fn get_current_central_free_list(&mut self, span_idx: usize) -> &mut CentralFreeList {
        assert!(span_idx < K_BASE_NUMBER_SPAN);
        &mut self.free_lists[span_idx]
    }

    pub fn put_span(&mut self, span: Option<Span>) {
        self.transfer_span = Some(span.unwrap());
    }

    pub fn take_list(&mut self) -> Option<*mut BoundedList> {
        self.transfer_list.take()
    }

    pub fn stat_handler(&mut self, stat: Option<CentralFreeListMetaStat>) -> FlowMod {
        match self.stat() {
            CentralFreeListMetaStat::Alloc => {
                self.alloc_list();
            }
            CentralFreeListMetaStat::Empty => {
                self.refill_list();
            }
            CentralFreeListMetaStat::Ready => {
                self.seed(stat);
            }
        }
        match self.stat() {
            CentralFreeListMetaStat::Ready => FlowMod::Forward,
            CentralFreeListMetaStat::Alloc => FlowMod::Circle,
            CentralFreeListMetaStat::Empty => FlowMod::Backward,
        }
    }

    fn seed(&mut self, stat: Option<CentralFreeListMetaStat>) {
        if let Some(stat) = stat {
            self.set_stat(stat);
        }
    }

    fn alloc_list(&mut self) {
        if let Some(ptr) = self.free_bounded_lists.pop() {
            self.transfer_list = Some(ptr as *mut BoundedList);
            self.set_stat(CentralFreeListMetaStat::Ready);
        } else {
            self.set_stat(CentralFreeListMetaStat::Empty);
        }
    }

    fn refill_list(&mut self) {
        if self.transfer_span.is_none() {
            return;
        }
        let transfer_span = self.transfer_span.take().unwrap();
        let size = core::mem::size_of::<BoundedList>();
        let start = transfer_span.start();
        let end = start + transfer_span.len() * K_PAGE_SIZE;
        for addr in (start..=end - size).rev() {
            self.free_bounded_lists.push(addr as *mut usize);
        }
        self.set_stat(CentralFreeListMetaStat::Alloc);
    }

    pub fn stat(&self) -> CentralFreeListMetaStat {
        self.stat.clone()
    }

    pub fn set_stat(&mut self, stat: CentralFreeListMetaStat) {
        self.stat = stat;
    }
}
