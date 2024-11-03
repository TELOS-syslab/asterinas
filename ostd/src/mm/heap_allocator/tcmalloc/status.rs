// SPDX-License-Identifier: MPL-2.0

use core::alloc::Layout;

#[derive(Debug)]
pub enum FlowMod {
    Forward,
    Circle,
    Backward,
}

#[derive(Clone, Debug)]
pub enum MetaStat {
    Ready,
    Alloc,
    Dealloc,
    Insufficient,
    LargeSize,
    Uncovered,
}

#[derive(Clone, Debug)]
pub struct MetaReg {
    layout: Option<Layout>,
    ptr: Option<*mut u8>,
    pages: Option<usize>,
}

impl MetaReg {
    pub const fn new() -> Self {
        Self {
            layout: None,
            ptr: None,
            pages: None,
        }
    }

    pub fn from(layout: Option<Layout>, ptr: Option<*mut u8>, pages: Option<usize>) -> Self {
        Self { layout, ptr, pages }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn layout(&self) -> Option<Layout> {
        self.layout
    }

    pub fn ptr(&self) -> Option<*mut u8> {
        self.ptr
    }

    pub fn pages(&self) -> Option<usize> {
        self.pages
    }

    pub fn set_pages(&mut self, pages: usize) {
        self.pages = Some(pages);
    }
}

#[derive(Clone, Debug)]
pub enum CpuCacheStat {
    Ready,
    Alloc,
    Dealloc,
    Insufficient, // FreeList is empty or no aligned object is available
    Overranged,   // FreeList overranged
    Oversized,    // CpuCache oversized
    Scavenge,
}

#[derive(Clone, Debug)]
pub struct CpuCacheReg {
    idx: Option<usize>,
    align: Option<usize>,
    ptr: Option<*mut u8>,
}

impl CpuCacheReg {
    pub const fn new() -> Self {
        Self {
            idx: None,
            align: None,
            ptr: None,
        }
    }

    pub fn from(idx: Option<usize>, align: Option<usize>, ptr: Option<*mut u8>) -> Self {
        Self { idx, align, ptr }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn idx(&self) -> Option<usize> {
        self.idx
    }

    pub fn align(&self) -> Option<usize> {
        self.align
    }

    pub fn ptr(&self) -> Option<*mut u8> {
        self.ptr
    }
}

#[derive(Clone, Debug)]
pub enum TransferCacheStat {
    Ready,
    Alloc,
    Dealloc,
    Empty,
    Lack,
    Oversized,
    Scavenge,
}

#[derive(Clone, Debug)]
pub enum CentralFreeListStat {
    Ready,
    Alloc,
    Dealloc,
    Empty,
    Overranged,
    Scavenge,
}

#[derive(Clone, Debug)]
pub enum CentralFreeListMetaStat {
    Ready,
    Alloc,
    Empty,
}

#[derive(Clone, Debug)]
pub enum PageHeapStat {
    Ready,
    Alloc,
    Dealloc,
    Insufficient,
    Uncovered,
}

#[derive(Clone, Debug)]
pub struct PageHeapReg {
    ptr: Option<*mut usize>,
    pages: Option<usize>,
}

impl PageHeapReg {
    pub const fn new() -> Self {
        Self {
            ptr: None,
            pages: None,
        }
    }

    pub fn from(ptr: Option<*mut usize>, pages: Option<usize>) -> Self {
        Self { ptr, pages }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn ptr(&self) -> Option<*mut usize> {
        self.ptr
    }

    pub fn pages(&self) -> Option<usize> {
        self.pages
    }
}
