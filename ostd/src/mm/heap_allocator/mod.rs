// SPDX-License-Identifier: MPL-2.0

use crate::early_println;

use core::alloc::{GlobalAlloc, Layout};

pub mod central_free_list;
pub mod common;
pub mod cpu_cache;
mod linked_list;
pub mod page_heap;
mod size_class;
mod transfer_cache;

struct Tcmalloc;

unsafe impl GlobalAlloc for Tcmalloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let cpu = cpu_cache::get_current_cpu();
        let cpu_cache = cpu_cache::get_current_cpu_cache(cpu);
        let tuple = size_class::match_size_class(layout);
        match tuple {
            // Allocated by tcmalloc
            Some(inner) => {
                cpu_cache.allocate(layout.align(), inner)
            },
            // Allocated by page heap
            None => {
                todo!()
            },
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let cpu = cpu_cache::get_current_cpu();
        let cpu_cache = cpu_cache::get_current_cpu_cache(cpu);
        let tuple = size_class::match_size_class(layout);
        match tuple {
            // Deallocated by tcmalloc
            Some(inner) => {
                cpu_cache.deallocate(inner, ptr);
            },
            // Deallocated by page heap
            None => {
                todo!()
            },
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        self.dealloc(ptr, layout);

        let size = new_size;
        let align = layout.align();
        let new_layout = Layout::from_size_align_unchecked(size, align);

        self.alloc(new_layout)
    }
}

#[global_allocator]
static HEAP_ALLOCATOR: Tcmalloc = Tcmalloc;

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

const INIT_KERNEL_HEAP_SIZE: usize = common::K_PAGE_SIZE * common::K_PRIMARY_HEAP_LEN;

#[repr(align(4096))]
struct InitHeapSpace([u8; INIT_KERNEL_HEAP_SIZE]);

static HEAP_SPACE: InitHeapSpace = InitHeapSpace([0; INIT_KERNEL_HEAP_SIZE]);

pub fn init() {
    unsafe { page_heap::init(HEAP_SPACE.0.as_ptr() as usize) };
    early_println!("[tcmalloc] page_heap_base {:x}", HEAP_SPACE.0.as_ptr() as usize);
    early_println!("[tcmalloc] page_heap_bound {:x}", HEAP_SPACE.0.as_ptr() as usize + INIT_KERNEL_HEAP_SIZE);
    unsafe { central_free_list::init() };
    unsafe { cpu_cache::init() };
}
