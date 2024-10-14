// SPDX-License-Identifier: MPL-2.0

#[cfg(feature = "page_shift_12")]
pub const K_PAGE_SHIFT: usize = 12;
#[cfg(feature = "page_shift_12")]
pub const K_BASE_NUMBER_CLASSES: usize = 44;
#[cfg(feature = "page_shift_12")]
pub const K_BASE_NUMBER_SPAN: usize = 7;
#[cfg(feature = "page_shift_12")]
// FIXME: `TransferCache` should be dynamic.
pub const K_MAX_NUMBER_SPAN: usize = 512;
#[cfg(feature = "default")]
pub const K_MAX_CPU_CACHE_SIZE: usize = 256 * 1024;     // 256KB

pub const K_PAGE_SIZE: usize = 1 << K_PAGE_SHIFT;

// The number of times that a deallocation can cause a freelist to
// go over its `max_len()`.
pub const K_MAX_OVERRANGES: usize = 4;

pub const K_PRIMARY_HEAP_LEN: usize = 256;

pub const K_FULL_SCALE: usize = 2;
