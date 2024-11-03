// SPDX-License-Identifier: MPL-2.0

use central_free_list::CentralFreeLists;
use common::{K_BASE_NUMBER_SPAN, K_PAGE_SHIFT, K_PAGE_SIZE};
use cpu_cache::CpuCaches;
use page_heap::PageHeap;
use size_class::{get_pages, match_size_class, Span};
use status::*;
use transfer_cache::TransferCaches;

use crate::early_println;

mod central_free_list;
pub mod common;
mod cpu_cache;
mod linked_list;
mod page_heap;
mod size_class;
pub mod status;
mod transfer_cache;

pub struct Tcmalloc<const C: usize> {
    cpu_caches: CpuCaches<C>,
    transfer_caches: TransferCaches,
    central_free_lists: CentralFreeLists,
    page_heap: PageHeap,
    stat: [MetaStat; C],
    reg: [MetaReg; C],
    transfer_span: [Option<Span>; C],
    transfer_object: [Option<*mut u8>; C],
}

impl<const C: usize> Tcmalloc<C> {
    pub const fn new() -> Self {
        const STAT_REPEAT_VALUE: MetaStat = MetaStat::Ready;
        const REG_REPEAT_VALUE: MetaReg = MetaReg::new();
        const SPAN_REPEAT_VALUE: Option<Span> = None;
        Self {
            cpu_caches: CpuCaches::new(),
            transfer_caches: TransferCaches::new(),
            central_free_lists: CentralFreeLists::new(),
            page_heap: PageHeap::new(),
            stat: [STAT_REPEAT_VALUE; C],
            reg: [REG_REPEAT_VALUE; C],
            transfer_span: [SPAN_REPEAT_VALUE; C],
            transfer_object: [None; C],
        }
    }

    // FIXME: To be called only once.
    pub fn init(&mut self, max_len: usize, base: usize) {
        self.cpu_caches.init();
        self.transfer_caches.init();
        self.central_free_lists.init(max_len);
        self.page_heap.init(base);
    }

    fn put_object(&mut self, cpu: usize, transfer_object: Option<*mut u8>) {
        self.transfer_object[cpu] = Some(transfer_object.unwrap());
    }

    pub fn take_object(&mut self, cpu: usize) -> Option<*mut u8> {
        self.transfer_object[cpu].take()
    }

    pub fn put_span(&mut self, cpu: usize, transfer_span: Option<Span>) {
        self.transfer_span[cpu] = Some(transfer_span.unwrap());
    }

    pub fn take_span(&mut self, cpu: usize) -> Option<Span> {
        self.transfer_span[cpu].take()
    }

    pub fn stat_handler(&mut self, cpu: usize, seed: Option<(MetaStat, MetaReg)>) -> FlowMod {
        match self.stat(cpu) {
            MetaStat::Alloc => {
                self.allocate(cpu);
            }
            MetaStat::Dealloc => {
                self.deallocate(cpu);
            }
            MetaStat::Insufficient => {
                self.refill_pages(cpu);
            }
            MetaStat::LargeSize => {
                self.alloc_large_object(cpu);
            }
            MetaStat::Ready => {
                self.seed(cpu, seed);
            }
            MetaStat::Uncovered => {
                self.scavenged(cpu);
            }
        }
        match self.stat(cpu) {
            MetaStat::Ready => {
                self.reg[cpu].reset();
                FlowMod::Forward
            }
            MetaStat::Alloc | MetaStat::Dealloc => FlowMod::Circle,
            MetaStat::Insufficient | MetaStat::LargeSize | MetaStat::Uncovered => FlowMod::Backward,
        }
    }

    fn seed(&mut self, cpu: usize, seed: Option<(MetaStat, MetaReg)>) {
        if let Some((stat, reg)) = seed {
            self.set_stat(cpu, stat);
            self.set_reg(cpu, reg);
        }
    }

    /// Try to allocate a sized object meeting the need of given `layout`.
    fn allocate(&mut self, cpu: usize) {
        let layout = self.reg[cpu].layout().unwrap();
        match match_size_class(layout) {
            None => {
                let size = layout.size();
                let pages = (size + K_PAGE_SIZE - 1) >> K_PAGE_SHIFT;
                if pages > K_BASE_NUMBER_SPAN {
                    self.reg[cpu].set_pages(pages);
                    self.set_stat(cpu, MetaStat::LargeSize);
                } else {
                    let mut _central_free_list_seed = Some(CentralFreeListStat::Alloc);
                    let mut _page_heap_seed = None;
                    loop {
                        let central_free_list = self
                            .central_free_lists
                            .get_current_central_free_list(pages - 1);
                        match central_free_list.stat_handler(_central_free_list_seed.clone()) {
                            FlowMod::Backward => {
                                _page_heap_seed = Some((
                                    PageHeapStat::Alloc,
                                    PageHeapReg::from(None, Some(pages)),
                                ));
                            }
                            FlowMod::Circle => continue,
                            FlowMod::Forward => {
                                let object =
                                    central_free_list.take_span().unwrap().start() as *mut u8;
                                self.put_object(cpu, Some(object));
                                break;
                            }
                        }

                        if _page_heap_seed.is_some() {
                            let page_heap = &mut self.page_heap;
                            match page_heap.stat_handler(_page_heap_seed.clone()) {
                                FlowMod::Backward => {
                                    self.reg[cpu].set_pages(page_heap.reg().pages().unwrap());
                                    break;
                                }
                                FlowMod::Circle => continue,
                                FlowMod::Forward => {
                                    self.central_free_lists
                                        .get_current_central_free_list(pages - 1)
                                        .put_span(page_heap.take_span());
                                    _page_heap_seed = None;
                                    continue;
                                }
                            }
                        }
                    }
                    if self.transfer_object[cpu].is_some() {
                        self.set_stat(cpu, MetaStat::Ready);
                    } else {
                        self.set_stat(cpu, MetaStat::Insufficient);
                    }
                }
            }
            Some(idx) => {
                let mut _cpu_cache_seed = Some((
                    CpuCacheStat::Alloc,
                    CpuCacheReg::from(Some(idx), Some(layout.align()), None),
                ));
                let mut _transfer_cache_seed = None;
                let mut _central_free_list_seed = None;
                let mut _central_free_list_meta_seed = None;
                let mut _page_heap_seed = None;
                loop {
                    let cpu_cache = self.cpu_caches.get_current_cpu_cache(cpu);
                    early_println!("[tcmalloc] cpu_cache_stat = {:#?}", cpu_cache.stat());
                    match cpu_cache.stat_handler(_cpu_cache_seed.clone()) {
                        FlowMod::Backward => _transfer_cache_seed = Some(TransferCacheStat::Alloc),
                        FlowMod::Circle => continue,
                        FlowMod::Forward => {
                            let object = cpu_cache.take_object().unwrap();
                            self.put_object(cpu, Some(object));
                            break;
                        }
                    }

                    if _transfer_cache_seed.is_some() {
                        let transfer_cache = self.transfer_caches.get_current_transfer_cache(idx);
                        early_println!(
                            "[tcmalloc] transfer_cache_stat = {:#?}",
                            transfer_cache.stat()
                        );
                        match transfer_cache.stat_handler(_transfer_cache_seed.clone()) {
                            FlowMod::Backward => match transfer_cache.stat() {
                                TransferCacheStat::Empty => {
                                    _central_free_list_seed = Some(CentralFreeListStat::Alloc)
                                }
                                TransferCacheStat::Lack => {
                                    _central_free_list_meta_seed =
                                        Some(CentralFreeListMetaStat::Alloc)
                                }
                                _ => panic!(),
                            },
                            FlowMod::Circle => continue,
                            FlowMod::Forward => {
                                self.cpu_caches
                                    .get_current_cpu_cache(cpu)
                                    .put_batch(transfer_cache.take_batch());
                                _transfer_cache_seed = None;
                                continue;
                            }
                        }
                    }

                    if _central_free_list_meta_seed.is_some() {
                        let central_free_lists = &mut self.central_free_lists;
                        early_println!(
                            "[tcmalloc] central_free_list_meta_stat = {:#?}",
                            central_free_lists.stat()
                        );
                        match central_free_lists.stat_handler(_central_free_list_meta_seed.clone())
                        {
                            FlowMod::Backward => {
                                _page_heap_seed =
                                    Some((PageHeapStat::Alloc, PageHeapReg::from(None, Some(1))));
                            }
                            FlowMod::Circle => continue,
                            FlowMod::Forward => {
                                self.transfer_caches
                                    .get_current_transfer_cache(idx)
                                    .put_list(central_free_lists.take_list());
                                _central_free_list_meta_seed = None;
                                continue;
                            }
                        }
                    }

                    let pages = get_pages(idx).unwrap();
                    if _central_free_list_seed.is_some() {
                        let central_free_list = self
                            .central_free_lists
                            .get_current_central_free_list(pages - 1);
                        early_println!(
                            "[tcmalloc] central_free_list__stat = {:#?}",
                            central_free_list.stat()
                        );
                        match central_free_list.stat_handler(_central_free_list_seed.clone()) {
                            FlowMod::Backward => {
                                _page_heap_seed = Some((
                                    PageHeapStat::Alloc,
                                    PageHeapReg::from(None, Some(pages)),
                                ))
                            }
                            FlowMod::Circle => continue,
                            FlowMod::Forward => {
                                self.transfer_caches
                                    .get_current_transfer_cache(idx)
                                    .put_span(central_free_list.take_span());
                                _central_free_list_seed = None;
                                continue;
                            }
                        }
                    }

                    if _page_heap_seed.is_some() {
                        let page_heap = &mut self.page_heap;
                        early_println!("[tcmalloc] page_heap_stat = {:#?}", page_heap.stat());
                        match page_heap.stat_handler(_page_heap_seed.clone()) {
                            FlowMod::Backward => {
                                self.reg[cpu].set_pages(page_heap.reg().pages().unwrap());
                                break;
                            }
                            FlowMod::Circle => continue,
                            FlowMod::Forward => {
                                if _central_free_list_meta_seed.is_some() {
                                    self.central_free_lists.put_span(page_heap.take_span());
                                } else if _central_free_list_seed.is_some() {
                                    self.central_free_lists
                                        .get_current_central_free_list(pages - 1)
                                        .put_span(page_heap.take_span());
                                }
                                _page_heap_seed = None;
                                continue;
                            }
                        }
                    }
                }
                if self.transfer_object[cpu].is_some() {
                    self.set_stat(cpu, MetaStat::Ready);
                } else {
                    self.set_stat(cpu, MetaStat::Insufficient);
                }
            }
        }
    }

    fn alloc_large_object(&mut self, cpu: usize) {
        if self.transfer_span[cpu].is_none() {
            return;
        }
        let ptr = self.transfer_span[cpu].take().unwrap().start() as *mut u8;
        self.transfer_object[cpu] = Some(ptr);
        self.set_stat(cpu, MetaStat::Ready);
    }

    /// Try to allocate a sized object meeting the need of given `layout`.
    fn deallocate(&mut self, cpu: usize) {
        todo!();
    }

    fn scavenged(&mut self, cpu: usize) {
        if self.transfer_span[cpu].is_none() {
            self.set_stat(cpu, MetaStat::Ready);
        }
    }

    fn refill_pages(&mut self, cpu: usize) {
        if self.transfer_span[cpu].is_none() {
            return;
        }
        let page_heap = &mut self.page_heap;
        page_heap.put_span(self.transfer_span[cpu].take());
        self.set_stat(cpu, MetaStat::Alloc);
    }

    pub fn stat(&self, cpu: usize) -> MetaStat {
        self.stat[cpu].clone()
    }

    pub fn set_stat(&mut self, cpu: usize, stat: MetaStat) {
        self.stat[cpu] = stat;
    }

    fn set_reg(&mut self, cpu: usize, reg: MetaReg) {
        self.reg[cpu] = reg;
    }
}
