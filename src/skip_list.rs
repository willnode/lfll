use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;

use crate::Seedable;
use crate::succ::NodeIter;
use crate::succ::SuccData;
use crate::succ::{AtomicSucc, List, Node};

/// `LockFreeSkipList` node internal data.
pub struct SkipNode<K, V> {
    /// The key helps Linked List in Ordered fashion
    pub key: K,
    /// Contained value, None if dummy
    pub element: Option<V>,
    /// Downward Node (N - 1 tower), only set once
    pub down: *mut SkipNode<K, V>,
    /// Downmost Node (level 0 tower), only set once
    pub tower_root: *mut SkipNode<K, V>,
    /// When deletion happening, this is set so iteration can recover
    pub backlink: AtomicPtr<SkipNode<K, V>>,
    /// Pointer and flags to another Node
    pub succ: AtomicSucc<SkipNode<K, V>>,
}

impl<K: Default + Ord, V> Node<K, V> for SkipNode<K, V> {
    fn key(&self) -> &K {
        &self.key
    }

    fn element(&self) -> Option<&V> {
        self.element.as_ref()
    }

    fn load_backlink(&self) -> *mut Self {
        self.backlink.load(Ordering::Acquire)
    }

    fn store_backlink(&self, new_val: *mut Self) {
        self.backlink.store(new_val, Ordering::Release)
    }

    fn load_successor(&self) -> SuccData<Self> {
        self.succ.load(Ordering::Acquire)
    }

    fn store_successor(&self, new_val: SuccData<Self>) {
        self.succ.store(new_val, Ordering::Release)
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

const MAX_LEVEL: usize = 16;

/// Lock Free Skip List, provides implementation to ordered linked list with table map.
/// Provides optimal performance for long linked list and key lookup at the cost of insert operation.
/// Requires random seed at insertion which is provided automatically if K is int-like types.
pub struct LockFreeSkipList<K, V> {
    /// Head Towers
    head_tower: [*mut SkipNode<K, V>; MAX_LEVEL],
}

unsafe impl<K: Send + Sync, V: Send + Sync> Send for LockFreeSkipList<K, V> {}
unsafe impl<K: Send + Sync, V: Send + Sync> Sync for LockFreeSkipList<K, V> {}

impl<K: Clone + Default + Ord, V> LockFreeSkipList<K, V> {
    pub fn new() -> Self {
        unsafe {
            let mut head_tower = [ptr::null_mut(); MAX_LEVEL];

            let root = Box::into_raw(Box::new(SkipNode {
                // TODO: should be neg infinity
                key: K::default(),
                element: None,
                down: ptr::null_mut(),
                tower_root: ptr::null_mut(),
                backlink: AtomicPtr::new(ptr::null_mut()),
                succ: AtomicSucc::default(),
            }));

            (*root).tower_root = root;
            head_tower[0] = root;

            // dummies at each level
            for i in 1..MAX_LEVEL {
                let node = Box::into_raw(Box::new(SkipNode {
                    key: K::default(),
                    element: None,
                    down: head_tower[i - 1],
                    tower_root: root,
                    backlink: AtomicPtr::new(ptr::null_mut()),
                    succ: AtomicSucc::default(),
                }));
                head_tower[i] = node;
            }

            Self { head_tower }
        }
    }

    /// Builds an unlinked tower of nodes from 0 to top_level
    unsafe fn build_tower(key: K, value: V, top_level: usize) -> [*mut SkipNode<K, V>; MAX_LEVEL] {
        let mut tower = [ptr::null_mut(); MAX_LEVEL];

        let root = Box::into_raw(Box::new(SkipNode {
            key: key.clone(),
            element: Some(value),
            down: ptr::null_mut(),
            tower_root: ptr::null_mut(),
            backlink: AtomicPtr::new(ptr::null_mut()),
            succ: AtomicSucc::default(),
        }));

        unsafe { (*root).tower_root = root };
        tower[0] = root;

        for i in 0..top_level {
            let node = Box::into_raw(Box::new(SkipNode {
                key: key.clone(),
                element: None,
                down: tower[i],
                tower_root: root,
                backlink: AtomicPtr::new(ptr::null_mut()),
                succ: AtomicSucc::default(),
            }));
            tower[i + 1] = node;
        }

        tower
    }

    pub unsafe fn prune_tower(tower: [*mut SkipNode<K, V>; MAX_LEVEL]) {
        for i in 0..MAX_LEVEL {
            if tower[i].is_null() {
                break;
            }
            _ = unsafe { Box::from_raw(tower[i]) };
        }
    }

    pub fn insert_seeded(&self, key: K, value: V, seed: usize) -> bool {
        unsafe {
            let top_level = core::cmp::min(seed.trailing_zeros() as usize, MAX_LEVEL - 1);
            let tower = Self::build_tower(key.clone(), value, top_level);
            let root_node = tower[0];

            for level in 0..=top_level {
                let curr_insert_node = tower[level];
                let mut prev_node = self.head_tower[level];

                loop {
                    if level > 0 && (*root_node).load_successor().mark {
                        // our tower just've been deleted
                        return true;
                    }

                    // algorithms below should be similar to linked list
                    let (p, next_node) = self.search_from(&key, prev_node);
                    prev_node = p;

                    if level == 0 && !next_node.is_null() && (*next_node).key == key {
                        Self::prune_tower(tower);
                        return false;
                    }

                    let prev_succ = (*prev_node).load_successor();

                    if prev_succ.flag {
                        self.help_flagged(prev_node, prev_succ.ptr);
                    } else {
                        (*curr_insert_node).store_successor(SuccData::new(next_node));

                        let expected = SuccData::new(next_node);
                        let new_val = SuccData::new(curr_insert_node);

                        match (*prev_node).swap_successor(expected, new_val) {
                            Ok(_) => break,
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
                            }
                        }
                    }
                }
            }
        }
        true
    }

    pub fn delete(&self, key: &K) -> bool {
        unsafe {
            let mut curr_node = self.head_tower[MAX_LEVEL - 1];
            let mut target_root = ptr::null_mut();

            for _ in (0..MAX_LEVEL).rev() {
                let (prev, next) = self.search_from(key, curr_node);

                if !next.is_null() && (*next).key == *key {
                    target_root = (*next).tower_root;
                    break;
                }
                curr_node = (*prev).down;
                if curr_node.is_null() {
                    break;
                }
            }

            if target_root.is_null() {
                return false;
            }

            let mut prev_root = self.head_tower[0];
            loop {
                let (p, n) = self.search_from(key, prev_root);

                if n != target_root {
                    return false;
                }

                let (actual_prev, result) = self.try_flag(p, target_root);

                if !actual_prev.is_null() {
                    self.help_flagged(actual_prev, target_root);

                    if result {
                        for level in 1..MAX_LEVEL {
                            self.search_from(key, self.head_tower[level]);
                        }
                        return true;
                    }
                } else if !result {
                    return false;
                }

                prev_root = actual_prev;
            }
        }
    }

    pub fn iter(&self) -> NodeIter<'_, K, V, SkipNode<K, V>> {
        unsafe {
            let head_ptr = self.head_tower[0];
            let first_node = (*head_ptr).load_successor().ptr;
            NodeIter::new(first_node)
        }
    }
}

impl<K: Clone + Default + Ord + Seedable, V> LockFreeSkipList<K, V> {
    pub fn insert(&self, key: K, value: V) -> bool {
        let seed = key.generate_seed();
        self.insert_seeded(key, value, seed)
    }
}

impl<K: Default + Ord, V> List<K, V, SkipNode<K, V>> for LockFreeSkipList<K, V> {
    /// Search from curr_node to the right
    unsafe fn search_from(
        &self,
        k: &K,
        mut curr_node: *mut SkipNode<K, V>,
    ) -> (*mut SkipNode<K, V>, *mut SkipNode<K, V>) {
        unsafe {
            let mut next_node = (*curr_node).load_successor().ptr;

            while !next_node.is_null() && (*next_node).key < *k {
                let mut curr_succ_val = (*curr_node).load_successor();
                let mut next_succ_val = (*next_node).load_successor();

                let tower_root = (*next_node).tower_root;
                let is_superfluous = !tower_root.is_null() && (*tower_root).load_successor().mark;

                while (next_succ_val.mark || is_superfluous)
                    && (!curr_succ_val.mark || curr_succ_val.ptr != next_node)
                {
                    if is_superfluous && !next_succ_val.mark {
                        let (actual_prev, _) = self.try_flag(curr_node, next_node);
                        if !actual_prev.is_null() {
                            self.help_flagged(actual_prev, next_node);
                        }
                    } else if curr_succ_val.ptr == next_node {
                        self.help_unflag(curr_node, next_node);
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

    unsafe fn search_node(&self, key: &K) -> Option<*mut SkipNode<K, V>> {
        unsafe {
            // Start from the highest tower
            let mut curr_node = self.head_tower[MAX_LEVEL - 1];

            loop {
                let next_node = (*curr_node).load_successor().ptr;
                if next_node.is_null() || (*next_node).key > *key {
                    // Loop down one level
                    curr_node = (*curr_node).down;
                    if curr_node.is_null() {
                        return None;
                    }
                } else if (*next_node).key == *key {
                    // Make sure it's not deleted before returning
                    let root_node = (*next_node).tower_root;
                    return if (*root_node).load_successor().mark {
                        None
                    } else {
                        Some(next_node)
                    };
                } else {
                    // Loop to the right
                    curr_node = next_node;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "std")]
    use crate::{DefaultGC, ScopedGarbageCollector};
    #[cfg(not(feature = "std"))]
    use alloc::{vec, vec::Vec};
    #[cfg(feature = "std")]
    use std::{
        sync::{Arc, Mutex},
        thread,
    };

    #[test]
    fn test_basic_sequential_operations() {
        let list = LockFreeSkipList::<i32, &'static str>::new();

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
        let list = Arc::new(LockFreeSkipList::<i32, i32>::new());
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
    #[cfg(feature = "std")]
    fn test_concurrent_inserts_and_deletes() {
        let list = Arc::new(LockFreeSkipList::<i32, i32>::new());
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

        for i in 1..=1000 {
            assert!(!list.contains(&i), "Key {} should have been deleted", i);
        }
        // definitely more than one object
        assert!(collector.lock().unwrap().prune() > 1000);
    }

    #[test]
    fn test_iteration_and_sorting() {
        let list = LockFreeSkipList::<i32, &'static str>::new();

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
