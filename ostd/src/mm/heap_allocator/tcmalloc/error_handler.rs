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
    Redo,
    PageAlloc(usize),
    PageDealloc(usize, usize),
}

pub enum CpuCacheErr {
    Empty,          // FreeList is empty
    Overranged,     // FreeList overranged
    Oversized,      // CpuCache oversized
}

impl CpuCacheErr {
    /// Fetch free sized objects from `TransferCache`.
    /// 
    /// May return: `Ok(())`, `TransferCacheErr::Empty`, `Err(TransferCacheErr::Full)`.
    pub fn resolve_empty_err(cpu_cache: &mut CpuCache, transfer_caches: &mut TransferCaches, size_class: (usize, SizeClassInfo)) -> Result<(), TransferCacheErr> {
        let (size_class_idx, size_class_info) = size_class;
        let num_to_move = size_class_info.num_to_move();
        let transfer_cache = transfer_caches.get_current_transfer_cache(size_class_idx);
        let mut oversized = false;
    
        for _i in 0..num_to_move {
            match transfer_cache.alloc_object() {
                Ok(ptr) => {
                    match cpu_cache.dealloc_object(size_class, ptr as *mut u8) {
                        Ok(()) => {},
                        Err(err) => {
                            match err {
                                CpuCacheErr::Oversized => {
                                    oversized = true;
                                    break;
                                },
                                _ => panic!("[tcmalloc] returned unexpected error!"),
                            }
                        }
                    }
                },
                Err(err) => {
                    match err {
                        TransferCacheErr::Empty => return Err(err),
                        _ => panic!("[tcmalloc] returned unexpected error!"),
                    }
                },
            }
        }

        if oversized {
            CpuCacheErr::resolve_oversized_err(cpu_cache, transfer_caches)?
        }

        Ok(())
    }

    /// Release spare free memory to `TransferCache`.
    /// 
    /// May return: `Ok(())`, `Err(TransferCacheErr::Full)`.
    pub fn resolve_oversized_err(cpu_cache: &mut CpuCache, transfer_caches: &mut TransferCaches) -> Result<(), TransferCacheErr> {
        while cpu_cache.size() > cpu_cache.max_size() {
            match cpu_cache.find_smallest_color() {
                None => break,
                Some(idx) => {
                    let size_class_info = get_size_class_info(idx).unwrap();
                    let size_class = (idx, size_class_info);
                    let num_to_move = size_class_info.num_to_move();
                    let transfer_cache = transfer_caches.get_current_transfer_cache(idx);

                    for _i in 0..num_to_move {
                        match cpu_cache.alloc_object(size_class) {
                            Ok(ptr) => {
                                transfer_cache.dealloc_object(ptr as *mut usize)?
                            },
                            Err(err) => {
                                match err {
                                    // Avoid thrash.
                                    CpuCacheErr::Empty => {},
                                    _ => panic!("[tcmalloc] returned unexpected error!"),
                                }
                            },
                        }
                    }
                },
            }
        }

        Ok(())
    }

    /// Shrink overranged `FreeList` and release free memory to `TransferCache`.
    /// 
    /// May return: `Ok(())`, `Err(TransferCacheErr::Full)`.
    pub fn resolve_overranged_err(cpu_cache: &mut CpuCache, transfer_caches: &mut TransferCaches, size_class: (usize, SizeClassInfo)) -> Result<(), TransferCacheErr> {
        let transfer_cache = transfer_caches.get_current_transfer_cache(size_class.0);
        
        while let Ok(ptr) = cpu_cache.alloc_object(size_class) {
            transfer_cache.dealloc_object(ptr as *mut usize)?
        }

        Ok(())
    }
}

pub enum TransferCacheErr {
    Empty,
    Full,
}

impl TransferCacheErr {
    /// Fetch span from `CentralFreeList`.
    /// 
    /// May return: `Ok(())`, `Err(CentralFreeListErr::Unsupported)`, `Err(CentralFreeListErr::Empty)`
    pub fn resolve_empty_err(transfer_caches: &mut TransferCaches, central_free_lists: &mut CentralFreeLists, size_class: (usize, SizeClassInfo)) -> Result<(), CentralFreeListErr> {
        let (idx, size_class_info) = size_class;
        let pages = size_class_info.pages();
        let transfer_cache = transfer_caches.get_current_transfer_cache(idx);
        
        match central_free_lists.alloc_span(pages) {
            Ok(ptr) => {
                let base = ptr as usize;
                let bound = base + (pages << K_PAGE_SHIFT);

                transfer_cache.dealloc_span(base, bound, size_class_info.size()).unwrap();

                Ok(())
            },
            Err(err) => Err(err),
        }
    }

    /// Release span to `CentralFreeList`.
    /// 
    /// May return: `Ok(())`, `Err(CentralFreeListErr::Unsupported)`, `Err(CentralFreeListErr::Oversized)`, `Err(CentralFreeListErr::Overranged)`.
    pub fn resolve_full_err(transfer_caches: &mut TransferCaches, central_free_lists: &mut CentralFreeLists, size_class: (usize, SizeClassInfo)) -> Result<(), CentralFreeListErr> {
        let (idx, size_class_info) = size_class;
        let pages = size_class_info.pages();
        let transfer_cache = transfer_caches.get_current_transfer_cache(idx);

        while transfer_cache.full_num() > 0 {
            if let Some(idx) = transfer_cache.find_full() {
                let base = transfer_cache.get_base(idx);

                assert_eq!(base > 0, true);

                while transfer_cache.alloc_object_with_index(idx).is_ok() {}

                return central_free_lists.dealloc_span(pages, base as *mut usize);
            }
        }

        Ok(())
    }
}

pub enum CentralFreeListErr {
    Empty,                      // FreeList is empty
    Overranged,                 // FreeList overranged
    Oversized,                  // CentralFreeLists oversized
    Unsupported(usize, usize),  // Try to allocate unsupported span
}

impl CentralFreeListErr {
    /// Fetch span from `PageHeap`.
    /// 
    /// May return: `Ok(())`, `Err(TcmallocErr::PageAlloc(pages))`, `Err(TcmallocErr::PageDealloc(addr, pages))`, `Err(TcmallocErr::Overlay)`.
    pub fn resolve_empty_err(central_free_lists: &mut CentralFreeLists, page_heap: &mut PageHeap, pages: usize) -> Result<(), TcmallocErr> {
        assert_eq!(pages <= K_BASE_NUMBER_SPAN, true);

        match page_heap.try_to_match_span(pages) {
            None => Err(TcmallocErr::PageAlloc(pages)),
            Some(addr) => {
                match central_free_lists.dealloc_span(pages, addr as *mut usize) {
                    Ok(()) => Ok(()),
                    Err(err) => {
                        match err {
                            CentralFreeListErr::Overranged => {
                                CentralFreeListErr::resolve_overranged_err(central_free_lists, page_heap, pages)
                            },
                            CentralFreeListErr::Oversized => {
                                CentralFreeListErr::resolve_oversized_err(central_free_lists, page_heap, pages)
                            },
                            CentralFreeListErr::Unsupported(addr, pages) => {
                                CentralFreeListErr::resolve_unsupported_err(addr, pages)
                            },
                            _ => panic!("[tcmalloc] returned unexpected error!"),
                        }
                    },
                }
            }
        }
    }

    /// Shrink overranged `CentralFreeList` to `PageHeap`.
    ///  
    /// May return: `Ok(())`, `Err(TcmallocErr::PageDealloc(addr, pages))`.
    pub fn resolve_overranged_err(central_free_lists: &mut CentralFreeLists, page_heap: &mut PageHeap, pages: usize) -> Result<(), TcmallocErr> {
        assert_eq!(pages <= K_BASE_NUMBER_SPAN, true);

        while let Ok(ptr) = central_free_lists.alloc_span(pages) {
            let addr = ptr as usize;

            match page_heap.dealloc_pages(addr, pages) {
                Ok(()) => {},
                Err(()) => return Err(TcmallocErr::PageDealloc(addr, pages)),
            }
        }

        Ok(())
    }

    /// Release spare span to `PageHeap`.
    /// 
    /// May return: `Ok(())`, `Err(TcmallocErr::PageDealloc(addr, pages))`.
    pub fn resolve_oversized_err(central_free_lists: &mut CentralFreeLists, page_heap: &mut PageHeap, pages: usize) -> Result<(), TcmallocErr> {
        assert_eq!(pages <= K_BASE_NUMBER_SPAN, true);

        while central_free_lists.len() > central_free_lists.max_len() {
            match central_free_lists.find_smallest_color() {
                None => break,
                Some(idx) => {
                    while let Ok(ptr) = central_free_lists.alloc_span_with_index(idx) {
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

    /// Tell the upstream to process by the context.
    /// 
    /// Return `Err(TcmallocErr::Overlay)`.
    pub fn resolve_unsupported_err(addr: usize, pages: usize) -> Result<(), TcmallocErr> {
        match addr == 0 {
            false => Err(TcmallocErr::PageAlloc(pages)),
            true => Err(TcmallocErr::PageDealloc(addr, pages)),
        }
    }
}
