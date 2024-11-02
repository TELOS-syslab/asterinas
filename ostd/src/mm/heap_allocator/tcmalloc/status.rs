// SPDX-License-Identifier: MPL-2.0

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
