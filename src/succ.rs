use core::marker::PhantomData;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::DefaultGC;
use crate::GarbageCollector;

const MARK_BIT: usize = 1;
const FLAG_BIT: usize = 2;
const PTR_MASK: usize = !(MARK_BIT | FLAG_BIT);

/// Successor data for linked list node.
/// Mostly a helper to pack into single atomic pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuccData<T> {
    /// This Node in pointer, not Arc.
    /// It's may tempting replace everything in this codebase with Rc, but please no.
    pub ptr: *mut T,
    /// Control when the right pointer of this node can be changed anytime.
    /// When it does, backlinks should be instead until the node is not marked.
    pub mark: bool,
    /// Control when this node is marked for deletion.
    /// When it does, deletion is happening and pointer cannot be changed (mark must false).
    pub flag: bool,
}

impl<T> SuccData<T> {
    pub fn new(ptr: *mut T) -> Self {
        Self {
            ptr,
            mark: false,
            flag: false,
        }
    }

    pub fn new_marked(ptr: *mut T) -> Self {
        Self {
            ptr,
            mark: true,
            flag: false,
        }
    }

    pub fn new_flagged(ptr: *mut T) -> Self {
        Self {
            ptr,
            mark: false,
            flag: true,
        }
    }

    fn into_packed(self) -> usize {
        let mut val = (self.ptr as usize) & PTR_MASK;
        if self.mark {
            val |= MARK_BIT;
        }
        if self.flag {
            val |= FLAG_BIT;
        }
        val
    }

    fn from_packed(val: usize) -> Self {
        Self {
            ptr: (val & PTR_MASK) as *mut T,
            mark: (val & MARK_BIT) != 0,
            flag: (val & FLAG_BIT) != 0,
        }
    }
}

/// Structure that holds SuccData inside an AtomicUsize
pub struct AtomicSucc<T> {
    inner: AtomicUsize,
    _marker: PhantomData<*mut T>,
}

impl<T> AtomicSucc<T> {
    pub fn new(initial: SuccData<T>) -> Self {
        Self {
            inner: AtomicUsize::new(initial.into_packed()),
            _marker: PhantomData,
        }
    }

    pub fn load(&self, order: Ordering) -> SuccData<T> {
        SuccData::from_packed(self.inner.load(order))
    }

    pub fn store(&self, data: SuccData<T>, order: Ordering) {
        self.inner.store(data.into_packed(), order);
    }

    pub fn compare_exchange(
        &self,
        expected: SuccData<T>,
        new: SuccData<T>,
        success: Ordering,
        failure: Ordering,
    ) -> Result<SuccData<T>, SuccData<T>> {
        match self.inner.compare_exchange(
            expected.into_packed(),
            new.into_packed(),
            success,
            failure,
        ) {
            Ok(val) => Ok(SuccData::from_packed(val)),
            Err(val) => Err(SuccData::from_packed(val)),
        }
    }
}

impl<T> Default for AtomicSucc<T> {
    fn default() -> Self {
        Self::new(SuccData::new(core::ptr::null_mut()))
    }
}

pub trait Node<K, V>
where
    K: Ord + Default,
    Self: Sized,
{
    fn key(&self) -> &K;

    fn element(&self) -> Option<&V>;

    fn load_backlink(&self) -> *mut Self;

    fn store_backlink(&self, new_val: *mut Self);

    fn load_successor(&self) -> SuccData<Self>;

    fn store_successor(&self, new_val: SuccData<Self>);

    fn swap_successor(
        &self,
        expected: SuccData<Self>,
        new_val: SuccData<Self>,
    ) -> Result<SuccData<Self>, SuccData<Self>>;
}

/// An iterator over the lock-free list.
pub struct NodeIter<'a, K, V, N>
where
    K: Ord + Default,
    N: Node<K, V>,
{
    next_ptr: *mut N,
    _marker: PhantomData<&'a N>,
    // Why?
    _marker1: PhantomData<&'a K>,
    _marker2: PhantomData<&'a V>,
}

impl<'a, K: Default + Ord, V, N: Node<K, V>> NodeIter<'a, K, V, N> {
    pub fn new(ptr: *mut N) -> Self {
        Self {
            next_ptr: ptr,
            _marker: PhantomData,
            _marker1: PhantomData,
            _marker2: PhantomData,
        }
    }
}

impl<'a, K: Default + Ord + Clone, V, N: Node<K, V>> Iterator for NodeIter<'a, K, V, N> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            while !self.next_ptr.is_null() {
                let node = &*self.next_ptr;
                let succ_val = node.load_successor();

                self.next_ptr = succ_val.ptr;

                if !succ_val.mark {
                    if let Some(val) = node.element() {
                        return Some((&node.key(), val));
                    }
                }
            }
            None
        }
    }
}

pub trait List<K, V, N>
where
    K: Ord + Default,
    N: Node<K, V>,
{
    unsafe fn search_from(&self, key: &K, curr_node: *mut N) -> (*mut N, *mut N);

    unsafe fn search_node(&self, key: &K) -> Option<*mut N>;

    fn contains(&self, key: &K) -> bool {
        unsafe { self.search_node(key) }.is_some()
    }

    fn get<'a>(&'a self, key: &'a K) -> Option<&'a V>
    where
        N: 'a,
    {
        unsafe { self.search_node(key) }
            .map(|s| unsafe { (*s).element() })
            .flatten()
    }

    /// Flag the node for deletion (will block!)
    unsafe fn try_flag(&self, mut prev_node: *mut N, target_node: *mut N) -> (*mut N, bool) {
        unsafe {
            loop {
                let succ_val = (*prev_node).load_successor();

                if succ_val.ptr == target_node && succ_val.flag {
                    return (prev_node, false);
                }

                let expected = SuccData::new(target_node);
                let new_val = SuccData::new_flagged(target_node);

                match (*prev_node).swap_successor(expected, new_val) {
                    Ok(_) => return (prev_node, true),
                    Err(actual) => {
                        if actual.ptr == target_node && actual.flag {
                            return (prev_node, false);
                        }

                        while (*prev_node).load_successor().mark {
                            let bl = (*prev_node).load_backlink();
                            if !bl.is_null() {
                                prev_node = bl;
                            } else {
                                break;
                            }
                        }

                        let (new_prev, del_node) =
                            self.search_from(&(*target_node).key(), prev_node);
                        if del_node != target_node {
                            return (core::ptr::null_mut(), false);
                        }
                        prev_node = new_prev;
                    }
                }
            }
        }
    }

    /// Mark the node such that it can be changed (will block!)
    unsafe fn try_mark(&self, del_node: *mut N) {
        unsafe {
            loop {
                let succ_val = (*del_node).load_successor();

                if succ_val.mark {
                    break;
                }

                let expected = SuccData::new(succ_val.ptr);
                let new_val = SuccData::new_marked(succ_val.ptr);

                match (*del_node).swap_successor(expected, new_val) {
                    Ok(_) => break,
                    Err(actual) => {
                        if actual.flag {
                            self.help_flagged(del_node, actual.ptr);
                        }
                    }
                }
            }
        }
    }

    /// Mark the node to unflag it (may blocking)
    unsafe fn help_flagged(&self, prev_node: *mut N, del_node: *mut N) {
        unsafe {
            (*del_node).store_backlink(prev_node);

            let succ_val = (*del_node).load_successor();
            if !succ_val.mark {
                self.try_mark(del_node);
            }

            self.help_unflag(prev_node, del_node);
        }
    }

    /// Unflag the node such that it can't be changed (not blocking)
    unsafe fn help_unflag(&self, prev_node: *mut N, del_node: *mut N) {
        unsafe {
            let next_node = (*del_node).load_successor().ptr;

            let expected = SuccData::new_flagged(del_node);
            let new_val = SuccData::new(next_node);

            // if failed, other thread may already done it
            if let Ok(e) = (*prev_node).swap_successor(expected, new_val) {
                DefaultGC::push(e.ptr);
            }
        }
    }
}
