// SPDX-License-Identifier: MPL-2.0

use super::{
    common::{K_BASE_NUMBER_CLASSES, K_MAX_CPU_CACHE_SIZE, K_MAX_OVERRANGES},
    error_handler::CpuCacheErr,
    linked_list::ElasticList,
    size_class::{get_size_class_info, SizeClassInfo},
};

pub struct CpuCache {
    free_lists: [ElasticList; K_BASE_NUMBER_CLASSES],
    size: usize,
    max_size: usize,
}

impl CpuCache {
    const fn new() -> Self {
        const ARRAY_REPEAT_VALUE: ElasticList = ElasticList::new();

        Self {
            free_lists: [ARRAY_REPEAT_VALUE; K_BASE_NUMBER_CLASSES],
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

    /// Allocate an aligned object from the current `CpuCache`.
    /// 
    /// Return `Ok(ptr)` where `ptr` is aligned to given `align` and points to the allocated object.
    /// 
    /// Return `Err(CpuCacheErr::Empty)` if the `ElasticList` of given `size_class` is empty.
    pub fn alloc_aligned_object(
        &mut self,
        align: usize,
        size_class: (usize, SizeClassInfo)
    ) -> Result<*mut usize, CpuCacheErr> {
        let (idx, size_class_info) = size_class;
        let free_list = &mut self.free_lists[idx];

        match free_list.pop_aligned(align) {
            None => Err(CpuCacheErr::Empty),
            Some(ptr) => {
                self.size -= size_class_info.size();
                Ok(ptr)
            },
        }
    }

    /// Scavenge a free object to `TransferCache` because of `CpuCacheErr::Overranged` or `CpuCacheErr::Oversized`.
    /// 
    /// Return `Ok(ptr)` where `ptr` points to the free object to be scavenged.
    /// 
    /// Return `Err(CpuCacheErr::Empty)` if the `ElasticList` of given `size_class` is empty.
    pub fn scavenge_object(
        &mut self,
        size_class: (usize, SizeClassInfo)
    ) -> Result<*mut usize, CpuCacheErr> {
        let (idx, size_class_info) = size_class;
        let free_list = &mut self.free_lists[idx];

        match free_list.pop() {
            None => {
                free_list.reset();
                Err(CpuCacheErr::Empty)
            },
            Some(ptr) => {
                self.size -= size_class_info.size();
                Ok(ptr)
            },
        }
    }

    /// Deallocate an object to the current `CpuCache`.
    /// 
    /// Return `Ok(())` if no overranging occurred.
    /// 
    /// Return `Err(CpuCacheErr::Overranged)` if the `ElasticList` of give `size_class` overranged its `max_len`
    /// more than `max_overrang` times. The `ElasticList` should be shrinked. 
    /// 
    /// Return `Err(CpuCacheErr::Oversized)` if the free memory size of `CpuCache` exceeded its `max_size`.
    /// Its spare free memory should be reclaimed.
    pub fn dealloc_object(
        &mut self,
        size_class: (usize, SizeClassInfo),
        ptr: *mut u8
    ) -> Result<(), CpuCacheErr> {
        let (idx, size_class_info) = size_class;
        let free_list = &mut self.free_lists[idx];

        free_list.push(ptr as *mut usize);
        self.size += size_class_info.size();

        if free_list.overranged() {
            return Err(CpuCacheErr::Overranged);
        }

        if self.oversized() {
            return Err(CpuCacheErr::Oversized);
        }

        Ok(())
    }

    /// Refill an object from `TransferCache` without checking the `CpuCacheErr`.
    pub fn refill_object_without_check(
        &mut self,
        size_class: (usize, SizeClassInfo),
        ptr: *mut usize
    ) {
        let (idx, size_class_info) = size_class;
        let free_list = &mut self.free_lists[idx];

        free_list.push(ptr);
        self.size += size_class_info.size();
    }

    /// Return the index of the `ElasticList` with the smallest `color`.
    pub fn find_smallest_color(&self) -> Option<usize> {
        let free_lists = &self.free_lists;
        let mut min_idx = 0usize;
        let mut min_color = core::usize::MAX;

        for (idx, free_list) in free_lists.iter().enumerate() {
            if !free_list.is_empty() && free_list.color() < min_color {
                min_idx = idx;
                min_color = free_list.color();
            }
        }

        match min_color < core::usize::MAX {
            false => None,
            true => Some(min_idx),
        }
    }

    pub fn oversized(&self) -> bool {
        self.size > self.max_size
    }
}

/// Constant `C` refers to the number of logical CPUs.
pub struct CpuCaches<const C: usize> {
    cpu_caches: [CpuCache; C],
}

impl<const C: usize> CpuCaches<C> {
    pub const fn new() -> Self {
        const ARRAY_REPEAT_VALUE: CpuCache = CpuCache::new();

        Self {
            cpu_caches: [ARRAY_REPEAT_VALUE; C],
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
