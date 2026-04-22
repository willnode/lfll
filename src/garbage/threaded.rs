use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::vec::Drain;

use crate::garbage::GarbageItem;
use crate::{GarbageCollector, ScopedGarbageCollector};

static GLOBAL_TRASH: Mutex<Vec<GarbageItem>> = Mutex::new(Vec::new());

struct ThreadLocalBag {
    garbage: Vec<GarbageItem>,
}

impl ThreadLocalBag {
    fn new() -> Self {
        Self {
            garbage: Vec::new(),
        }
    }
    pub fn push(&mut self, item: GarbageItem) {
        self.garbage.push(item);
    }
    pub fn drain<'a>(&'a mut self) -> Drain<'a, GarbageItem> {
        self.garbage.drain(..)
    }
}

impl Drop for ThreadLocalBag {
    fn drop(&mut self) {
        if !self.garbage.is_empty() {
            let mut global_bag = GLOBAL_TRASH.lock().unwrap();
            global_bag.extend_from_slice(&self.garbage[..]);
        }
    }
}

thread_local! {
    static LOCAL_TRASH: RefCell<ThreadLocalBag> = RefCell::new(ThreadLocalBag::new());
}

pub struct ThreadedGC {
    garbage: Vec<GarbageItem>,
}

impl<N> GarbageCollector<N> for ThreadedGC {
    fn push(del: *mut N) {
        let ptr = del as *mut u8;

        LOCAL_TRASH.with(|s| {
            s.borrow_mut().push(GarbageItem {
                ptr,
                dropper: <Self as GarbageCollector<N>>::dropper,
            });
        })
    }
}

impl ScopedGarbageCollector for ThreadedGC {
    fn new() -> std::sync::Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            garbage: Vec::new(),
        }))
    }

    fn collect(&mut self) {
        LOCAL_TRASH.with(|s| {
            self.garbage
                .extend_from_slice(&s.borrow_mut().drain().collect::<Vec<_>>())
        })
    }

    unsafe fn prune_now() -> usize {
        LOCAL_TRASH.with(|s| {
            let mut i = 0;
            for item in s.borrow_mut().drain() {
                unsafe { (item.dropper)(item.ptr) };
                i += 1;
            }
            i
        })
    }

    fn prune_all() -> usize {
        let mut global_bag = GLOBAL_TRASH.lock().unwrap();
        let mut i = 0;
        for item in global_bag.drain(..) {
            unsafe { (item.dropper)(item.ptr) };
            i += 1;
        }
        i
    }

    fn prune(&mut self) -> usize {
        let mut i = 0;
        for item in self.garbage.drain(..) {
            unsafe { (item.dropper)(item.ptr) };
            i += 1;
        }
        i
    }
}
