// SPDX-License-Identifier: MPL-2.0

use core::{
    alloc::{GlobalAlloc, Layout},
    panic,
};

use tcmalloc::{
    common::{K_PAGE_SHIFT, K_PAGE_SIZE, K_PRIMARY_HEAP_LEN},
    error_handler::TcmallocErr,
    Tcmalloc,
};

use super::{page::meta::mapping, Vaddr};
use crate::{
    early_println,
    mm::{kspace::LINEAR_MAPPING_BASE_VADDR, paddr_to_vaddr, page::{allocator::PAGE_ALLOCATOR, Page}},
};

mod tcmalloc;

// FIXME: The number of cpu should be introduced at runtime.
const K_MAX_PAGE_NUMBER: usize = 1024;
const CPU_NUMBER: usize = 16;
const INIT_KERNEL_HEAP_SIZE: usize = K_PAGE_SIZE * K_PRIMARY_HEAP_LEN;

#[global_allocator]
static mut HEAP_ALLOCATOR: Tcmalloc<CPU_NUMBER> = Tcmalloc::new();

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

#[repr(align(4096))]
struct InitHeapSpace([u8; INIT_KERNEL_HEAP_SIZE]);

static PRIMARY_HEAP: InitHeapSpace = InitHeapSpace([0; INIT_KERNEL_HEAP_SIZE]);

pub fn init() {
    // TODO: SAFETY
    unsafe { HEAP_ALLOCATOR.init(K_MAX_PAGE_NUMBER, PRIMARY_HEAP.0.as_ptr() as usize) };
}

// FIXME: Implement this function by APIs provided by
fn get_current_cpu() -> usize {
    0
}

unsafe impl<const C: usize> GlobalAlloc for Tcmalloc<C> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // early_println!("[tcmalloc] alloc, layout = {:#?}", layout);
        let cpu = get_current_cpu();

        match HEAP_ALLOCATOR.allocate(cpu, layout) {
            Ok(ptr) => {
                ptr
            }
            Err(err) => {
                let mut pages = 0usize;

                match err {
                    TcmallocErr::PageAlloc(_pages) => pages = _pages,
                    TcmallocErr::SpanAlloc(_pages) => pages = _pages,
                    TcmallocErr::PageDealloc(_addr, _pages) => {
                        panic!("Should not reach here while allocating.");
                    }
                }

                // TODO: Change to the Latest API after the
                // latest commits merged.
                let new_layout = core::alloc::Layout::from_size_align(
                    pages << K_PAGE_SHIFT,
                    K_PAGE_SIZE,
                )
                .unwrap();

                // let pages = crate::mm::page::allocator::alloc_contiguous(layout, metadata_fn);
                // let pages = core::mem::ManuallyDrop::new(pages);
                // let addr = pages.paddr();

                let paddr_from_page_allocator = PAGE_ALLOCATOR
                    .get()
                    .unwrap()
                    .lock()
                    .alloc(
                        new_layout
                    )
                    .expect("Failed to allocate a span from PageAllocator.");
                

                // `ptr` points to the new span.
                let ptr = paddr_to_vaddr(paddr_from_page_allocator) as *mut u8;

                match err {
                    TcmallocErr::PageAlloc(_pages) => ptr,
                    TcmallocErr::SpanAlloc(_pages) => {
                        HEAP_ALLOCATOR.refill_span_and_redo(cpu, ptr, layout, pages).unwrap()
                    }
                    TcmallocErr::PageDealloc(_addr, _pages) => {
                        panic!("Should not reach here while allocating.");
                    }
                }
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // early_println!("[tcmalloc] dealloc, layout = {:#?}", layout);

        let cpu = get_current_cpu();

        match HEAP_ALLOCATOR.deallocate(cpu, ptr, layout) {
            Ok(()) => {}
            Err(err) => match err {
                TcmallocErr::PageAlloc(_) => {
                    panic!("Should not reach here while deallocating.");
                }
                TcmallocErr::SpanAlloc(_) => {
                    panic!("Should not reach here while deallocating.");
                }
                TcmallocErr::PageDealloc(addr, pages) => {
                    // crate::early_println!("addr: {:x}", addr);
                    // let page = Page::<KernelMeta>::from_raw(addr - LINEAR_MAPPING_BASE_VADDR);
                    PAGE_ALLOCATOR.get().unwrap().lock().dealloc(
                        addr - LINEAR_MAPPING_BASE_VADDR,
                        pages,
                    );
                }
            }
        }
    }
}
