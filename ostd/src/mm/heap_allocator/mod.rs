// SPDX-License-Identifier: MPL-2.0

use core::{
    alloc::{GlobalAlloc, Layout},
    panic,
};

use log::debug;
use tcmalloc::{
    common::{K_BASE_NUMBER_SPAN, K_PAGE_SHIFT, K_PAGE_SIZE, K_PRIMARY_HEAP_LEN},
    error_handler::TcmallocErr,
    Tcmalloc,
};

use super::{page::meta::mapping, Vaddr};
use crate::{
    early_println,
    mm::{paddr_to_vaddr, page::allocator::PAGE_ALLOCATOR},
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
    early_println!("[tcmalloc] init.");
    unsafe { HEAP_ALLOCATOR.init(K_MAX_PAGE_NUMBER, PRIMARY_HEAP.0.as_ptr() as usize) };
}

// FIXME: Implement this function by APIs provided by
fn get_current_cpu() -> usize {
    0
}

unsafe impl<const C: usize> GlobalAlloc for Tcmalloc<C> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let cpu = get_current_cpu();

        match HEAP_ALLOCATOR.allocate(cpu, layout) {
            Ok(ptr) => {
                // early_println!("[tcmalloc] alloc ptr = {:x}, size = {}", ptr as usize, layout.size());
                ptr
            }
            Err(err) => {
                match err {
                    TcmallocErr::Redo => self.alloc(layout),
                    TcmallocErr::PageAlloc(pages) => {
                        // TODO: Change to the Latest API after the
                        // latest commits merged.
                        let new_layout = core::alloc::Layout::from_size_align(
                            pages << K_PAGE_SHIFT,
                            K_PAGE_SIZE,
                        )
                        .unwrap();

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

                        if pages > K_BASE_NUMBER_SPAN {
                            debug!("[+]Successfully allocated a span from PageAllocator and give it out, Layout = {:?}", new_layout);
                            ptr
                        } else {
                            // Add the new page span to the heap.
                            // TODO: Rewrite the name of the function.
                            // Dealloc may be too confusing.
                            self.dealloc(ptr, new_layout);

                            debug!("[-]Successfully Add a span to Heap. Layout = {:?}", new_layout);
                            
                            self.alloc(layout)
                        }
                    }
                    TcmallocErr::PageDealloc(_addr, _pages) => {
                        panic!("Should not reach here while allocating.");
                    }
                }
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let cpu = get_current_cpu();

        match HEAP_ALLOCATOR.deallocate(cpu, ptr, layout) {
            Ok(()) => {
                // early_println!("[tcmalloc] dealloc ptr = {:x}, size = {}", ptr as usize, layout.size());
            }
            Err(err) => match err {
                TcmallocErr::Redo => self.dealloc(ptr, layout),
                TcmallocErr::PageAlloc(_) => {
                    panic!("Should not reach here while deallocating.");
                }
                TcmallocErr::PageDealloc(addr, pages) => {
                    PAGE_ALLOCATOR.get().unwrap().lock().dealloc(
                        mapping::meta_to_page::<crate::arch::mm::PagingConsts>(addr as Vaddr),
                        pages,
                    );
                }
            },
        }
    }
}
