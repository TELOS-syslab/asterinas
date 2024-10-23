// SPDX-License-Identifier: MPL-2.0

use core::{
    alloc::{GlobalAlloc, Layout},
    panic,
};

use tcmalloc::{
    common::{K_PAGE_SHIFT, K_PAGE_SIZE, K_PRIMARY_HEAP_LEN},
    status::{FlowMod, MetaStat},
    Tcmalloc,
};

use super::{page::meta::mapping, Vaddr};
use crate::{
    early_println,
    mm::{
        kspace::LINEAR_MAPPING_BASE_VADDR,
        paddr_to_vaddr,
        page::{allocator::PAGE_ALLOCATOR, Page},
    },
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
    early_println!("[tcmalloc] init.");
    // TODO: SAFETY
    unsafe { HEAP_ALLOCATOR.init(K_MAX_PAGE_NUMBER, PRIMARY_HEAP.0.as_ptr() as usize) };
}

// FIXME: Implement this function by APIs provided by
fn get_current_cpu() -> usize {
    0
}

unsafe impl<const C: usize> GlobalAlloc for Tcmalloc<C> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        early_println!("[tcmalloc] alloc, layout = {:#?}.", layout);
        let mut _meta_seed = Some(MetaStat::Alloc(layout));
        let cpu = get_current_cpu();
        let mut rslt = core::ptr::null_mut();
        loop {
            early_println!("[tcmalloc] meta_stat = {:#?}", HEAP_ALLOCATOR.stat(cpu));
            match HEAP_ALLOCATOR.stat_handler(cpu, _meta_seed.clone()) {
                FlowMod::Backward => {
                    panic!();
                }
                FlowMod::Circle => continue,
                FlowMod::Exit => break,
                FlowMod::Forward => {
                    rslt = HEAP_ALLOCATOR.take_object(cpu).unwrap();
                }
            }
        }
        early_println!("[tcmalloc] rslt = {:x}", rslt as usize);
        rslt
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {}
}
