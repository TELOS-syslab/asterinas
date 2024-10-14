// SPDX-License-Identifier: MPL-2.0

use super::common::K_BASE_NUMBER_SPAN;
use super::common::K_PAGE_SHIFT;
use super::cpu_cache::CpuCache;
use super::central_free_list::CentralFreeLists;
use super::page_heap::PageHeap;
use super::size_class::SizeClassInfo;
use super::size_class::get_size_class_info;
use super::transfer_cache::TransferCaches;

#[derive(Debug)]
pub enum TcmallocErr {
    PageAlloc(usize),
    PageDealloc(usize, usize),
    SpanAlloc(usize),
}

#[derive(Debug)]
pub enum CpuCacheErr {
    Empty,          // FreeList is empty or no aligned object is available
    Overranged,     // FreeList overranged
    Oversized,      // CpuCache oversized
}

impl CpuCacheErr {
    /// Refill free sized objects from `TransferCache`.
    /// 
    /// May return: `Ok(ptr)` points to allocated object, `Err(TransferCacheErr::Empty)`.
    pub fn resolve_empty_err_and_redo(
        cpu_cache: &mut CpuCache,
        transfer_caches: &mut TransferCaches,
        align: usize,
        size_class: (usize, SizeClassInfo)
    ) -> Result<*mut usize, TransferCacheErr> {
        let (size_class_idx, size_class_info) = size_class;
        let num_to_move = size_class_info.num_to_move();
        let transfer_cache = transfer_caches.get_current_transfer_cache(size_class_idx);
    
        let mut count = 0usize;
        while count < num_to_move {
            match transfer_cache.alloc_object_with_lazy_check() {
                Ok(ptr) => {
                    count += 1;
                    cpu_cache.refill_object_without_check(size_class, ptr);
                }
                Err(err) => {
                    match err {
                        TransferCacheErr::Empty => break,
                        _ => panic!("[tcmalloc] returned unexpected error!"),
                    }
                }
            }
        }

        if count > 0 {
            let ptr = cpu_cache
                .alloc_aligned_object(align, size_class)
                .expect("[tcmalloc] failed to refill object!");

            Ok(ptr)
        }
        else {
            Err(TransferCacheErr::Empty)
        }
    }

    /// Release spare free memory to `TransferCache`.
    /// 
    /// May return: `Ok(())`, `Err((idx, TransferCacheErr::Full))`.
    pub fn resolve_oversized_err(
        cpu_cache: &mut CpuCache,
        transfer_caches: &mut TransferCaches
    ) -> Result<(), (usize, TransferCacheErr)> {
        while cpu_cache.oversized() {
            match cpu_cache.find_smallest_color() {
                None => break,
                Some(idx) => {
                    let size_class_info = get_size_class_info(idx).unwrap();
                    let size_class = (idx, size_class_info);
                    let num_to_move = size_class_info.num_to_move();
                    let transfer_cache = transfer_caches.get_current_transfer_cache(idx);
                    let mut rslt: Result<(), (usize, TransferCacheErr)> = Ok(());

                    for _i in 0..num_to_move {
                        match cpu_cache.scavenge_object(size_class) {
                            Ok(ptr) => {
                                let idx = transfer_cache.match_span(ptr).unwrap();
                                
                                transfer_cache.dealloc_object_with_lazy_check(ptr as *mut usize, idx);

                                if transfer_cache.pseudo_full() {
                                    rslt = Err((idx, TransferCacheErr::Full));
                                }
                            }
                            Err(err) => {
                                match err {
                                    CpuCacheErr::Empty => break,
                                    _ => panic!("[tcmalloc] returned unexpected error!"),
                                }
                            }
                        }
                    }

                    if let Err(err) = rslt {
                        return Err(err);
                    }
                }
            }
        }

        Ok(())
    }

    /// Shrink overranged `FreeList` and release free memory to `TransferCache`.
    pub fn resolve_overranged_err(
        cpu_cache: &mut CpuCache,
        transfer_caches: &mut TransferCaches,
        size_class: (usize, SizeClassInfo)
    ) {
        let transfer_cache = transfer_caches.get_current_transfer_cache(size_class.0);

        while let Ok(ptr) = cpu_cache.scavenge_object(size_class) {
            let idx = transfer_cache.match_span(ptr).unwrap();
            
            transfer_cache.dealloc_object_without_check(ptr as *mut usize, idx);
        }
    }
}

#[derive(Debug)]
pub enum TransferCacheErr {
    Empty,
    Full,
}

impl TransferCacheErr {
    /// Fetch span from `CentralFreeList`.
    /// 
    /// May return: `Ok(ptr)` points to allocated object, `Err(CentralFreeListErr::Empty)`.
    pub fn resolve_empty_err_and_redo(
        cpu_cache: &mut CpuCache,
        transfer_caches: &mut TransferCaches,
        central_free_lists: &mut CentralFreeLists,
        align: usize,
        size_class: (usize, SizeClassInfo)
    ) -> Result<*mut usize, CentralFreeListErr> {
        let (idx, size_class_info) = size_class;
        let pages = size_class_info.pages();
        let transfer_cache = transfer_caches.get_current_transfer_cache(idx);
        
        match central_free_lists.alloc_span(pages) {
            Ok(ptr) => {
                let base = ptr as usize;
                let bound = base + (pages << K_PAGE_SHIFT);

                transfer_cache.refill_object_without_check(base, bound, size_class_info.size());

                let ptr = CpuCacheErr::resolve_empty_err_and_redo(cpu_cache, transfer_caches, align, size_class)
                    .expect("[tcmalloc] failed to refill object!");

                Ok(ptr)
            },
            Err(err) => Err(err),
        }
    }

    /// Release span to `CentralFreeList`.
    /// 
    /// May return: `Ok(())`, `Err(CentralFreeListErr::Overranged)`, `Err(CentralFreeListErr::Oversized)`.
    pub fn resolve_full_err_with_index(
        transfer_caches: &mut TransferCaches,
        central_free_lists: &mut CentralFreeLists,
        idx: usize
    ) -> Result<(), CentralFreeListErr> {
        let size_class_info = get_size_class_info(idx).unwrap();
        let pages = size_class_info.pages();
        let transfer_cache = transfer_caches.get_current_transfer_cache(idx);

        while !transfer_cache.no_full() {
            if let Some(idx) = transfer_cache.find_full_with_color() {
                let base = transfer_cache.get_base(idx);

                while transfer_cache.scavenge_object_with_index(idx).is_ok() {}

                central_free_lists.dealloc_span_with_lazy_check(pages, base as *mut usize);
            }
        }

        if central_free_lists.overranged(pages) {
            return Err(CentralFreeListErr::Overranged);
        }

        if central_free_lists.oversized() {
            return Err(CentralFreeListErr::Oversized);
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum CentralFreeListErr {
    Empty,                      // FreeList is empty
    Overranged,                 // FreeList overranged
    Oversized,                  // CentralFreeLists oversized
}

impl CentralFreeListErr {
    /// Fetch span from `PageHeap`.
    /// 
    /// May return: `Ok(ptr)`, `Err(TcmallocErr::PageAlloc(pages))`.
    pub fn resolve_empty_err(
        page_heap: &mut PageHeap,
        pages: usize
    ) -> Result<*mut usize, TcmallocErr> {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        match page_heap.alloc_pages(pages) {
            Ok(addr) => Ok(addr as *mut usize),
            Err(err) => {
                match err {
                    PageHeapErr::Unsupported => {
                        Err(TcmallocErr::PageAlloc(pages))
                    }
                }
            }
        }
    }

    /// Fetch span from `PageHeap`.
    /// 
    /// May return: `Ok(ptr)` points to allocated object, `Err(TcmallocErr::SpanAlloc(pages))`.
    pub fn resolve_empty_err_and_redo(
        cpu_cache: &mut CpuCache,
        transfer_caches: &mut TransferCaches,
        central_free_lists: &mut CentralFreeLists,
        align: usize,
        size_class: (usize, SizeClassInfo),
        page_heap: &mut PageHeap,
        pages: usize
    ) -> Result<*mut usize, TcmallocErr> {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        match page_heap.alloc_pages(pages) {
            Ok(addr) => {
                central_free_lists.refill_span_without_check(pages, addr as *mut usize);

                let ptr = TransferCacheErr::resolve_empty_err_and_redo(cpu_cache, transfer_caches, central_free_lists, align, size_class)
                    .expect("[tcmalloc] failed to refill span!");

                Ok(ptr)
            }
            Err(err) => {
                match err {
                    PageHeapErr::Unsupported => {
                        Err(TcmallocErr::SpanAlloc(pages))
                    }
                }
            }
        }
    }

    /// Shrink overranged `CentralFreeList` to `PageHeap`.
    ///  
    /// May return: `Ok(())`, `Err(TcmallocErr::PageDealloc(addr, pages))`.
    pub fn resolve_overranged_err(
        central_free_lists: &mut CentralFreeLists,
        page_heap: &mut PageHeap,
        pages: usize
    ) -> Result<(), TcmallocErr> {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        while let Ok(ptr) = central_free_lists.scavenge_span(pages) {
            let addr = ptr as usize;

            if page_heap.dealloc_pages(addr, pages).is_err() {
                return Err(TcmallocErr::PageDealloc(addr, pages));
            }
        }

        Ok(())
    }

    /// Release spare span to `PageHeap`.
    /// 
    /// May return: `Ok(())`, `Err(TcmallocErr::PageDealloc(addr, pages))`.
    pub fn resolve_oversized_err(
        central_free_lists: &mut CentralFreeLists,
        page_heap: &mut PageHeap,
        pages: usize
    ) -> Result<(), TcmallocErr> {
        assert_eq!(pages > 0 && pages <= K_BASE_NUMBER_SPAN, true);

        while central_free_lists.oversized() {
            match central_free_lists.find_smallest_color() {
                None => break,
                Some(idx) => {
                    let pages = idx + 1;

                    while let Ok(ptr) = central_free_lists.scavenge_span(pages) {
                        let addr = ptr as usize;

                        if page_heap.dealloc_pages(addr, pages).is_err() {
                            return Err(TcmallocErr::PageDealloc(addr, pages));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

pub enum PageHeapErr {
    Unsupported,
}
