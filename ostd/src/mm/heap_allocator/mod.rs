// SPDX-License-Identifier: MPL-2.0

use tcmalloc_rs::*;

use crate::early_println;

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
    early_println!("[tcmalloc] page_heap init.");
    unsafe { page_heap::init(HEAP_SPACE.0.as_ptr() as usize) };
    early_println!("[tcmalloc] central_free_list init.");
    unsafe { central_free_list::init() };
    early_println!("[tcmalloc] cpu_cache init.");
    unsafe { cpu_cache::init() };
    early_println!("[tcmalloc] init done.");
}
