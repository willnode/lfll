use core::{
    marker::PhantomData,
    ptr,
    sync::atomic::{AtomicPtr, AtomicUsize, Ordering},
};

const MARK_BIT: usize = 1;
const FLAG_BIT: usize = 2;
const PTR_MASK: usize = !(MARK_BIT | FLAG_BIT);

/// Successor data for linked list node.
/// Mostly a helper to pack into single atomic pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuccData<K, V> {
    /// This Node in pointer, not Arc.
    /// It's may tempting replace everything in this codebase with Rc, but please no.
    pub ptr: *mut Node<K, V>,
    /// Control when the right pointer of this node can be changed anytime.
    /// When it does, backlinks should be instead until the node is not marked.
    pub mark: bool,
    /// Control when this node is marked for deletion.
    /// When it does, deletion is happening and pointer cannot be changed (mark must false).
    pub flag: bool,
}

impl<K, V> SuccData<K, V> {
    pub fn new(ptr: *mut Node<K, V>, mark: bool, flag: bool) -> Self {
        Self { ptr, mark, flag }
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
            ptr: (val & PTR_MASK) as *mut Node<K, V>,
            mark: (val & MARK_BIT) != 0,
            flag: (val & FLAG_BIT) != 0,
        }
    }
}

/// Structure that holds SuccData inside an AtomicUsize
pub struct AtomicSucc<K, V> {
    inner: AtomicUsize,
    _marker: PhantomData<*mut Node<K, V>>,
}

impl<K, V> AtomicSucc<K, V> {
    pub fn new(initial: SuccData<K, V>) -> Self {
        Self {
            inner: AtomicUsize::new(initial.into_packed()),
            _marker: PhantomData,
        }
    }

    pub fn load(&self, order: Ordering) -> SuccData<K, V> {
        SuccData::from_packed(self.inner.load(order))
    }

    pub fn store(&self, data: SuccData<K, V>, order: Ordering) {
        self.inner.store(data.into_packed(), order);
    }

    pub fn compare_exchange(
        &self,
        expected: SuccData<K, V>,
        new: SuccData<K, V>,
        success: Ordering,
        failure: Ordering,
    ) -> Result<SuccData<K, V>, SuccData<K, V>> {
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

/// LockFreeLinkedList
pub struct Node<K, V> {
    /// The key helps Linked List in Ordered fashion
    pub key: K,
    /// Contained value, None if dummy head
    pub element: Option<V>,
    /// When deletion happening, this is set so iteration can recover
    pub backlink: AtomicPtr<Node<K, V>>,
    /// Pointer and flags to another Node
    pub succ: AtomicSucc<K, V>,
}

impl<K, V> Node<K, V> {
    pub fn new(key: K, element: Option<V>) -> Self {
        Self {
            key,
            element,
            backlink: AtomicPtr::new(ptr::null_mut()),
            succ: AtomicSucc::new(SuccData::new(ptr::null_mut(), false, false)),
        }
    }
}

/// Lock Free Linked List, with K for link ordering and V for contained value.
/// The lock free is achieved through multiple CAS at the cost of leaking the value heap.
pub struct LockFreeLinkedList<K, V> {
    /// Always a dummy head
    head: core::sync::atomic::AtomicPtr<Node<K, V>>,
}

impl<K, V> LockFreeLinkedList<K, V>
where
    K: Ord + Default,
{
    pub fn new() -> Self {
        let dummy_head = Box::into_raw(Box::new(Node {
            key: K::default(),
            element: None,
            backlink: AtomicPtr::new(ptr::null_mut()),
            succ: AtomicSucc::new(SuccData::new(ptr::null_mut(), false, false)),
        }));

        Self {
            head: AtomicPtr::new(dummy_head),
        }
    }

    /// Flag the node for deletion (will block!)
    unsafe fn try_flag(
        &self,
        mut prev_node: *mut Node<K, V>,
        target_node: *mut Node<K, V>,
    ) -> (*mut Node<K, V>, bool) {
        unsafe {
            loop {
                let succ_val = (*prev_node).succ.load(Ordering::Acquire);

                if succ_val.ptr == target_node && succ_val.flag {
                    return (prev_node, false);
                }

                let expected = SuccData::new(target_node, false, false);
                let new_val = SuccData::new(target_node, false, true);

                match (*prev_node).succ.compare_exchange(
                    expected,
                    new_val,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) => return (prev_node, true),
                    Err(actual) => {
                        if actual.ptr == target_node && actual.flag {
                            return (prev_node, false);
                        }

                        while (*prev_node).succ.load(Ordering::Acquire).mark {
                            let bl = (*prev_node).backlink.load(Ordering::Acquire);
                            if !bl.is_null() {
                                prev_node = bl;
                            } else {
                                break;
                            }
                        }

                        let (new_prev, del_node) = self.search_from(&(*target_node).key, prev_node);
                        if del_node != target_node {
                            return (ptr::null_mut(), false);
                        }
                        prev_node = new_prev;
                    }
                }
            }
        }
    }

    /// Mark the node such that it can be changed (will block!)
    unsafe fn try_mark(&self, del_node: *mut Node<K, V>) {
        unsafe {
            loop {
                let succ_val = (*del_node).succ.load(Ordering::Acquire);

                if succ_val.mark {
                    break;
                }

                let expected = SuccData::new(succ_val.ptr, false, false);
                let new_val = SuccData::new(succ_val.ptr, true, false);

                match (*del_node).succ.compare_exchange(
                    expected,
                    new_val,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
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

    /// Unflag the node from deletion (not blocking)
    unsafe fn help_flagged(&self, prev_node: *mut Node<K, V>, del_node: *mut Node<K, V>) {
        unsafe {
            (*del_node).backlink.store(prev_node, Ordering::Release);

            let succ_val = (*del_node).succ.load(Ordering::Acquire);
            if !succ_val.mark {
                self.try_mark(del_node);
            }

            self.help_marked(prev_node, del_node);
        }
    }

    /// Unmark the node such that it can't be changed (not blocking)
    unsafe fn help_marked(&self, prev_node: *mut Node<K, V>, del_node: *mut Node<K, V>) {
        unsafe {
            let next_node = (*del_node).succ.load(Ordering::Acquire).ptr;

            let expected = SuccData::new(del_node, false, true);
            let new_val = SuccData::new(next_node, false, false);

            // if failed, other thread may already done it
            let _ = (*prev_node).succ.compare_exchange(
                expected,
                new_val,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }
    }

    unsafe fn search_from(
        &self,
        k: &K,
        mut curr_node: *mut Node<K, V>,
    ) -> (*mut Node<K, V>, *mut Node<K, V>) {
        unsafe {
            let mut next_node = (*curr_node).succ.load(Ordering::Acquire).ptr;

            while !next_node.is_null() && (*next_node).key < *k {
                let mut curr_succ_val = (*curr_node).succ.load(Ordering::Acquire);
                let mut next_succ_val = (*next_node).succ.load(Ordering::Acquire);

                while next_succ_val.mark && (!curr_succ_val.mark || curr_succ_val.ptr != next_node)
                {
                    if curr_succ_val.ptr == next_node {
                        self.help_marked(curr_node, next_node);
                    }

                    next_node = (*curr_node).succ.load(Ordering::Acquire).ptr;

                    if next_node.is_null() {
                        break;
                    }

                    curr_succ_val = (*curr_node).succ.load(Ordering::Acquire);
                    next_succ_val = (*next_node).succ.load(Ordering::Acquire);
                }

                if !next_node.is_null() && (*next_node).key < *k {
                    curr_node = next_node;
                    next_node = (*curr_node).succ.load(Ordering::Acquire).ptr;
                }
            }

            (curr_node, next_node)
        }
    }

    /// Insert into linked list. Key must unique, return true if inserted.
    /// This operation is O(N).
    pub fn insert(&self, key: K, value: V) -> bool {
        unsafe {
            let new_node = Box::into_raw(Box::new(Node::new(key, Some(value))));
            let head_ptr = self.head.load(Ordering::Acquire);

            let (mut prev_node, mut next_node) = self.search_from(&(*new_node).key, head_ptr);

            if !next_node.is_null() && (*next_node).key == (*new_node).key {
                let _ = Box::from_raw(new_node);
                return false;
            }

            loop {
                let prev_succ = (*prev_node).succ.load(Ordering::Acquire);

                if prev_succ.flag {
                    self.help_flagged(prev_node, prev_succ.ptr);
                } else {
                    (*new_node)
                        .succ
                        .store(SuccData::new(next_node, false, false), Ordering::Release);

                    let expected = SuccData::new(next_node, false, false);
                    let new_val = SuccData::new(new_node, false, false);

                    match (*prev_node).succ.compare_exchange(
                        expected,
                        new_val,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ) {
                        Ok(_) => return true,
                        Err(actual) => {
                            if actual.flag {
                                self.help_flagged(prev_node, actual.ptr);
                            }

                            while (*prev_node).succ.load(Ordering::Acquire).mark {
                                let bl = (*prev_node).backlink.load(Ordering::Acquire);
                                if !bl.is_null() {
                                    prev_node = bl;
                                } else {
                                    break;
                                }
                            }

                            let (new_prev, new_next) =
                                self.search_from(&(*new_node).key, prev_node);
                            prev_node = new_prev;
                            next_node = new_next;

                            if !next_node.is_null() && (*next_node).key == (*new_node).key {
                                let _ = Box::from_raw(new_node);
                                return false;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Delete from linked list. Return true if found and deleted.
    /// This operation is O(N) and does not reclaim allocation from Value.
    pub fn delete(&self, key: &K) -> bool {
        unsafe {
            let head_ptr = self.head.load(Ordering::Acquire);
            let (prev_node, del_node) = self.search_from(key, head_ptr);

            if del_node.is_null() || (*del_node).key != *key {
                return false;
            }

            let (actual_prev, result) = self.try_flag(prev_node, del_node);

            if !actual_prev.is_null() {
                self.help_flagged(actual_prev, del_node);
            }

            result
        }
    }

    /// Check if key contained in list.
    /// This operation is O(N).
    pub fn contains(&self, key: &K) -> bool {
        unsafe {
            let head_ptr = self.head.load(Ordering::Acquire);
            let (_, next_node) = self.search_from(key, head_ptr);

            !next_node.is_null() && (*next_node).key == *key
        }
    }
}

/// An iterator over the lock-free linked list.
pub struct Iter<'a, K, V> {
    next_ptr: *mut Node<K, V>,
    _marker: PhantomData<&'a Node<K, V>>,
}

impl<K: Ord, V> LockFreeLinkedList<K, V> {
    pub fn iter(&self) -> Iter<'_, K, V> {
        unsafe {
            let head_ptr = self.head.load(Ordering::Acquire);

            let first_node = (*head_ptr).succ.load(Ordering::Acquire).ptr;

            Iter {
                next_ptr: first_node,
                _marker: PhantomData,
            }
        }
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            while !self.next_ptr.is_null() {
                let node = &*self.next_ptr;
                let succ_val = node.succ.load(Ordering::Acquire);

                self.next_ptr = succ_val.ptr;

                if !succ_val.mark {
                    if let Some(ref val) = node.element {
                        return Some((&node.key, val));
                    }
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread};

    use super::*;

    #[test]
    fn test_basic_sequential_operations() {
        let list = LockFreeLinkedList::<i32, &'static str>::new();

        assert!(list.insert(10, "A"));
        assert!(list.insert(30, "C"));
        assert!(list.insert(20, "B"));

        assert!(
            !list.insert(20, "Duplicate"),
            "Duplicate insert should return false"
        );

        assert!(list.contains(&10));
        assert!(list.contains(&20));
        assert!(list.contains(&30));
        assert!(!list.contains(&40), "Should not contain uninserted keys");

        assert!(list.delete(&20), "Deleting existing element should succeed");
        assert!(!list.contains(&20), "Element should be gone after deletion");
        assert!(
            !list.delete(&50),
            "Deleting non-existent element should fail"
        );

        assert!(list.contains(&10));
        assert!(list.contains(&30));
    }

    #[test]
    fn test_concurrent_inserts() {
        let list = Arc::new(LockFreeLinkedList::<i32, i32>::new());
        let mut handles = vec![];

        let num_threads = 10;
        let items_per_thread = 1000;

        for t in 0..num_threads {
            let list_clone = Arc::clone(&list);
            handles.push(thread::spawn(move || {
                for i in 0..items_per_thread {
                    // global unique key across threads
                    let key = (t * items_per_thread) + i + 1;
                    assert!(list_clone.insert(key, key));
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        for t in 0..num_threads {
            for i in 0..items_per_thread {
                let key = (t * items_per_thread) + i + 1;
                assert!(list.contains(&key), "Missing key: {}", key);
            }
        }
    }

    #[test]
    fn test_concurrent_inserts_and_deletes() {
        let list = Arc::new(LockFreeLinkedList::<i32, i32>::new());
        let mut handles = vec![];

        for i in 1..=1000 {
            list.insert(i, i);
        }

        let list_clone1 = Arc::clone(&list);
        handles.push(thread::spawn(move || {
            for i in (2..=1000).step_by(2) {
                list_clone1.delete(&i);
            }
        }));

        let list_clone2 = Arc::clone(&list);
        handles.push(thread::spawn(move || {
            for i in (1..=1000).step_by(2) {
                list_clone2.delete(&i);
            }
        }));

        for handle in handles {
            handle.join().unwrap();
        }

        for i in 1..=1000 {
            assert!(!list.contains(&i), "Key {} should have been deleted", i);
        }
    }

    #[test]
    fn test_iteration_and_sorting() {
        let list = LockFreeLinkedList::<i32, &'static str>::new();

        assert!(list.insert(50, "Fifty"));
        assert!(list.insert(10, "Ten"));
        assert!(list.insert(30, "Thirty"));
        assert!(list.insert(40, "Forty"));
        assert!(list.insert(20, "Twenty"));

        let keys: Vec<i32> = list.iter().map(|(k, _)| *k).collect();

        assert_eq!(keys, vec![10, 20, 30, 40, 50], "List is not sorted!");

        let pairs: Vec<(i32, &str)> = list.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(
            pairs,
            vec![
                (10, "Ten"),
                (20, "Twenty"),
                (30, "Thirty"),
                (40, "Forty"),
                (50, "Fifty")
            ]
        );

        assert!(list.delete(&30));
        assert!(list.delete(&10));

        let updated_keys: Vec<i32> = list.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            updated_keys,
            vec![20, 40, 50],
            "Iterator did not skip deleted nodes correctly!"
        );
    }
}
