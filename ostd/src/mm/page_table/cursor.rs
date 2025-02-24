// SPDX-License-Identifier: MPL-2.0

//! The page table cursor for mapping and querying over the page table.
//!
//! ## The page table lock protocol
//!
//! We provide a fine-grained transactional lock protocol to allow concurrent
//! accesses to the page table. [`CursorMut::new`] will lock a range in the
//! virtual space and all the operations on the range with the cursor will be
//! atomic as a transaction.
//!
//! [`CursorMut::new`] accepts an address range, which indicates the page table
//! entries that may be visited by this cursor. Then, [`CursorMut::new`] finds
//! an intermediate page table (not necessarily the last-level or the top
//! -level) which represents an address range that contains the whole specified
//! address range. Then it locks all the nodes in the sub-tree rooted at the
//! intermediate page table node. The cursor will only be able to access the
//! page table entries in the locked range. Upon destruction, the cursor will
//! release the locks in the reverse order of acquisition.

use core::{
    any::TypeId, marker::PhantomData, mem::ManuallyDrop, ops::Range, sync::atomic::Ordering,
};

use align_ext::AlignExt;

use super::{
    page_size, pte_index, Child, Entry, KernelMode, MapTrackingStatus, PageTable,
    PageTableEntryTrait, PageTableError, PageTableMode, PageTableNode, PagingConstsTrait,
    PagingLevel, UserMode,
};
use crate::{
    mm::{
        frame::{meta::AnyFrameMeta, Frame},
        kspace::should_map_as_tracked,
        nr_subpage_per_huge, paddr_to_vaddr,
        vm_space::Token,
        Paddr, PageProperty, Vaddr,
    },
    trap::{self, DisabledLocalIrqGuard},
};

#[derive(Clone, Debug)]
pub enum PageTableItem {
    NotMapped {
        va: Vaddr,
        len: usize,
    },
    Mapped {
        va: Vaddr,
        page: Frame<dyn AnyFrameMeta>,
        prop: PageProperty,
    },
    ChildPageTable {
        va: Vaddr,
        len: usize,
        pt: Frame<dyn AnyFrameMeta>,
    },
    MappedUntracked {
        va: Vaddr,
        pa: Paddr,
        len: usize,
        prop: PageProperty,
    },
    /// The current slot is marked to be reserved.
    Marked {
        /// The virtual address of the slot.
        va: Vaddr,
        /// The length of the slot.
        len: usize,
        /// A user-provided token.
        token: Token,
    },
}

/// The cursor for traversal over the page table.
///
/// A slot is a PTE at any levels, which correspond to a certain virtual
/// memory range sized by the "page size" of the current level.
///
/// A cursor is able to move to the next slot, to read page properties,
/// and even to jump to a virtual address directly. We use a guard stack to
/// simulate the recursion, and adpot a page table locking protocol to
/// provide concurrency.
#[derive(Debug)]
pub struct Cursor<'a, M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> {
    /// The current node path of the cursor.
    path: [Option<ManuallyDrop<PageTableNode<E, C>>>; MAX_NR_LEVELS],
    /// The level of the page table that the cursor points to.
    level: PagingLevel,
    /// The level of the sub-tree root which has locked PTEs.
    root_level: PagingLevel,
    /// The current virtual address that the cursor points to.
    va: Vaddr,
    /// The virtual address range that is locked.
    barrier_va: Range<Vaddr>,
    #[expect(dead_code)]
    irq_guard: DisabledLocalIrqGuard,
    _phantom: PhantomData<&'a PageTable<M, E, C>>,
}

/// The maximum value of `PagingConstsTrait::NR_LEVELS`.
const MAX_NR_LEVELS: usize = 4;

/// Acquires the locks for the given range in the sub-tree rooted at the node.
///
/// `cur_node_va` must be the virtual address of the `cur_node`. The `va_range`
/// must be within the range of the `cur_node`. The range must not be empty.
///
/// The function will forget all the [`PageTableNode`] objects in the sub-tree
/// with [`PageTableNode::into_raw_paddr`].
fn dfs_acquire_lock<E: PageTableEntryTrait, C: PagingConstsTrait>(
    cur_node: &PageTableNode<E, C>,
    cur_node_va: Vaddr,
    va_range: &Range<Vaddr>,
) {
    let cur_level = cur_node.level();
    let idx_range = get_idx_range::<C>(cur_level, cur_node_va, va_range);
    for i in idx_range {
        unsafe { cur_node.lock(i) };

        let child = cur_node.entry(i);
        match child.to_ref() {
            Child::PageTableRef(pt) => {
                debug_assert!(cur_level > 1);
                let child_node_va = cur_node_va + i * page_size::<C>(cur_level);
                dfs_acquire_lock(&pt, child_node_va, va_range);
            }
            Child::None
            | Child::Frame(_, _)
            | Child::Untracked(_, _, _)
            | Child::Token(_)
            | Child::PageTable(_) => {}
        }
    }
}

/// Releases the locks for the given range in the sub-tree rooted at the node.
///
/// # Safety
///
/// The function must be called only once, after the locks are acquired by
/// [`dfs_acquire_lock`] with the same arguments.
unsafe fn dfs_release_lock<E: PageTableEntryTrait, C: PagingConstsTrait>(
    cur_node: &PageTableNode<E, C>,
    cur_node_va: Vaddr,
    va_range: &Range<Vaddr>,
) {
    let cur_level = cur_node.level();
    let idx_range = get_idx_range::<C>(cur_level, cur_node_va, va_range);
    for i in idx_range.rev() {
        let child = cur_node.entry(i);
        match child.to_ref() {
            Child::PageTableRef(pt) => {
                debug_assert!(cur_level > 1);
                let child_node_va = cur_node_va + i * page_size::<C>(cur_level);
                dfs_release_lock(&pt, child_node_va, va_range);
            }
            Child::None
            | Child::Frame(_, _)
            | Child::Untracked(_, _, _)
            | Child::Token(_)
            | Child::PageTable(_) => {}
        }
        unsafe { cur_node.unlock(i) };
    }
}

/// Gets the index range of a node corresponding to a virtual address range.
///
/// If the `va_range` is larger than the node's virtual address range, the
/// exceeding part will be ignored.
fn get_idx_range<C: PagingConstsTrait>(
    node_level: PagingLevel,
    node_va: Vaddr,
    va_range: &Range<Vaddr>,
) -> Range<usize> {
    let va_start = va_range.start.max(node_va);
    let va_end = va_range.end.min(node_va + page_size::<C>(node_level + 1));

    let start_idx = (va_start - node_va) / page_size::<C>(node_level);
    let end_idx = (va_end - node_va).div_ceil(page_size::<C>(node_level));

    debug_assert!(start_idx < end_idx);
    debug_assert!(end_idx <= nr_subpage_per_huge::<C>());

    start_idx..end_idx
}

impl<M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> Drop for Cursor<'_, M, E, C> {
    fn drop(&mut self) {
        let guard_node = self.path[self.root_level as usize - 1].take().unwrap();
        let cur_node_va = self
            .barrier_va
            .start
            .align_down(page_size::<C>(self.root_level + 1));
        unsafe { dfs_release_lock(&guard_node, cur_node_va, &self.barrier_va) };
    }
}

impl<'a, M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> Cursor<'a, M, E, C> {
    /// Creates a cursor claiming the read access for the given range.
    ///
    /// The cursor created will only be able to query or jump within the given
    /// range. Out-of-bound accesses will result in panics or errors as return values,
    /// depending on the access method.
    ///
    /// Note that this function does not ensure exclusive access to the claimed
    /// virtual address range. The accesses using this cursor may block or fail.
    pub fn new(pt: &'a PageTable<M, E, C>, va: &Range<Vaddr>) -> Result<Self, PageTableError> {
        if !M::covers(va) || va.is_empty() {
            return Err(PageTableError::InvalidVaddrRange(va.start, va.end));
        }
        if va.start % C::BASE_PAGE_SIZE != 0 || va.end % C::BASE_PAGE_SIZE != 0 {
            return Err(PageTableError::UnalignedVaddr);
        }

        const { assert!(C::NR_LEVELS as usize <= MAX_NR_LEVELS) };

        let mut cursor = Self {
            path: core::array::from_fn(|_| None),
            level: C::NR_LEVELS,
            root_level: C::NR_LEVELS,
            va: va.start,
            barrier_va: va.clone(),
            irq_guard: trap::disable_local(),
            _phantom: PhantomData,
        };

        let mut cur_pt_addr = pt.root.paddr();

        // Go down and find the sub-tree that contains the virtual address range.
        loop {
            let start_idx = pte_index::<C>(va.start, cursor.level);
            let level_too_high = {
                let end_idx = pte_index::<C>(va.end - 1, cursor.level);
                cursor.level > 1 && start_idx == end_idx
            };
            if !level_too_high {
                break;
            }

            let cur_pt_ptr = paddr_to_vaddr(cur_pt_addr) as *mut E;
            // SAFETY:
            // - The page table node is alive because (1) the root node is alive and (2) all child nodes cannot
            //   be recycled if there are cursors.
            // - The index is inside the bound, so the page table entry is valid.
            // - All page table entries are aligned and accessed with atomic operations only.
            let cur_pte = unsafe { super::load_pte(cur_pt_ptr.add(start_idx), Ordering::Acquire) };
            if cur_pte.is_present() {
                if cur_pte.is_last(cursor.level) {
                    break;
                } else {
                    cur_pt_addr = cur_pte.paddr();
                }
            } else {
                // In either marked case or not mapped case, we should lock
                // and allocate a new page table node.
                // SAFETY: The address and level corresponds to a child converted into
                // a PTE and we clone it to get a new handle to the node.
                let node = ManuallyDrop::new(unsafe {
                    PageTableNode::<E, C>::from_raw_paddr(cur_pt_addr)
                });

                unsafe { node.lock(start_idx) };

                let cur_entry = node.entry(start_idx);
                if cur_entry.is_none() {
                    let is_tracked = if should_map_as_tracked(va.start) {
                        MapTrackingStatus::Tracked
                    } else {
                        MapTrackingStatus::Untracked
                    };
                    let pt = PageTableNode::<E, C>::alloc(cursor.level - 1, is_tracked, 0..0);
                    cur_pt_addr = pt.paddr();
                    unsafe { node.unlock(start_idx) };
                    let _ = unsafe { cur_entry.replace(Child::PageTable(pt)) };
                } else if cur_entry.is_node() {
                    let Child::PageTableRef(pt) = cur_entry.to_ref() else {
                        unreachable!();
                    };
                    cur_pt_addr = pt.paddr();

                    unsafe { node.unlock(start_idx) };
                } else if let Some(split_child) = unsafe { cur_entry.split_if_huge_token(0..0) } {
                    cur_pt_addr = split_child.paddr();

                    unsafe { node.unlock(start_idx) };
                } else {
                    unsafe { node.unlock(start_idx) };
                    break;
                }
            }
            cursor.level -= 1;
        }

        // SAFETY: The address and level corresponds to a child converted into
        // a PTE and we clone it to get a new handle to the node.
        let node = ManuallyDrop::new(unsafe { PageTableNode::<E, C>::from_raw_paddr(cur_pt_addr) });

        dfs_acquire_lock(
            &node,
            va.start.align_down(page_size::<C>(cursor.level + 1)),
            va,
        );

        cursor.path[cursor.level as usize - 1] = Some(node);
        cursor.root_level = cursor.level;

        Ok(cursor)
    }

    /// Gets the information of the current slot.
    pub fn query(&mut self) -> Result<PageTableItem, PageTableError> {
        if self.va >= self.barrier_va.end {
            return Err(PageTableError::InvalidVaddr(self.va));
        }

        loop {
            let level = self.level;
            let va = self.va;

            match self.cur_entry().to_ref() {
                Child::PageTableRef(pt) => {
                    self.push_level(pt);
                    continue;
                }
                Child::PageTable(_) => {
                    unreachable!();
                }
                Child::None => {
                    return Ok(PageTableItem::NotMapped {
                        va,
                        len: page_size::<C>(level),
                    });
                }
                Child::Frame(page, prop) => {
                    return Ok(PageTableItem::Mapped { va, page, prop });
                }
                Child::Untracked(pa, plevel, prop) => {
                    debug_assert_eq!(plevel, level);
                    return Ok(PageTableItem::MappedUntracked {
                        va,
                        pa,
                        len: page_size::<C>(level),
                        prop,
                    });
                }
                Child::Token(token) => {
                    return Ok(PageTableItem::Marked {
                        va,
                        len: page_size::<C>(level),
                        token,
                    });
                }
            }
        }
    }

    /// Traverses forward in the current level to the next PTE.
    ///
    /// If reached the end of a page table node, it leads itself up to the next page of the parent
    /// page if possible.
    pub(in crate::mm) fn move_forward(&mut self) {
        let page_size = page_size::<C>(self.level);
        let next_va = self.va.align_down(page_size) + page_size;
        while self.level < self.root_level && pte_index::<C>(next_va, self.level) == 0 {
            self.pop_level();
        }
        self.va = next_va;
    }

    /// Jumps to the given virtual address.
    /// If the target address is out of the range, this method will return `Err`.
    ///
    /// # Panics
    ///
    /// This method panics if the address has bad alignment.
    pub fn jump(&mut self, va: Vaddr) -> Result<(), PageTableError> {
        assert!(va % C::BASE_PAGE_SIZE == 0);
        if !self.barrier_va.contains(&va) {
            return Err(PageTableError::InvalidVaddr(va));
        }

        loop {
            let cur_node_start = self.va & !(page_size::<C>(self.level + 1) - 1);
            let cur_node_end = cur_node_start + page_size::<C>(self.level + 1);
            // If the address is within the current node, we can jump directly.
            if cur_node_start <= va && va < cur_node_end {
                self.va = va;
                return Ok(());
            }

            // There is a corner case that the cursor is depleted, sitting at the start of the
            // next node but the next node is not locked because the parent is not locked.
            if self.va >= self.barrier_va.end && self.level == self.root_level {
                self.va = va;
                return Ok(());
            }

            debug_assert!(self.level < self.root_level);
            self.pop_level();
        }
    }

    pub fn virt_addr(&self) -> Vaddr {
        self.va
    }

    /// Goes up a level.
    fn pop_level(&mut self) {
        let _ = self.path[self.level as usize - 1]
            .take()
            .expect("Popping a level not on the path");
        self.level += 1;
    }

    /// Goes down a level to a child page table.
    fn push_level(&mut self, child_pt: ManuallyDrop<PageTableNode<E, C>>) {
        self.level -= 1;
        debug_assert_eq!(self.level, child_pt.level());
        self.path[self.level as usize - 1] = Some(child_pt);
    }

    fn should_map_as_tracked(&self) -> bool {
        (TypeId::of::<M>() == TypeId::of::<KernelMode>()
            || TypeId::of::<M>() == TypeId::of::<UserMode>())
            && should_map_as_tracked(self.va)
    }

    fn cur_entry(&self) -> Entry<'_, E, C> {
        let node = self.path[self.level as usize - 1].as_ref().unwrap();
        node.entry(pte_index::<C>(self.va, self.level))
    }
}

impl<M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> Iterator
    for Cursor<'_, M, E, C>
{
    type Item = PageTableItem;

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.query();
        if result.is_ok() {
            self.move_forward();
        }
        result.ok()
    }
}

/// The cursor of a page table that is capable of map, unmap or protect pages.
///
/// Also, it has all the capabilities of a [`Cursor`]. A virtual address range
/// in a page table can only be accessed by one cursor whether it is mutable or not.
#[derive(Debug)]
pub struct CursorMut<'a, M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait>(
    Cursor<'a, M, E, C>,
);

impl<'a, M: PageTableMode, E: PageTableEntryTrait, C: PagingConstsTrait> CursorMut<'a, M, E, C> {
    /// Creates a cursor claiming the write access for the given range.
    ///
    /// The cursor created will only be able to map, query or jump within the given
    /// range. Out-of-bound accesses will result in panics or errors as return values,
    /// depending on the access method.
    ///
    /// Note that this function, the same as [`Cursor::new`], does not ensure exclusive
    /// access to the claimed virtual address range. The accesses using this cursor may
    /// block or fail.
    pub(super) fn new(
        pt: &'a PageTable<M, E, C>,
        va: &Range<Vaddr>,
    ) -> Result<Self, PageTableError> {
        Cursor::new(pt, va).map(|inner| Self(inner))
    }

    /// Jumps to the given virtual address.
    ///
    /// This is the same as [`Cursor::jump`].
    ///
    /// # Panics
    ///
    /// This method panics if the address is out of the range where the cursor is required to operate,
    /// or has bad alignment.
    pub fn jump(&mut self, va: Vaddr) -> Result<(), PageTableError> {
        self.0.jump(va)
    }

    /// Gets the current virtual address.
    pub fn virt_addr(&self) -> Vaddr {
        self.0.virt_addr()
    }

    /// Gets the information of the current slot.
    pub fn query(&mut self) -> Result<PageTableItem, PageTableError> {
        self.0.query()
    }

    /// Maps the range starting from the current address to a [`Frame<dyn AnyFrameMeta>`].
    ///
    /// It returns the previously mapped [`Frame<dyn AnyFrameMeta>`] if that exists.
    ///
    /// # Panics
    ///
    /// This function will panic if
    ///  - the virtual address range to be mapped is out of the range;
    ///  - the alignment of the page is not satisfied by the virtual address;
    ///  - it is already mapped to a huge page while the caller wants to map a smaller one.
    ///
    /// # Safety
    ///
    /// The caller should ensure that the virtual range being mapped does
    /// not affect kernel's memory safety.
    pub unsafe fn map(
        &mut self,
        page: Frame<dyn AnyFrameMeta>,
        prop: PageProperty,
    ) -> Option<Frame<dyn AnyFrameMeta>> {
        let end = self.0.va + page.size();
        assert!(end <= self.0.barrier_va.end);

        // Go down if not applicable.
        while self.0.level > C::HIGHEST_TRANSLATION_LEVEL
            || self.0.va % page_size::<C>(self.0.level) != 0
            || self.0.va + page_size::<C>(self.0.level) > end
        {
            debug_assert!(self.0.should_map_as_tracked());
            let cur_level = self.0.level;
            let child_range = self.get_child_lock_range();
            let cur_entry = self.0.cur_entry();
            match cur_entry.to_ref() {
                Child::PageTableRef(pt) => {
                    self.0.push_level(pt);
                }
                Child::PageTable(_) => {
                    unreachable!();
                }
                Child::None => {
                    let pt = PageTableNode::<E, C>::alloc(
                        cur_level - 1,
                        MapTrackingStatus::Tracked,
                        child_range,
                    );
                    let pt_ref = unsafe { pt.get_manual_ref() };
                    let _ = cur_entry.replace(Child::PageTable(pt));
                    self.0.push_level(pt_ref);
                }
                Child::Frame(_, _) => {
                    panic!("Mapping a smaller page in an already mapped huge page");
                }
                Child::Untracked(_, _, _) => {
                    panic!("Mapping a tracked page in an untracked range");
                }
                Child::Token(_) => {
                    let split_child = cur_entry.split_if_huge_token(child_range).unwrap();
                    self.0.push_level(split_child);
                }
            }
            continue;
        }
        debug_assert_eq!(self.0.level, page.level());

        // Map the current page.
        let old = self.0.cur_entry().replace(Child::Frame(page, prop));
        self.0.move_forward();

        match old {
            Child::Frame(old_page, _) => Some(old_page),
            Child::None | Child::Token(_) => None,
            Child::PageTable(_) => {
                todo!("Dropping page table nodes while mapping requires TLB flush")
            }
            Child::Untracked(_, _, _) => panic!("Mapping a tracked page in an untracked range"),
            Child::PageTableRef(_) => unreachable!(),
        }
    }

    /// Maps the range starting from the current address to a physical address range.
    ///
    /// The function will map as more huge pages as possible, and it will split
    /// the huge pages into smaller pages if necessary. If the input range is
    /// large, the resulting mappings may look like this (if very huge pages
    /// supported):
    ///
    /// ```text
    /// start                                                             end
    ///   |----|----------------|--------------------------------|----|----|
    ///    base      huge                     very huge           base base
    ///    4KiB      2MiB                       1GiB              4KiB  4KiB
    /// ```
    ///
    /// In practice it is not suggested to use this method for safety and conciseness.
    ///
    /// # Panics
    ///
    /// This function will panic if
    ///  - the virtual address range to be mapped is out of the range.
    ///
    /// # Safety
    ///
    /// The caller should ensure that
    ///  - the range being mapped does not affect kernel's memory safety;
    ///  - the physical address to be mapped is valid and safe to use;
    ///  - it is allowed to map untracked pages in this virtual address range.
    pub unsafe fn map_pa(&mut self, pa: &Range<Paddr>, prop: PageProperty) {
        let end = self.0.va + pa.len();
        let mut pa = pa.start;
        assert!(end <= self.0.barrier_va.end);

        while self.0.va < end {
            // We ensure not mapping in reserved kernel shared tables or releasing it.
            // Although it may be an invariant for all architectures and will be optimized
            // out by the compiler since `C::NR_LEVELS - 1 > C::HIGHEST_TRANSLATION_LEVEL`.
            let is_kernel_shared_node =
                TypeId::of::<M>() == TypeId::of::<KernelMode>() && self.0.level >= C::NR_LEVELS - 1;
            if self.0.level > C::HIGHEST_TRANSLATION_LEVEL
                || is_kernel_shared_node
                || self.0.va % page_size::<C>(self.0.level) != 0
                || self.0.va + page_size::<C>(self.0.level) > end
                || pa % page_size::<C>(self.0.level) != 0
            {
                let cur_level = self.0.level;
                let child_range = self.get_child_lock_range();
                let cur_entry = self.0.cur_entry();
                match cur_entry.to_ref() {
                    Child::PageTableRef(pt) => {
                        self.0.push_level(pt);
                    }
                    Child::PageTable(_) => {
                        unreachable!();
                    }
                    Child::None => {
                        let pt = PageTableNode::<E, C>::alloc(
                            cur_level - 1,
                            MapTrackingStatus::Untracked,
                            child_range,
                        );
                        let pt_ref = unsafe { pt.get_manual_ref() };
                        let _ = cur_entry.replace(Child::PageTable(pt));
                        self.0.push_level(pt_ref);
                    }
                    Child::Frame(_, _) => {
                        panic!("Mapping a smaller page in an already mapped huge page");
                    }
                    Child::Untracked(_, _, _) => {
                        let split_child = cur_entry.split_if_untracked_huge(child_range).unwrap();
                        self.0.push_level(split_child);
                    }
                    Child::Token(_) => {
                        let split_child = cur_entry.split_if_huge_token(child_range).unwrap();
                        self.0.push_level(split_child);
                    }
                }
                continue;
            }

            // Map the current page.
            debug_assert!(!self.0.should_map_as_tracked());
            let level = self.0.level;
            let _ = self
                .0
                .cur_entry()
                .replace(Child::Untracked(pa, level, prop));

            // Move forward.
            pa += page_size::<C>(level);
            self.0.move_forward();
        }
    }

    /// Mark a virtual address range with a token.
    ///
    /// It can overwrite existing tokens, but it cannot overwrite existing
    /// mappings.
    ///
    /// # Panics
    ///
    /// It panics if
    ///  - the virtual address range already has mappings;
    ///  - the virtual address range is out of the range;
    ///  - the length is not aligned to the page size.
    pub fn mark(&mut self, len: usize, token: Token) {
        assert!(len % page_size::<C>(1) == 0);
        let end = self.0.va + len;
        assert!(end <= self.0.barrier_va.end);

        while self.0.va < end {
            if self.0.va % page_size::<C>(self.0.level) != 0
                || self.0.va + page_size::<C>(self.0.level) > end
            {
                let cur_level = self.0.level;
                let should_track_if_created = if self.0.should_map_as_tracked() {
                    MapTrackingStatus::Tracked
                } else {
                    MapTrackingStatus::Untracked
                };
                let child_range = self.get_child_lock_range();
                let cur_entry = self.0.cur_entry();
                match cur_entry.to_ref() {
                    Child::PageTableRef(pt) => {
                        self.0.push_level(pt);
                    }
                    Child::PageTable(_) => {
                        unreachable!();
                    }
                    Child::None => {
                        let pt = PageTableNode::<E, C>::alloc(
                            cur_level - 1,
                            should_track_if_created,
                            child_range,
                        );
                        let pt_ref = unsafe { pt.get_manual_ref() };
                        let _ = unsafe { cur_entry.replace(Child::PageTable(pt)) };
                        self.0.push_level(pt_ref);
                    }
                    Child::Frame(_, _) => {
                        panic!("Marking a smaller page in an already mapped huge page");
                    }
                    Child::Untracked(_, _, _) => {
                        panic!("Marking an already untracked mapped page");
                    }
                    Child::Token(_) => {
                        let split_child =
                            unsafe { cur_entry.split_if_huge_token(child_range).unwrap() };
                        self.0.push_level(split_child);
                    }
                }
                continue;
            }

            let cur_entry = self.0.cur_entry();
            match cur_entry.to_ref() {
                Child::PageTableRef(pt) => {
                    self.0.push_level(pt);
                    continue;
                }
                Child::PageTable(_) => {
                    unreachable!();
                }
                Child::None | Child::Token(_) => {} // Ok to proceed.
                Child::Frame(_, _) => {
                    panic!("Marking an already mapped huge page");
                }
                Child::Untracked(_, _, _) => {
                    panic!("Marking an already untracked mapped huge page");
                }
            }

            // Mark the current page.
            let _ = unsafe { cur_entry.replace(Child::Token(token)) };

            // Move forward.
            self.0.move_forward();
        }
    }

    /// Find and remove the first page in the cursor's following range.
    ///
    /// The range to be found in is the current virtual address with the
    /// provided length.
    ///
    /// The function stops and yields the page if it has actually removed a
    /// page, no matter if the following pages are also required to be unmapped.
    /// The returned page is the virtual page that existed before the removal
    /// but having just been unmapped.
    ///
    /// It also makes the cursor moves forward to the next page after the
    /// removed one, when an actual page is removed. If no mapped pages exist
    /// in the following range, the cursor will stop at the end of the range
    /// and return [`PageTableItem::NotMapped`].
    ///
    /// # Safety
    ///
    /// The caller should ensure that the range being unmapped does not affect
    /// kernel's memory safety.
    ///
    /// # Panics
    ///
    /// This function will panic if the end range covers a part of a huge page
    /// and the next page is that huge page.
    pub unsafe fn take_next(&mut self, len: usize) -> PageTableItem {
        let start = self.0.va;
        assert!(len % page_size::<C>(1) == 0);
        let end = start + len;
        assert!(end <= self.0.barrier_va.end);

        while self.0.va < end {
            let cur_va = self.0.va;
            let cur_level = self.0.level;
            let cur_entry = self.0.cur_entry();

            // Skip if it is already absent.
            if cur_entry.is_none() {
                if self.0.va + page_size::<C>(self.0.level) > end {
                    self.0.va = end;
                    break;
                }
                self.0.move_forward();
                continue;
            }

            // Go down if not applicable.
            if cur_va % page_size::<C>(cur_level) != 0 || cur_va + page_size::<C>(cur_level) > end {
                let child_range = self.get_child_lock_range();
                let child = cur_entry.to_ref();
                match child {
                    Child::PageTableRef(pt) => {
                        self.0.push_level(pt);
                    }
                    Child::PageTable(_) => {
                        unreachable!();
                    }
                    Child::None => {
                        unreachable!("Already checked");
                    }
                    Child::Frame(_, _) => {
                        panic!("Removing part of a huge page");
                    }
                    Child::Untracked(_, _, _) => {
                        let split_child = cur_entry.split_if_untracked_huge(child_range).unwrap();
                        self.0.push_level(split_child);
                    }
                    Child::Token(_) => {
                        let split_child = cur_entry.split_if_huge_token(child_range).unwrap();
                        self.0.push_level(split_child);
                    }
                }
                continue;
            }

            // Unmap the current page and return it.
            let old = cur_entry.replace(Child::None);

            self.0.move_forward();

            return match old {
                Child::Frame(page, prop) => PageTableItem::Mapped {
                    va: self.0.va,
                    page,
                    prop,
                },
                Child::Untracked(pa, level, prop) => {
                    debug_assert_eq!(level, self.0.level);
                    PageTableItem::MappedUntracked {
                        va: self.0.va,
                        pa,
                        len: page_size::<C>(level),
                        prop,
                    }
                }
                Child::Token(token) => PageTableItem::Marked {
                    va: self.0.va,
                    len: page_size::<C>(self.0.level),
                    token,
                },
                Child::PageTable(pt) => PageTableItem::ChildPageTable {
                    va: self.0.va,
                    len: page_size::<C>(self.0.level),
                    pt: pt.into_frame().into(),
                },
                Child::None | Child::PageTableRef(_) => unreachable!(),
            };
        }

        // If the loop exits, we did not find any mapped pages in the range.
        PageTableItem::NotMapped { va: start, len }
    }

    /// Applies the operation to the next slot of mapping within the range.
    ///
    /// The range to be found in is the current virtual address with the
    /// provided length.
    ///
    /// The function stops and yields the actually protected range if it has
    /// actually protected a page, no matter if the following pages are also
    /// required to be protected.
    ///
    /// It also makes the cursor moves forward to the next page after the
    /// protected one. If no mapped pages exist in the following range, the
    /// cursor will stop at the end of the range and return [`None`].
    ///
    /// # Safety
    ///
    /// The caller should ensure that the range being protected with the
    /// operation does not affect kernel's memory safety.
    ///
    /// # Panics
    ///
    /// This function will panic if:
    ///  - the range to be protected is out of the range where the cursor
    ///    is required to operate;
    ///  - the specified virtual address range only covers a part of a page.
    pub unsafe fn protect_next(
        &mut self,
        len: usize,
        prot_op: &mut impl FnMut(&mut PageProperty),
        token_op: &mut impl FnMut(&mut Token),
    ) -> Option<Range<Vaddr>> {
        let end = self.0.va + len;
        assert!(end <= self.0.barrier_va.end);

        while self.0.va < end {
            let cur_va = self.0.va;
            let cur_level = self.0.level;
            let mut cur_entry = self.0.cur_entry();

            // Skip if it is already absent.
            if cur_entry.is_none() {
                self.0.move_forward();
                continue;
            }

            // Go down if it's not a last entry.
            if cur_entry.is_node() {
                let Child::PageTableRef(pt) = cur_entry.to_ref() else {
                    unreachable!("Already checked");
                };
                self.0.push_level(pt);
                continue;
            }

            // Go down if the page size is too big and we are protecting part
            // of tokens or untracked huge pages.
            if cur_va % page_size::<C>(cur_level) != 0 || cur_va + page_size::<C>(cur_level) > end {
                let child_range = self.get_child_lock_range();
                let split_child = if cur_entry.is_token() {
                    cur_entry.split_if_huge_token(child_range).unwrap()
                } else {
                    cur_entry
                        .split_if_untracked_huge(child_range)
                        .expect("Protecting part of a huge page")
                };
                self.0.push_level(split_child);
                continue;
            }

            // Protect the current page.
            cur_entry.protect(prot_op, token_op);

            let protected_va = self.0.va..self.0.va + page_size::<C>(self.0.level);
            self.0.move_forward();

            return Some(protected_va);
        }

        None
    }

    /// Copies the mapping from the given cursor to the current cursor.
    ///
    /// All the mappings in the current cursor's range must be empty. The
    /// function allows the source cursor to operate on the mapping before the
    /// copy happens. So for mapped pages it is equivalent to protect then
    /// duplicate. For tokens, it is equivalent to modify the source token
    /// and set a new token in the destination.
    ///
    /// Only the mapping is copied, the mapped pages are not copied.
    ///
    /// It can only copy tracked mappings since we consider the untracked
    /// mappings not useful to be copied.
    ///
    /// After the operation, both cursors will advance by the specified length.
    ///
    /// # Safety
    ///
    /// The caller should ensure that
    ///  - the range being copied with the operation does not affect kernel's
    ///    memory safety.
    ///  - both of the cursors are in tracked mappings.
    ///
    /// # Panics
    ///
    /// This function will panic if:
    ///  - either one of the range to be copied is out of the range where any
    ///    of the cursor is required to operate;
    ///  - either one of the specified virtual address ranges only covers a
    ///    part of a page.
    ///  - the current cursor's range contains mapped pages.
    pub unsafe fn copy_from(
        &mut self,
        src: &mut Self,
        len: usize,
        prot_op: &mut impl FnMut(&mut PageProperty),
        token_op: &mut impl FnMut(&mut Token),
    ) {
        assert!(len % page_size::<C>(1) == 0);
        let this_end = self.0.va + len;
        assert!(this_end <= self.0.barrier_va.end);
        let src_end = src.0.va + len;
        assert!(src_end <= src.0.barrier_va.end);

        while self.0.va < this_end && src.0.va < src_end {
            let src_va = src.0.va;
            let src_level = src.0.level;
            let mut src_entry = src.0.cur_entry();

            match src_entry.to_ref() {
                Child::PageTableRef(pt) => {
                    src.0.push_level(pt);
                    continue;
                }
                Child::PageTable(_) => {
                    unreachable!();
                }
                Child::None => {
                    src.0.move_forward();
                    continue;
                }
                Child::Untracked(_, _, _) => {
                    panic!("Copying untracked mappings");
                }
                Child::Frame(page, mut prop) => {
                    let mapped_page_size = page.size();

                    // Do protection.
                    src_entry.protect(prot_op, token_op);

                    // Do copy.
                    prot_op(&mut prop);
                    self.jump(src_va).unwrap();
                    let original = self.map(page, prop);
                    assert!(original.is_none());

                    // Only move the source cursor forward since `Self::map` will do it.
                    // This assertion is to ensure that they move by the same length.
                    debug_assert_eq!(mapped_page_size, page_size::<C>(src.0.level));
                    src.0.move_forward();
                }
                Child::Token(mut token) => {
                    // Do protection.
                    src_entry.protect(prot_op, token_op);

                    // Do copy.
                    token_op(&mut token);
                    self.jump(src_va).unwrap();
                    self.mark(page_size::<C>(src_level), token);

                    src.0.move_forward();
                }
            }
        }
    }

    fn get_child_lock_range(&self) -> Range<usize> {
        debug_assert!(self.0.level > 1);
        get_idx_range::<C>(
            self.0.level - 1,
            self.0.va.align_down(page_size::<C>(self.0.level)),
            &self.0.barrier_va,
        )
    }
}
