//! Provide the intrusive LinkedList

use buddy_system_allocator::linked_list::LinkedList;

#[derive(Clone, Copy)]
pub struct BoundedList {
    list: LinkedList,
    len: usize,
    max_len: usize,
    color: usize,
    base: usize,
    bound: usize,
}

impl BoundedList {
    pub const fn new() -> Self {
        Self {
            list: LinkedList::new(),
            len: 0,
            max_len: 0,
            color: 0,
            base: 0,
            bound: 0,
        }
    }

    pub fn init(&mut self, base: usize, bound: usize) {
        assert_eq!(base < bound, true);

        self.base = base;
        self.bound = bound;
    }

    pub fn reset(&mut self) {
        self.len = 0;
        self.max_len = 0;
        self.color = 0;
        self.base = 0;
        self.bound = 0;
    }

    pub fn push(&mut self, item: *mut usize) {
        let list = &mut self.list;
        // TODO: SAFETY
        unsafe { list.push(item) };

        self.len += 1;
        assert_eq!(self.len > self.max_len, false);
    }

    pub fn pop(&mut self) -> Option<*mut usize> {
        match self.list.pop() {
            None => None,
            Some(ptr) => {
                self.len -= 1;
                self.color += 1;
                Some(ptr)
            }
        }
    }

    pub fn set_max_len(&mut self, max_len: usize) {
        self.max_len = max_len;
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn is_full(&self) -> bool {
        assert_eq!(self.len <= self.max_len, true);

        self.len == self.max_len
    }

    pub fn color(&self) -> usize {
        self.color
    }

    pub fn unused(&self) -> bool {
        self.is_empty() && self.color == 0
    }

    pub fn base(&self) -> usize {
        assert_eq!(!self.unused(), true);

        self.base
    }

    pub fn infra_metas(&self, addr: usize) -> bool {
        addr >= self.base && addr < self.bound
    }
}
