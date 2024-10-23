//! Provide the intrusive LinkedList

use core::marker::PhantomData;

use buddy_system_allocator::linked_list::LinkedList;

pub struct BoundedLists {
    head: Option<*mut BoundedList>,
}

impl BoundedLists {
    pub const fn new() -> Self {
        Self { head: None }
    }

    pub fn push(&mut self, bounded_list: *mut BoundedList) {
        // TODO: SAFETY
        unsafe { (*bounded_list).next = self.head };
        self.head = Some(bounded_list);
    }

    pub fn iter(&self) -> Iter {
        Iter {
            next: self.head,
            _marker: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> IterMut {
        IterMut {
            next: self.head,
            _marker: PhantomData,
        }
    }

    pub fn lack(&self) -> bool {
        let mut count = 0usize;
        for bounded_list in self.iter() {
            if bounded_list.unused() {
                count += 1;
            }
        }
        count == 0
    }
}

pub struct Iter<'a> {
    next: Option<*mut BoundedList>,
    _marker: PhantomData<&'a BoundedList>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a BoundedList;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next.is_none() {
            None
        } else {
            let list = unsafe { &*self.next.unwrap() };
            self.next = list.next;
            Some(list)
        }
    }
}

pub struct IterMut<'a> {
    next: Option<*mut BoundedList>,
    _marker: PhantomData<&'a mut BoundedList>,
}

impl<'a> Iterator for IterMut<'a> {
    type Item = &'a mut BoundedList;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next.is_none() {
            None
        } else {
            let list = unsafe { &mut *self.next.unwrap() };
            self.next = list.next;
            Some(list)
        }
    }
}

#[derive(Debug)]
pub struct BoundedList {
    list: LinkedList,
    len: usize,
    max_len: usize,
    color: usize,
    base: usize,
    bound: usize,
    next: Option<*mut BoundedList>,
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
            next: None,
        }
    }

    pub fn init(&mut self, base: usize, bound: usize) {
        assert_eq!(base < bound, true);
        self.base = base;
        self.bound = bound;
        self.max_len = self.len;
    }

    pub fn reset(&mut self) {
        // assert!(self.list.is_empty() && self.next.is_none());
        self.list = LinkedList::new();
        self.len = 0;
        self.max_len = 0;
        self.color = 0;
        self.base = 0;
        self.bound = 0;
        self.next = None
    }

    pub fn push(&mut self, item: *mut usize) -> bool {
        let list = &mut self.list;
        // TODO: SAFETY
        unsafe { list.push(item) };
        self.len += 1;
        self.is_full()
    }

    pub fn pop(&mut self) -> Option<(*mut usize, bool)> {
        let flag = self.is_full();
        match self.list.pop() {
            None => None,
            Some(ptr) => {
                self.len -= 1;
                self.color += 1;
                Some((ptr, flag))
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

    pub fn len(&self) -> usize {
        self.len
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
