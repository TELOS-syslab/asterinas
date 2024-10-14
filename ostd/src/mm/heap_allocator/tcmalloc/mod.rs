// SPDX-License-Identifier: MPL-2.0

use core::alloc::Layout;

use common::{K_BASE_NUMBER_SPAN, K_PAGE_SHIFT, K_PAGE_SIZE};

use error_handler::*;
use cpu_cache::CpuCaches;
use size_class::{get_size_class_info, match_size_class};
use transfer_cache::TransferCaches;
use central_free_list::CentralFreeLists;
use page_heap::PageHeap;

mod central_free_list;
pub mod common;
mod cpu_cache;
pub mod error_handler;
mod linked_list;
mod page_heap;
mod size_class;
mod transfer_cache;

pub struct Tcmalloc<const C: usize> {
    cpu_caches: CpuCaches<C>,
    transfer_caches: TransferCaches,
    central_free_lists: CentralFreeLists,
    page_heap: PageHeap,
}

impl<const C: usize> Tcmalloc<C> {
    pub const fn new() -> Self {
        Self {
            cpu_caches: CpuCaches::new(),
            transfer_caches: TransferCaches::new(),
            central_free_lists: CentralFreeLists::new(),
            page_heap: PageHeap::new(),
        }
    }

    // FIXME: To be called only once.
    pub fn init(&mut self, max_len: usize, base: usize) {
        self.cpu_caches.init();
        self.central_free_lists.init(max_len);
        self.page_heap.init(base);
    }

    /// Try to allocate a sized object meeting the need of given `layout`.
    /// 
    /// May return: `Ok(ptr)`, `Err(TcmallocErr::PageAlloc(pages)`.
    pub fn allocate(&mut self, cpu: usize, layout: Layout) -> Result<*mut u8, TcmallocErr> {
        match match_size_class(layout) {
            None => {
                let size = core::cmp::max(layout.size(), layout.align());
                let pages = (size + K_PAGE_SIZE - 1) >> K_PAGE_SHIFT;
                // FIXME: Lock point.
                let central_free_lists = &mut self.central_free_lists;
                let page_heap = &mut self.page_heap;

                match pages <= K_BASE_NUMBER_SPAN {
                    false => {
                        match page_heap.alloc_pages(pages) {
                            Ok(addr) => Ok(addr as *mut u8),
                            Err(_) => Err(TcmallocErr::PageAlloc(pages)),
                        }
                    }
                    true => {
                        match central_free_lists.alloc_span_object(pages) {
                            Ok(ptr) => Ok(ptr as *mut u8),
                            Err(err) => {
                                match err {
                                    CentralFreeListErr::Empty => {
                                        match CentralFreeListErr::resolve_empty_err(page_heap, pages) {
                                            Ok(ptr) => Ok(ptr as *mut u8),
                                            Err(err) => Err(err),
                                        }
                                    }
                                    _ => panic!("[tcmalloc] returned unexpected error!"),
                                }
                            }
                        }
                    }
                }
            }
            Some(size_class) => {
                let align = layout.align();
                let cpu_cache = self.cpu_caches.get_current_cpu_cache(cpu);

                // Try to allocate object from current CpuCache.
                match cpu_cache.alloc_aligned_object(align, size_class) {
                    Ok(ptr) => Ok(ptr as *mut u8),
                    Err(err) => {
                        // FIXME: Lock point.
                        let transfer_caches = &mut self.transfer_caches;
                        let central_free_lists = &mut self.central_free_lists;
                        let page_heap = &mut self.page_heap;

                        match err {
                            CpuCacheErr::Empty => {
                                match CpuCacheErr::resolve_empty_err_and_redo(
                                    cpu_cache,
                                    transfer_caches,
                                    align,
                                    size_class
                                ) {
                                    Ok(ptr) => Ok(ptr as *mut u8),
                                    Err(err) => {
                                        match err {
                                            TransferCacheErr::Empty => {
                                                match TransferCacheErr::resolve_empty_err_and_redo(
                                                    cpu_cache,
                                                    transfer_caches,
                                                    central_free_lists,
                                                    align,
                                                    size_class
                                                ) {
                                                    Ok(ptr) => Ok(ptr as *mut u8),
                                                    Err(err) => {
                                                        let pages = size_class.1.pages();

                                                        match err {
                                                            CentralFreeListErr::Empty => {
                                                                match CentralFreeListErr::resolve_empty_err_and_redo(
                                                                    cpu_cache,
                                                                    transfer_caches,
                                                                    central_free_lists,
                                                                    align,
                                                                    size_class,
                                                                    page_heap,
                                                                    pages
                                                                ) {
                                                                    Ok(ptr) => Ok(ptr as *mut u8),
                                                                    Err(err) => Err(err),
                                                                }
                                                            }
                                                            _ => panic!("[tcmalloc] returned unexpected error!"), 
                                                        }                                                        
                                                    }
                                                }
                                            }
                                            _ => panic!("[tcmalloc] returned unexpected error!"),
                                        }
                                    }
                                }
                            }
                            _ => panic!("[tcmalloc] returned unexpected error!"),
                        }
                    }
                }
            }
        }
    }

    /// Allocate a sized object from the refilled span.
    pub fn refill_span_and_redo(&mut self, cpu: usize, ptr: *mut u8, layout: Layout, pages: usize) -> Result<*mut u8, ()> {
        let align = layout.align();
        let size_class = match_size_class(layout).unwrap();

        let cpu_cache = self.cpu_caches.get_current_cpu_cache(cpu);
        // FIXME: Lock point.
        let transfer_caches = &mut self.transfer_caches;
        let central_free_lists = &mut self.central_free_lists;

        central_free_lists.refill_span_without_check(pages, ptr as *mut usize);

        let ptr = TransferCacheErr::resolve_empty_err_and_redo(cpu_cache, transfer_caches, central_free_lists, align, size_class)
            .expect("[tcmalloc] failed to refill span!");

        Ok(ptr as *mut u8)
    }

    /// Try to allocate a sized object meeting the need of given `layout`.
    /// 
    /// May return: `Ok(ptr)`, `Err(TcmallocErr)`.
    pub fn deallocate(&mut self, cpu: usize, ptr: *mut u8, layout: Layout) -> Result<(), TcmallocErr> {
        match match_size_class(layout) {
            None => {
                let size = layout.size();
                let pages = (size + K_PAGE_SIZE - 1) >> K_PAGE_SHIFT;
                // FIXME: Lock point.
                let central_free_lists = &mut self.central_free_lists;
                let page_heap = &mut self.page_heap;
                let addr = ptr as usize;

                match pages <= K_BASE_NUMBER_SPAN {
                    false => {
                        match page_heap.dealloc_pages(addr, pages) {
                            Ok(()) => Ok(()),
                            Err(_) => Err(TcmallocErr::PageDealloc(addr, pages)),
                        }
                    }
                    true => {
                        match central_free_lists.dealloc_span_object(pages, addr as *mut usize) {
                            Ok(()) => Ok(()),
                            Err(err) => {
                                match err {
                                    CentralFreeListErr::Overranged => {
                                        CentralFreeListErr::resolve_overranged_err(central_free_lists, page_heap, pages)
                                    }
                                    CentralFreeListErr::Oversized => {
                                        CentralFreeListErr::resolve_oversized_err(central_free_lists, page_heap, pages)
                                    }
                                    _ => panic!("[tcmalloc] returned unexpected error!"),
                                }
                            }
                        }
                    }
                }
            }
            Some(size_class) => {
                let cpu_cache = self.cpu_caches.get_current_cpu_cache(cpu);

                match cpu_cache.dealloc_object(size_class, ptr) {
                    Ok(()) => Ok(()),
                    Err(err) => {
                        // Lock point.
                        let transfer_caches = &mut self.transfer_caches;
                        let central_free_lists = &mut self.central_free_lists;
                        let page_heap = &mut self.page_heap;

                        match err {
                            CpuCacheErr::Overranged => {
                                CpuCacheErr::resolve_overranged_err(cpu_cache, transfer_caches, size_class);
                                Ok(())
                            }
                            CpuCacheErr::Oversized => {
                                match CpuCacheErr::resolve_oversized_err(cpu_cache, transfer_caches) {
                                    Ok(()) => Ok(()),
                                    Err((idx, err)) => {
                                        let size_class_info = get_size_class_info(idx).unwrap();
                                        let pages = size_class_info.pages();

                                        match err {
                                            TransferCacheErr::Full => {
                                                match TransferCacheErr::resolve_full_err_with_index(
                                                    transfer_caches,
                                                    central_free_lists,
                                                    idx
                                                ) {
                                                    Ok(()) => Ok(()),
                                                    Err(err) => {
                                                        match err {
                                                            CentralFreeListErr::Overranged => {
                                                                CentralFreeListErr::resolve_overranged_err(
                                                                    central_free_lists,
                                                                    page_heap,
                                                                    pages
                                                                )
                                                            }
                                                            CentralFreeListErr::Oversized => {
                                                                CentralFreeListErr::resolve_oversized_err(
                                                                    central_free_lists,
                                                                    page_heap,
                                                                    pages
                                                                )
                                                            }
                                                            _ => panic!("[tcmalloc] returned unexpected error!"),                                                      }
                                                    }
                                                }
                                            }
                                            _ => panic!("[tcmalloc] returned unexpected error!"),
                                        }
                                    }
                                }
                            }
                            _ => panic!("[tcmalloc] returned unexpected error!"),
                        }
                    }
                }
            }
        }
    }
}
