// SPDX-License-Identifier: MPL-2.0

use core::alloc::Layout;

use super::common::K_BASE_NUMBER_CLASSES;

const MAX_NUM_TO_MOVE: usize = 32;

pub struct Span {
    len: usize,
    start: usize,
}

impl Span {
    pub fn new(len: usize, start: usize) -> Self {
        Self { len, start }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn start(&self) -> usize {
        self.start
    }
}

pub struct TransferBatch {
    _padding: [*mut usize; MAX_NUM_TO_MOVE],
    len: usize,
    max_len: usize,
}

impl TransferBatch {
    pub fn new(max_len: usize) -> Self {
        let max_len = core::cmp::min(max_len, MAX_NUM_TO_MOVE);
        Self {
            _padding: [core::ptr::null_mut(); MAX_NUM_TO_MOVE],
            len: 0,
            max_len,
        }
    }

    /// Return `true` if full
    pub fn push(&mut self, ptr: *mut usize) -> bool {
        self._padding[self.len] = ptr;
        self.len += 1;
        self.len >= self.max_len
    }

    pub fn pop(&mut self) -> Option<*mut usize> {
        if self.len > 0 {
            self.len -= 1;
            Some(self._padding[self.len])
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// Precomputed size class parameters
pub struct SizeClassInfo {
    // Max size storable in that class
    size: usize,

    // Number of pages to allocate at a time
    pages: usize,

    // Number of items to move between per-CPU FreeList and CentralFreeList in a shot
    num_to_move: usize,

    // Max per-CPU slab capacity for the default 256KB slab size
    max_capacity: usize,
}

impl SizeClassInfo {
    const fn new(size: usize, pages: usize, num_to_move: usize, max_capacity: usize) -> Self {
        SizeClassInfo {
            size,
            pages,
            num_to_move,
            max_capacity,
        }
    }
}

struct SizeClasses([SizeClassInfo; K_BASE_NUMBER_CLASSES]);

impl SizeClasses {
    /// Return the `index` of size class satisfying the `layout`
    ///
    /// Return `None` for large sizes
    fn match_size_class(&self, layout: Layout) -> Option<usize> {
        let size = layout.size();

        for (index, size_class_info) in self.0.iter().enumerate() {
            let size_class_size = size_class_info.size;
            if size_class_size >= size {
                return Some(index);
            }
        }

        None
    }

    fn get_size(&self, size_class_idx: usize) -> Option<usize> {
        if size_class_idx < K_BASE_NUMBER_CLASSES {
            Some(self.0[size_class_idx].size)
        } else {
            None
        }
    }

    fn get_pages(&self, size_class_idx: usize) -> Option<usize> {
        if size_class_idx < K_BASE_NUMBER_CLASSES {
            Some(self.0[size_class_idx].pages)
        } else {
            None
        }
    }

    fn get_num_to_move(&self, size_class_idx: usize) -> Option<usize> {
        if size_class_idx < K_BASE_NUMBER_CLASSES {
            Some(self.0[size_class_idx].num_to_move)
        } else {
            None
        }
    }

    fn get_capacity(&self, size_class_idx: usize) -> Option<usize> {
        if size_class_idx < K_BASE_NUMBER_CLASSES {
            Some(self.0[size_class_idx].max_capacity)
        } else {
            None
        }
    }

    #[cfg(feature = "page_shift_12")]
    const fn new() -> Self {
        Self([
            //                                                                                                   |    waste     |
            //                           bytes        pages             batch                 cap    class  objs |fixed sampling|    inc
            //  SizeClassInfo::new(          0,           0,                0,                  0),  //  0     0  0.00%    0.00%   0.00%
            SizeClassInfo::new(8, 1, 32, 5811), //  0   512  1.16%    0.92%   0.00%
            SizeClassInfo::new(16, 1, 32, 5811), //  1   256  1.16%    0.92% 100.00%
            SizeClassInfo::new(32, 1, 32, 5811), //  2   128  1.16%    0.92% 100.00%
            SizeClassInfo::new(64, 1, 32, 5811), //  3    64  1.16%    0.92% 100.00%
            SizeClassInfo::new(80, 1, 32, 5811), //  4    51  1.54%    0.92%  25.00%
            SizeClassInfo::new(96, 1, 32, 3615), //  5    42  2.70%    0.92%  20.00%
            SizeClassInfo::new(112, 1, 32, 2468), //  6    36  2.70%    0.92%  16.67%
            SizeClassInfo::new(128, 1, 32, 2667), //  7    32  1.16%    0.92%  14.29%
            SizeClassInfo::new(144, 1, 32, 2037), //  8    28  2.70%    0.92%  12.50%
            SizeClassInfo::new(160, 1, 32, 2017), //  9    25  3.47%    0.92%  11.11%
            SizeClassInfo::new(176, 1, 32, 973), // 10    23  2.32%    0.92%  10.00%
            SizeClassInfo::new(192, 1, 32, 999), // 11    21  2.70%    0.92%   9.09%
            SizeClassInfo::new(208, 1, 32, 885), // 12    19  4.63%    0.92%   8.33%
            SizeClassInfo::new(224, 1, 32, 820), // 13    18  2.70%    0.92%   7.69%
            SizeClassInfo::new(240, 1, 32, 800), // 14    17  1.54%    0.92%   7.14%
            SizeClassInfo::new(256, 1, 32, 1226), // 15    16  1.16%    0.92%   6.67%
            SizeClassInfo::new(272, 1, 32, 582), // 16    15  1.54%    0.92%   6.25%
            SizeClassInfo::new(288, 1, 32, 502), // 17    14  2.70%    0.92%   5.88%
            SizeClassInfo::new(304, 1, 32, 460), // 18    13  4.63%    0.92%   5.56%
            SizeClassInfo::new(336, 1, 32, 854), // 19    12  2.70%    0.92%  10.53%
            SizeClassInfo::new(368, 1, 32, 485), // 20    11  2.32%    0.92%   9.52%
            SizeClassInfo::new(448, 1, 32, 559), // 21     9  2.70%    0.92%  21.74%
            SizeClassInfo::new(512, 1, 32, 1370), // 22     8  1.16%    0.92%  14.29%
            SizeClassInfo::new(576, 2, 32, 684), // 23    14  2.14%    0.92%  12.50%
            SizeClassInfo::new(640, 2, 32, 403), // 24    12  6.80%    0.92%  11.11%
            SizeClassInfo::new(704, 2, 32, 389), // 25    11  6.02%    0.92%  10.00%
            SizeClassInfo::new(768, 2, 32, 497), // 26    10  6.80%    0.93%   9.09%
            SizeClassInfo::new(896, 2, 32, 721), // 27     9  2.14%    0.92%  16.67%
            SizeClassInfo::new(1024, 2, 32, 3115), // 28     8  0.58%    0.92%  14.29%
            SizeClassInfo::new(1152, 3, 32, 451), // 29    10  6.61%    0.93%  12.50%
            SizeClassInfo::new(1280, 3, 32, 372), // 30     9  6.61%    0.93%  11.11%
            SizeClassInfo::new(1536, 3, 32, 420), // 31     8  0.39%    0.92%  20.00%
            SizeClassInfo::new(1792, 4, 32, 406), // 32     9  1.85%    0.92%  16.67%
            SizeClassInfo::new(2048, 4, 32, 562), // 33     8  0.29%    0.92%  14.29%
            SizeClassInfo::new(2304, 4, 28, 380), // 34     7  1.85%    0.92%  12.50%
            SizeClassInfo::new(2688, 4, 24, 394), // 35     6  1.85%    0.93%  16.67%
            SizeClassInfo::new(3200, 4, 20, 389), // 36     5  2.63%    0.93%  19.05%
            SizeClassInfo::new(3584, 7, 18, 409), // 37     8  0.17%    0.92%  12.00%
            SizeClassInfo::new(4096, 4, 16, 1430), // 38     4  0.29%    0.92%  14.29%
            SizeClassInfo::new(4736, 5, 13, 440), // 39     4  7.72%    1.77%  15.62%
            SizeClassInfo::new(5376, 4, 12, 361), // 40     3  1.85%    1.72%  13.51%
            SizeClassInfo::new(6144, 3, 10, 369), // 41     2  0.39%    1.70%  14.29%
            SizeClassInfo::new(7168, 7, 9, 377), // 42     4  0.17%    1.70%  16.67%
            SizeClassInfo::new(8192, 4, 8, 505), // 43     2  0.29%    1.70%  14.29%
        ])
    }
}

static SIZE_CLASSES: SizeClasses = SizeClasses::new();

pub fn match_size_class(layout: Layout) -> Option<usize> {
    SIZE_CLASSES.match_size_class(layout)
}

pub fn get_size(size_class_idx: usize) -> Option<usize> {
    SIZE_CLASSES.get_size(size_class_idx)
}

pub fn get_pages(size_class_idx: usize) -> Option<usize> {
    SIZE_CLASSES.get_pages(size_class_idx)
}

pub fn get_num_to_move(size_class_idx: usize) -> Option<usize> {
    SIZE_CLASSES.get_num_to_move(size_class_idx)
}

pub fn get_capacity(size_class_idx: usize) -> Option<usize> {
    SIZE_CLASSES.get_capacity(size_class_idx)
}
