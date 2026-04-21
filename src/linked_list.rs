use core::{
    marker::PhantomData,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};

use crate::succ::{AtomicSucc, List, Node, SuccData};

/// LockFreeLinkedList
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
            succ: AtomicSucc::new(SuccData::new(ptr::null_mut(), false, false)),
        }
    }
}

impl<K: Clone + Default + Ord, V> Node<K, V> for LinkedNode<K, V> {
    fn key(&self) -> K {
        self.key.clone()
    }

    fn load_backlink(&self) -> *mut LinkedNode<K, V> {
        self.backlink.load(Ordering::Acquire)
    }

    fn store_backlink(&self, new_val: *mut LinkedNode<K, V>) {
        self.backlink.store(new_val, Ordering::Release)
    }

    fn load_successor(&self) -> SuccData<LinkedNode<K, V>> {
        self.succ.load(Ordering::Acquire)
    }

    fn swap_successor(
        &self,
        expected: SuccData<LinkedNode<K, V>>,
        new_val: SuccData<LinkedNode<K, V>>,
    ) -> Result<SuccData<LinkedNode<K, V>>, SuccData<LinkedNode<K, V>>> {
        self.succ
            .compare_exchange(expected, new_val, Ordering::SeqCst, Ordering::SeqCst)
    }
}

/// Lock Free Linked List, with K for link ordering and V for contained value.
/// The lock free is achieved through multiple CAS at the cost of leaking the value heap.
pub struct LockFreeLinkedList<K, V> {
    /// Always a dummy head
    head: AtomicPtr<LinkedNode<K, V>>,
}

impl<K, V> LockFreeLinkedList<K, V>
where
    K: Clone + Default + Ord,
{
    pub fn new() -> Self {
        let dummy_head = Box::into_raw(Box::new(LinkedNode {
            key: K::default(),
            element: None,
            backlink: AtomicPtr::new(ptr::null_mut()),
            succ: AtomicSucc::default(),
        }));

        Self {
            head: AtomicPtr::new(dummy_head),
        }
    }

    /// Insert into linked list. Key must unique, return true if inserted.
    /// This operation is O(N).
    pub fn insert(&self, key: K, value: V) -> bool {
        unsafe {
            let new_node = Box::into_raw(Box::new(LinkedNode::new(key, Some(value))));
            let head_ptr = self.head.load(Ordering::Acquire);

            let (mut prev_node, mut next_node) = self.search(&(*new_node).key, head_ptr);

            if !next_node.is_null() && (*next_node).key == (*new_node).key {
                let _ = Box::from_raw(new_node);
                return false;
            }

            loop {
                let prev_succ = (*prev_node).load_successor();

                if prev_succ.flag {
                    self.help_flagged(prev_node, prev_succ.ptr);
                } else {
                    (*new_node)
                        .succ
                        .store(SuccData::new(next_node, false, false), Ordering::Release);

                    let expected = SuccData::new(next_node, false, false);
                    let new_val = SuccData::new(new_node, false, false);

                    match (*prev_node).swap_successor(expected, new_val) {
                        Ok(_) => return true,
                        Err(actual) => {
                            if actual.flag {
                                self.help_flagged(prev_node, actual.ptr);
                            }

                            while (*prev_node).load_successor().mark {
                                let bl = (*prev_node).backlink.load(Ordering::Acquire);
                                if !bl.is_null() {
                                    prev_node = bl;
                                } else {
                                    break;
                                }
                            }

                            let (new_prev, new_next) = self.search(&(*new_node).key, prev_node);
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
            let (prev_node, del_node) = self.search(key, head_ptr);

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
            let (_, next_node) = self.search(key, head_ptr);

            !next_node.is_null() && (*next_node).key == *key
        }
    }
}

/// An iterator over the lock-free linked list.
pub struct Iter<'a, K, V> {
    next_ptr: *mut LinkedNode<K, V>,
    _marker: PhantomData<&'a LinkedNode<K, V>>,
}

impl<K: Clone + Default + Ord, V> LockFreeLinkedList<K, V> {
    pub fn iter(&self) -> Iter<'_, K, V> {
        unsafe {
            let head_ptr = self.head.load(Ordering::Acquire);

            let first_node = (*head_ptr).load_successor().ptr;

            Iter {
                next_ptr: first_node,
                _marker: PhantomData,
            }
        }
    }
}

impl<K: Clone + Default + Ord, V> List<K, V, LinkedNode<K, V>> for LockFreeLinkedList<K, V> {
    unsafe fn search(
        &self,
        k: &K,
        mut curr_node: *mut LinkedNode<K, V>,
    ) -> (*mut LinkedNode<K, V>, *mut LinkedNode<K, V>) {
        unsafe {
            let mut next_node = (*curr_node).load_successor().ptr;

            while !next_node.is_null() && (*next_node).key < *k {
                let mut curr_succ_val = (*curr_node).load_successor();
                let mut next_succ_val = (*next_node).load_successor();

                while next_succ_val.mark && (!curr_succ_val.mark || curr_succ_val.ptr != next_node)
                {
                    if curr_succ_val.ptr == next_node {
                        self.help_marked(curr_node, next_node);
                    }

                    next_node = (*curr_node).load_successor().ptr;

                    if next_node.is_null() {
                        break;
                    }

                    curr_succ_val = (*curr_node).load_successor();
                    next_succ_val = (*next_node).load_successor();
                }

                if !next_node.is_null() && (*next_node).key < *k {
                    curr_node = next_node;
                    next_node = (*curr_node).load_successor().ptr;
                }
            }

            (curr_node, next_node)
        }
    }
}

impl<'a, K: Clone + Default + Ord, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            while !self.next_ptr.is_null() {
                let node = &*self.next_ptr;
                let succ_val = node.load_successor();

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
