use core::sync::atomic::{AtomicI64, AtomicPtr, Ordering};

use crate::{LinkedNode, List, LockFreeLinkedList, Node, succ::NodeIter};

type K = i64;

/// Lock Free Deque List, provides implementation to LIFO/FIFO linked list with indexed i64 managed internally.
/// Provides very optimal `push_back()` and `push_front()` function compared to vanilla linked list.
pub struct LockFreeDequeList<T> {
    list: LockFreeLinkedList<K, T>,
    front_seq: AtomicI64,
    back_seq: AtomicI64,
    tail_hint: AtomicPtr<LinkedNode<K, T>>,
}

impl<T> LockFreeDequeList<T> {
    pub fn new() -> Self {
        Self {
            list: LockFreeLinkedList::new(),
            front_seq: AtomicI64::new(0),
            back_seq: AtomicI64::new(1),
            tail_hint: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    pub unsafe fn head_node(&self) -> *mut LinkedNode<K, T> {
        unsafe { self.list.head_node() }
    }

    pub unsafe fn tail_node(&self) -> *mut LinkedNode<K, T> {
        self.tail_hint.load(Ordering::Acquire)
    }

    pub fn push_front(&self, value: T) -> K {
        let seq = self.front_seq.fetch_sub(1, Ordering::SeqCst);
        self.list.insert(seq, value);
        seq
    }

    pub fn push_back(&self, value: T) -> K {
        let seq = self.back_seq.fetch_add(1, Ordering::SeqCst);

        unsafe {
            let mut hint = self.tail_node();
            let head_ptr = self.head_node();

            if hint.is_null() || (*hint).key >= seq {
                hint = head_ptr;
            } else {
                while (*hint).succ.load(Ordering::Acquire).mark {
                    let bl = (*hint).backlink.load(Ordering::Acquire);
                    if !bl.is_null() && (*bl).key < seq {
                        hint = bl;
                    } else {
                        hint = head_ptr;
                        break;
                    }
                }
            }

            let new_node = self.list.insert_from(seq, value, hint);

            if !new_node.is_null() {
                self.tail_hint.store(new_node, Ordering::Release);
            }
        }

        seq
    }

    pub fn delete(&self, key: &K) -> bool {
        self.list.delete(key)
    }

    pub fn iter(&self) -> NodeIter<'_, K, T, LinkedNode<K, T>> {
        unsafe {
            let head_ptr = self.head_node();
            let first_node = (*head_ptr).load_successor().ptr;
            NodeIter::new(first_node)
        }
    }
}

impl<T> List<i64, T, LinkedNode<i64, T>> for LockFreeDequeList<T> {
    unsafe fn search_from(
        &self,
        key: &i64,
        curr_node: *mut LinkedNode<i64, T>,
    ) -> (*mut LinkedNode<i64, T>, *mut LinkedNode<i64, T>) {
        unsafe { self.list.search_from(key, curr_node) }
    }

    unsafe fn search_node(&self, key: &i64) -> Option<*mut LinkedNode<i64, T>> {
        unsafe { self.list.search_node(key) }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "std")]
    use crate::{DefaultGC, ScopedGarbageCollector};
    #[cfg(feature = "std")]
    use std::{
        sync::{Arc, Mutex},
        thread,
    };

    use super::*;

    #[test]
    fn test_basic_sequential_operations() {
        let list = LockFreeDequeList::<&'static str>::new();

        assert_eq!(list.push_back("A"), 1);
        assert_eq!(list.push_back("C"), 2);
        assert_eq!(list.push_back("B"), 3);

        assert!(list.contains(&1));
        assert!(list.contains(&2));
        assert!(list.contains(&3));
        assert!(!list.contains(&4), "Should not contain uninserted keys");

        assert!(list.delete(&2), "Deleting existing element should succeed");
        assert!(!list.contains(&2), "Element should be gone after deletion");
        assert!(
            !list.delete(&5),
            "Deleting non-existent element should fail"
        );

        assert!(list.contains(&1));
        assert!(list.contains(&3));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_concurrent_inserts() {
        let list = Arc::new(LockFreeDequeList::<i64>::new());
        let mut handles = vec![];

        let num_threads = 10i64;
        let items_per_thread = 1000;

        for t in 0..num_threads {
            let list_clone = Arc::clone(&list);
            handles.push(thread::spawn(move || {
                for i in 0..items_per_thread {
                    // global unique key across threads
                    let key = (t * items_per_thread) + i + 1;
                    list_clone.push_back(key);
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
    #[cfg(feature = "std")]
    fn test_concurrent_inserts_front() {
        let list = Arc::new(LockFreeDequeList::<i64>::new());
        let mut handles = vec![];

        let num_threads = 10i64;
        let items_per_thread = 1000;

        for t in 0..num_threads {
            let list_clone = Arc::clone(&list);
            handles.push(thread::spawn(move || {
                for i in 0..items_per_thread {
                    // global unique key across threads
                    let key = (t * items_per_thread) + i + 1;
                    list_clone.push_front(key);
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        for t in 0..num_threads {
            for i in 0..items_per_thread {
                let key = -(t * items_per_thread) - i;
                assert!(list.contains(&key), "Missing key: {}", key);
            }
        }
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_concurrent_inserts_and_deletes() {
        let list = Arc::new(LockFreeDequeList::<i64>::new());
        let mut handles = vec![];

        for i in 1..=1000 {
            list.push_back(i);
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

        for i in 1..=1000 {
            assert!(!list.contains(&i), "Key {} should have been deleted", i);
        }

        assert_eq!(collector.lock().unwrap().prune(), 1000);
    }
}
