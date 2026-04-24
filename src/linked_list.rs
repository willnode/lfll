#[cfg(not(feature = "std"))]
use alloc::boxed::Box;

use crate::succ::{AtomicSucc, List, Node, SuccData};
use core::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

/// `LockFreeLinkedList` node internal data.
pub struct LinkedNode<K, V> {
    /// The key helps Linked List in Ordered fashion
    pub key: K,
    /// Contained value, None if dummy head
    pub element: Option<V>,
    /// When deletion happening, this is set so iteration can recover
    pub backlink: AtomicPtr<LinkedNode<K, V>>,
    /// Pointer and flags to another Node
    pub succ: AtomicSucc<LinkedNode<K, V>>,
}

impl<K, V> LinkedNode<K, V> {
    pub fn new(key: K, element: Option<V>) -> Self {
        Self {
            key,
            element,
            backlink: AtomicPtr::new(ptr::null_mut()),
            succ: AtomicSucc::default(),
        }
    }
}

impl<K: Default + Ord, V> Node<K, V> for LinkedNode<K, V> {
    fn key(&self) -> &K {
        &self.key
    }

    fn element(&self) -> Option<&V> {
        self.element.as_ref()
    }

    fn load_backlink(&self) -> *mut Self {
        self.backlink.load(Ordering::Relaxed)
    }

    fn store_backlink(&self, new_val: *mut Self) {
        self.backlink.store(new_val, Ordering::Relaxed)
    }

    fn load_successor(&self) -> SuccData<Self> {
        self.succ.load(Ordering::Relaxed)
    }

    fn store_successor(&self, new_val: SuccData<Self>) {
        self.succ.store(new_val, Ordering::Relaxed)
    }

    fn swap_successor(
        &self,
        expected: SuccData<Self>,
        new_val: SuccData<Self>,
    ) -> Result<SuccData<Self>, SuccData<Self>> {
        self.succ
            .compare_exchange(expected, new_val, Ordering::SeqCst, Ordering::SeqCst)
    }
}

/// Lock Free Linked List, provides minimal implementation to ordered linked list.
/// Provides very optimal performance for short linked list without key lookup table.
pub struct LockFreeLinkedList<K, V> {
    /// Always a dummy head
    head: AtomicPtr<LinkedNode<K, V>>,
}

impl<K, V> LockFreeLinkedList<K, V>
where
    K: Default + Ord,
{
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    fn init(&self) -> *mut LinkedNode<K, V> {
        // TODO: #![cold]
        let dummy_head = Box::into_raw(Box::new(LinkedNode {
            // this key is not read for head, so any value works
            key: K::default(),
            element: None,
            backlink: AtomicPtr::new(ptr::null_mut()),
            succ: AtomicSucc::default(),
        }));

        match self.head.compare_exchange(
            ptr::null_mut(),
            dummy_head,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => dummy_head,
            Err(_) => {
                let _ = unsafe { Box::from_raw(dummy_head) };
                let r = self.head.load(Ordering::Relaxed);
                debug_assert!(!r.is_null());
                r
            }
        }
    }

    /// Insert into linked list. Key must unique, return true if inserted.
    /// This operation is O(N).
    pub fn insert(&self, key: K, value: V) -> bool {
        unsafe { !self.insert_from(key, value, self.head_node()).is_null() }
    }

    /// Insert into linked list from hinted node position. Key must unique, return true if inserted.
    pub unsafe fn insert_from(
        &self,
        key: K,
        value: V,
        curr_node: *mut LinkedNode<K, V>,
    ) -> *mut LinkedNode<K, V> {
        unsafe {
            let new_node = Box::into_raw(Box::new(LinkedNode::new(key, Some(value))));

            let (mut prev_node, mut next_node) = self.search_from(&(*new_node).key, curr_node);

            if !next_node.is_null() && (*next_node).key == (*new_node).key {
                let _ = Box::from_raw(new_node);
                return ptr::null_mut();
            }

            loop {
                let prev_succ = (*prev_node).load_successor();

                if prev_succ.flag {
                    self.help_flagged(prev_node, prev_succ.ptr);
                } else {
                    (*new_node).store_successor(SuccData::new(next_node));

                    let expected = SuccData::new(next_node);
                    let new_val = SuccData::new(new_node);

                    match (*prev_node).swap_successor(expected, new_val) {
                        Ok(_) => return new_node,
                        Err(actual) => {
                            if actual.flag {
                                self.help_flagged(prev_node, actual.ptr);
                            }

                            while (*prev_node).load_successor().mark {
                                let bl = (*prev_node).load_backlink();
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
                                return ptr::null_mut();
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
            let head_ptr = self.head_node();
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
}

impl<K: Default + Ord, V> List<K, V, LinkedNode<K, V>> for LockFreeLinkedList<K, V> {
    /// Search from curr_node
    unsafe fn search_from(
        &self,
        k: &K,
        mut curr_node: *mut LinkedNode<K, V>,
    ) -> (*mut LinkedNode<K, V>, *mut LinkedNode<K, V>) {
        unsafe {
            let mut succ_curr = (*curr_node).load_successor();
            let mut next_node = succ_curr.ptr;

            while !next_node.is_null() && (*next_node).key < *k {
                let mut curr_succ_val = succ_curr;
                let mut next_succ_val = (*next_node).load_successor();

                while next_succ_val.mark && (!curr_succ_val.mark || curr_succ_val.ptr != next_node)
                {
                    if curr_succ_val.ptr == next_node {
                        self.help_unflag(curr_node, next_node);
                    }

                    succ_curr = (*curr_node).load_successor();

                    next_node = succ_curr.ptr;

                    if next_node.is_null() {
                        break;
                    }

                    curr_succ_val = succ_curr;
                    next_succ_val = (*next_node).load_successor();
                }

                if !next_node.is_null() && (*next_node).key < *k {
                    curr_node = next_node;
                    succ_curr = (*curr_node).load_successor();
                    next_node = succ_curr.ptr;
                } else {
                    break;
                }
            }

            (curr_node, next_node)
        }
    }
    unsafe fn search_node(&self, key: &K) -> Option<*mut LinkedNode<K, V>> {
        unsafe {
            let head_ptr = self.head_node();
            let (_, next_node) = self.search_from(key, head_ptr);

            if !next_node.is_null() && (*next_node).key == *key {
                Some(next_node)
            } else {
                None
            }
        }
    }
    unsafe fn head_node(&self) -> *mut LinkedNode<K, V> {
        let head = self.head.load(Ordering::Relaxed);
        if head.is_null() { self.init() } else { head }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "std")]
    use crate::{DefaultGC, ScopedGarbageCollector};
    #[cfg(not(feature = "std"))]
    use alloc::{vec, vec::Vec};
    #[cfg(feature = "std")]
    use std::{
        sync::{Arc, Mutex},
        thread,
    };

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
    #[cfg(feature = "std")]
    fn test_concurrent_inserts() {
        let list = Arc::new(LockFreeLinkedList::<i32, i32>::new());
        let mut handles = vec![];

        let num_threads = 8;
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

        let total_items = (num_threads * items_per_thread) as usize;
        let mut expected_keys = Vec::with_capacity(total_items);

        for t in 0..num_threads {
            for i in 0..items_per_thread {
                let key = (t * items_per_thread) + i + 1;
                expected_keys.push(key);
            }
        }

        let results = list.contains_many(&expected_keys);

        for (index, &was_found) in results.iter().enumerate() {
            assert!(was_found, "Missing key: {}", expected_keys[index]);
        }
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_concurrent_inserts_and_deletes() {
        let list = Arc::new(LockFreeLinkedList::<i32, i32>::new());
        let mut handles = vec![];

        for i in 1..=1000 {
            list.insert(i, i);
        }

        let collector: Arc<Mutex<DefaultGC>> = DefaultGC::new();

        let list_clone1 = Arc::clone(&list);
        let collector1 = Arc::clone(&collector);
        handles.push(thread::spawn(move || {
            for i in (2..=1000).step_by(2) {
                list_clone1.delete(&i);
            }
            collector1.lock().unwrap().collect();
        }));

        let list_clone2 = Arc::clone(&list);
        let collector2 = Arc::clone(&collector);
        handles.push(thread::spawn(move || {
            for i in (1..=1000).step_by(2) {
                list_clone2.delete(&i);
            }
            collector2.lock().unwrap().collect();
        }));

        for handle in handles {
            handle.join().unwrap();
        }

        let results = list.contains_many(&(1..=1000).collect::<Vec<_>>()[..]);
        for (i, &was_found) in results.iter().enumerate() {
            assert!(!was_found, "Key {} should have been deleted", i);
        }
        assert_eq!(collector.lock().unwrap().prune(), 1000);
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
