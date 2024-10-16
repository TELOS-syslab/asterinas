//! Provide the intrusive LinkedList

use buddy_system_allocator::linked_list::LinkedList;

pub struct ElasticList {
    list: LinkedList,
    len: usize,
    max_len: usize,
    color: usize,
    overrange: usize,
    max_overrange: usize,
}

impl ElasticList {
    pub const fn new() -> Self {
        Self {
            list: LinkedList::new(),
            len: 0,
            max_len: 0,
            color: 0,
            overrange: 0,
            max_overrange: 0,
        }
    }

    pub fn init(&mut self, max_len: usize, max_overrange: usize) {
        self.max_len = max_len;
        self.max_overrange = max_overrange;
    }

    pub fn push(&mut self, item: *mut usize) {
        let list = &mut self.list;

        // TODO: SAFETY
        unsafe { list.push(item) };

        self.len += 1;
        if self.len > self.max_len {
            self.overrange += 1;
        }
    }

    pub fn pop(&mut self) -> Option<*mut usize> {
        match self.list.pop() {
            None => None,
            Some(ptr) => {
                self.len -= 1;
                Some(ptr)
            }
        }
    }

    pub fn pop_aligned(&mut self, align: usize) -> Option<*mut usize> {
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

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn color(&self) -> usize {
        self.color
    }

    pub fn overranged(&self) -> bool {
        self.overrange > self.max_overrange
    }

    pub fn reset(&mut self) {
        self.color = 0;
        self.overrange = 0;
    }
}
