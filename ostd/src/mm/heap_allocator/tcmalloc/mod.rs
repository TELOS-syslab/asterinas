// SPDX-License-Identifier: MPL-2.0

use core::alloc::Layout;

use common::{K_BASE_NUMBER_SPAN, K_PAGE_SHIFT, K_PAGE_SIZE};

use error_handler::*;
use cpu_cache::CpuCaches;
use size_class::match_size_class;
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
    /// May return: `Ok(ptr)`, `Err(TcmallocErr`.
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
                            Err(()) => Err(TcmallocErr::PageAlloc(pages)),
                        }
                    },
                    true => {
                        match central_free_lists.alloc_span(pages) {
                            Ok(ptr) => Ok(ptr as *mut u8),
                            Err(err) => {
                                match err {
                                    CentralFreeListErr::Empty => {
                                        if let Err(err) = CentralFreeListErr::resolve_empty_err(central_free_lists, page_heap, pages) {
                                            return Err(err);
                                        }
                                    },
                                    CentralFreeListErr::Unsupported(addr, pages) => {
                                        if let Err(err) = CentralFreeListErr::resolve_unsupported_err(addr, pages) {
                                            return Err(err);
                                        }
                                    },
                                    _ => panic!("[tcmalloc] returned unexpected error!"),
                                }

                                Err(TcmallocErr::Redo)
                            },
                        }
                    },
                }
            },
            Some(size_class) => {
                let align = layout.align();
                let cpu_cache = self.cpu_caches.get_current_cpu_cache(cpu);

                // Try to allocate object from current CpuCache.
                match cpu_cache.alloc_object_aligned(align, size_class) {
                    Ok(ptr) => Ok(ptr),
                    Err(err) => {
                        // FIXME: Lock point.
                        let transfer_caches = &mut self.transfer_caches;
                        let central_free_lists = &mut self.central_free_lists;
                        let page_heap = &mut self.page_heap;

                        match err {
                            CpuCacheErr::Empty => {
                                match CpuCacheErr::resolve_empty_err(cpu_cache, transfer_caches, size_class) {
                                    Ok(()) => {},
                                    Err(err) => {
                                        match err {
                                            TransferCacheErr::Empty => {
                                                match TransferCacheErr::resolve_empty_err(transfer_caches, central_free_lists, size_class) {
                                                    Ok(()) => {},
                                                    Err(err) => {
                                                        let pages = size_class.1.pages();

                                                        match err {
                                                            CentralFreeListErr::Empty => {
                                                                if let Err(err) =  CentralFreeListErr::resolve_empty_err(central_free_lists, page_heap, pages) {
                                                                    return Err(err);
                                                                }
                                                            },
                                                            CentralFreeListErr::Unsupported(addr, pages) => {
                                                                if let Err(err) = CentralFreeListErr::resolve_unsupported_err(addr, pages) {
                                                                    return Err(err);
                                                                }
                                                            },
                                                            _ => panic!("[tcmalloc] returned unexpected error!"), 
                                                        }
                                                    }
                                                }
                                            },
                                            TransferCacheErr::Full => {
                                                match TransferCacheErr::resolve_full_err(transfer_caches, central_free_lists, size_class) {
                                                    Ok(()) => {},
                                                    Err(err) => {
                                                        let pages = size_class.1.pages();

                                                        match err {
                                                            CentralFreeListErr::Overranged => {
                                                                if let Err(err) = CentralFreeListErr::resolve_overranged_err(central_free_lists, page_heap, pages) {
                                                                    return Err(err);
                                                                }
                                                            },
                                                            CentralFreeListErr::Oversized => {
                                                                if let Err(err) = CentralFreeListErr::resolve_oversized_err(central_free_lists, page_heap, pages) {
                                                                    return Err(err);
                                                                }
                                                            },
                                                            CentralFreeListErr::Unsupported(addr, pages) => {
                                                                if let Err(err) = CentralFreeListErr::resolve_unsupported_err(addr, pages) {
                                                                    return Err(err);
                                                                }
                                                            },
                                                            _ => panic!("[tcmalloc] returned unexpected error!"),
                                                        }
                                                    }
                                                }
                                            },
                                        }
                                    },
                                }

                                Err(TcmallocErr::Redo)
                            },
                            _ => panic!("[tcmalloc] returned unexpected error!"),
                        }
                    }
                }
            }
        }
    }

    /// Try to allocate a sized object meeting the need of given `layout`.
    /// 
    /// May return: `Ok(ptr)`, `Err(TcmallocErr`.
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
                            Err(()) => Err(TcmallocErr::PageDealloc(addr, pages)),
                        }
                    },
                    true => {
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
                    },
                }
            },
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
                                match CpuCacheErr::resolve_overranged_err(cpu_cache, transfer_caches, size_class) {
                                    Ok(()) => {},
                                    Err(err) => {
                                        match err {
                                            TransferCacheErr::Full => {
                                                match TransferCacheErr::resolve_full_err(transfer_caches, central_free_lists, size_class) {
                                                    Ok(()) => {},
                                                    Err(err) => {
                                                        let pages = size_class.1.pages();

                                                        match err {
                                                            CentralFreeListErr::Overranged => {
                                                                if let Err(err) = CentralFreeListErr::resolve_overranged_err(central_free_lists, page_heap, pages) {
                                                                    return Err(err);
                                                                }
                                                            },
                                                            CentralFreeListErr::Oversized => {
                                                                if let Err(err) = CentralFreeListErr::resolve_oversized_err(central_free_lists, page_heap, pages) {
                                                                    return Err(err);
                                                                }

                                                            },
                                                            CentralFreeListErr::Unsupported(addr, pages) => {
                                                                if let Err(err) = CentralFreeListErr::resolve_unsupported_err(addr, pages) {
                                                                    return Err(err);
                                                                }
                                                            },
                                                            _ => panic!("[tcmalloc] returned unexpected error!"),
                                                        }
                                                    },
                                                }
                                            }
                                            _ => panic!("[tcmalloc] returned unexpected error!"),
                                        }
                                    }
                                }
                            },
                            CpuCacheErr::Oversized => {

                            },
                            _ => panic!("[tcmalloc] returned unexpected error!"),
                        }

                        Err(TcmallocErr::Redo)
                    }
                }
            }
        }
    }
}
