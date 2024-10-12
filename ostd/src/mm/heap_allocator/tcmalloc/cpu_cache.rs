// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_BASE_NUMBER_CLASSES, K_MAX_CPU_CACHE_SIZE, K_MAX_OVERRANGES},
    error_handler::CpuCacheErr,
    linked_list::LinkedList,
    size_class::{get_size_class_info, SizeClassInfo},
};

#[derive(Clone, Copy)]
pub struct CpuCache {
    free_lists: [FreeList; K_BASE_NUMBER_CLASSES],
    size: usize,
    max_size: usize,
}

impl CpuCache {
    const fn new() -> Self {
        Self {
            free_lists: [FreeList::new(); K_BASE_NUMBER_CLASSES],
            size: 0,
            max_size: 0,
        }
    }

    fn init(&mut self) {
        let free_lists = &mut self.free_lists;

        for (idx, free_list) in free_lists.iter_mut().enumerate() {
            let size_class_info = get_size_class_info(idx).unwrap();
            free_list.init(size_class_info.max_capacity(), K_MAX_OVERRANGES);
        }

        self.max_size = K_MAX_CPU_CACHE_SIZE;
    }

    /// Return `Ok(ptr)` where `ptr` is aligned to given `align` and points to the allocated object.
    /// 
    /// Return `Err(CpuCacheErr::Empty)` if the `FreeList` of given `size_class` is empty.
    pub fn alloc_object(&mut self, size_class: (usize, SizeClassInfo)) -> Result<*mut u8, CpuCacheErr> {
        let (idx, size_class_info) = size_class;
        let free_list = &mut self.free_lists[idx];

        match free_list.pop() {
            None => Err(CpuCacheErr::Empty),
            Some(ptr) => {
                self.size -= size_class_info.size();
                Ok(ptr as *mut u8)
            },
        }
    }

    /// Return `Ok(ptr)` where `ptr` is aligned to given `align` and points to the allocated object.
    /// 
    /// Return `Err(CpuCacheErr::Empty)` if the `FreeList` of given `size_class` is empty.
    pub fn alloc_object_aligned(&mut self, align: usize, size_class: (usize, SizeClassInfo)) -> Result<*mut u8, CpuCacheErr> {
        let (idx, size_class_info) = size_class;
        let free_list = &mut self.free_lists[idx];

        match free_list.pop_aligned(align) {
            None => Err(CpuCacheErr::Empty),
            Some(ptr) => {
                self.size -= size_class_info.size();
                Ok(ptr as *mut u8)
            },
        }
    }

    /// Return `Ok(())` if no overranging occurred.
    /// 
    /// Return `Err(CpuCacheErr::Oversized)` if the free memory size of `CpuCache` exceeded its `max_size`.
    /// Its spare free memory should be reclaimed.
    /// 
    /// Return `Err(CpuCacheErr::Overranged)` if the `FreeList` of give `size_class` overranged its `max_len`
    /// more than `max_overrang` times. The `FreeList` should be shrinked.
    pub fn dealloc_object(&mut self, size_class: (usize, SizeClassInfo), ptr: *mut u8) -> Result<(), CpuCacheErr> {
        let (idx, size_class_info) = size_class;
        let free_list = &mut self.free_lists[idx];

        let overranged = free_list.push(ptr as *mut usize);
        self.size += size_class_info.size();

        match overranged {
            false => {
                match self.size > self.max_size {
                    false => Ok(()),
                    true => Err(CpuCacheErr::Oversized),
                }
            },
            true => Err(CpuCacheErr::Overranged),
        }
    }

    /// Return the index of the `FreeList` with the smallest `color`.
    pub fn find_smallest_color(&self) -> Option<usize> {
        let free_lists = &self.free_lists;
        let mut min_idx = 0usize;
        let mut min_color = core::usize::MAX;

        for (idx, free_list) in free_lists.iter().enumerate() {
            if free_list.len > 0 && free_list.color < min_color {
                min_idx = idx;
                min_color = free_list.color;
            }
        }

        match min_color < core::usize::MAX {
            false => None,
            true => Some(min_idx),
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn max_size(&self) -> usize {
        self.max_size
    }
}

/// Constant `C` refers to the number of logical CPUs.
pub struct CpuCaches<const C: usize> {
    cpu_caches: [CpuCache; C],
}

impl<const C: usize> CpuCaches<C> {
    pub const fn new() -> Self {
        Self {
            cpu_caches: [CpuCache::new(); C],
        }
    }

    pub fn init(&mut self) {
        let cpu_caches = &mut self.cpu_caches;

        for cpu_cache in cpu_caches.iter_mut() {
            cpu_cache.init();
        }
    }

    pub fn get_current_cpu_cache(&mut self, cpu: usize) -> &mut CpuCache {
        assert_eq!(cpu < C, true);

        &mut self.cpu_caches[cpu]
    }
}

#[derive(Clone, Copy)]
struct FreeList {
    list: LinkedList,
    len: usize,
    max_len: usize,
    color: usize,
    overrange: usize,
    max_overrange: usize,
}

impl FreeList {
    const fn new() -> Self {
        Self {
            list: LinkedList::new(),
            len: 0,
            max_len: 0,
            color: 0,
            overrange: 0,
            max_overrange: 0,
        }
    }

    fn init(&mut self, max_len: usize, max_overrange: usize) {
        self.max_len = max_len;
        self.max_overrange = max_overrange;
    }

    fn push(&mut self, item: *mut usize) -> bool {
        let list = &mut self.list;

        // TODO: SAFETY
        unsafe { list.push(item) };

        self.len += 1;
        if self.len > self.max_len {
            self.overrange += 1;
        }
        if self.overrange > self.max_overrange {
            self.overrange = 0;
            true
        }
        else {
            false
        }
    }

    fn pop(&mut self) -> Option<*mut usize> {
        match self.list.pop() {
            None => None,
            Some(ptr) => {
                self.len -= 1;
                self.color += 1;
                Some(ptr)
            }
        }
    }

    fn pop_aligned(&mut self, align: usize) -> Option<*mut usize> {
        let list = &mut self.list;
        let mut rslt = core::ptr::null_mut();

        for node in list.iter_mut() {
            let ptr = node.value();
            if ptr.is_aligned_to(align) {
                rslt = node.pop();
                break;
            }
        }

        match rslt.is_null() {
            false => {
                self.len -= 1;
                self.color += 1;
                Some(rslt)
            }
            true => None,
        }
    }
}
