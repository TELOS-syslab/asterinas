// SPDX-License-Identifier: MPL-2.0

use core::alloc::Layout;

#[derive(Debug)]
pub enum FlowMod {
    Forward,
    Circle,
    Backward,
    Exit,
}

#[derive(Clone, Debug)]
pub enum MetaStat {
    Ready,
    /// layout
    Alloc(Layout),
    /// ptr, layout
    Dealloc(*mut u8, Layout),
    Finish,
    /// pages, layout
    Insufficient(usize, Layout),
    /// pages
    LargeSize(usize),
    /// ptr, layout
    Uncovered(*mut u8, Layout),
}

#[derive(Clone, Debug)]
pub enum CpuCacheStat {
    Ready,
    /// size_class_idx, alignment
    Alloc(usize, usize),
    /// size_class_idx, ptr
    Dealloc(usize, *mut u8),
    Finish,
    /// size_class_idx, alignment
    Insufficient(usize, usize), // FreeList is empty or no aligned object is available
    /// size_class_idx
    Overranged(usize), // FreeList overranged
    Oversized, // CpuCache oversized
    /// size_class_idx
    Scavenge(usize),
}

#[derive(Clone, Debug)]
pub enum TransferCacheStat {
    Ready,
    Alloc,
    Dealloc,
    Finish,
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
    Finish,
    Empty,
    Overranged,
    Scavenge,
}

#[derive(Clone, Debug)]
pub enum CentralFreeListMetaStat {
    Ready,
    Alloc,
    Finish,
    Empty,
}

#[derive(Clone, Debug)]
pub enum PageHeapStat {
    Ready,
    /// pages
    Alloc(usize),
    /// pages, ptr
    Dealloc(usize, *mut usize),
    Finish,
    /// pages
    Insufficient(usize),
    Uncovered,
}
